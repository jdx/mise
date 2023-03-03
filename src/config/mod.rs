use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use color_eyre::Report;
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use rayon::prelude::*;

pub use settings::{MissingRuntimeBehavior, Settings};

use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::ConfigFile;
use crate::config::settings::SettingsBuilder;
use crate::plugins::{Plugin, PluginName};
use crate::shorthands::{get_shorthands, Shorthands};
use crate::{dirs, env, file, hook_env};

pub mod config_file;
mod settings;

type AliasMap = IndexMap<PluginName, IndexMap<String, String>>;

#[derive(Debug, Default)]
pub struct Config {
    pub settings: Settings,
    pub global_config: RtxToml,
    pub legacy_files: IndexMap<String, PluginName>,
    pub config_files: IndexMap<PathBuf, Box<dyn ConfigFile>>,
    pub plugins: IndexMap<PluginName, Arc<Plugin>>,
    pub env: IndexMap<String, String>,
    pub aliases: AliasMap,
    pub all_aliases: OnceCell<AliasMap>,
    pub should_exit_early: bool,
    shorthands: OnceCell<HashMap<String, String>>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut plugins = load_plugins()?;
        let global_config = load_rtxrc()?;
        let mut settings = SettingsBuilder::default();
        let config_filenames = load_config_filenames(&IndexMap::new());
        let config_files = load_all_config_files(
            &settings.build(),
            &config_filenames,
            &plugins,
            &IndexMap::new(),
            IndexMap::new(),
        );
        for cf in config_files.values() {
            settings.merge(cf.settings());
        }
        let settings = settings.build();
        trace!("Settings: {:#?}", settings);

        let legacy_files = load_legacy_files(&settings, &plugins);
        let config_filenames = load_config_filenames(&legacy_files);

        let (config_files, should_exit_early) = rayon::join(
            || {
                load_all_config_files(
                    &settings,
                    &config_filenames,
                    &plugins,
                    &legacy_files,
                    config_files,
                )
            },
            || hook_env::should_exit_early(&config_filenames),
        );

        for cf in config_files.values() {
            for (plugin_name, repo_url) in cf.plugins() {
                plugins.entry(plugin_name.clone()).or_insert_with(|| {
                    let mut plugin = Plugin::new(&plugin_name);
                    plugin.repo_url = Some(repo_url);
                    Arc::new(plugin)
                });
            }
        }

        let config = Self {
            env: load_env(&config_files),
            aliases: load_aliases(&config_files),
            all_aliases: OnceCell::new(),
            shorthands: OnceCell::new(),
            config_files,
            settings,
            legacy_files,
            global_config,
            plugins,
            should_exit_early,
        };

        debug!("{}", &config);

        Ok(config)
    }

    pub fn get_shorthands(&self) -> &Shorthands {
        self.shorthands
            .get_or_init(|| get_shorthands(&self.settings))
    }

    pub fn get_all_aliases(&self) -> &AliasMap {
        self.all_aliases.get_or_init(|| self.load_all_aliases())
    }

    pub fn is_activated(&self) -> bool {
        env::var("__RTX_DIFF").is_ok()
    }

    fn load_all_aliases(&self) -> AliasMap {
        let mut aliases: AliasMap = self.aliases.clone();
        let plugin_aliases: Vec<_> = self
            .plugins
            .values()
            .par_bridge()
            .map(|plugin| {
                let aliases = match plugin.get_aliases(&self.settings) {
                    Ok(aliases) => aliases,
                    Err(err) => {
                        eprintln!("Error: {err}");
                        IndexMap::new()
                    }
                };
                (&plugin.name, aliases)
            })
            .collect();
        for (plugin, plugin_aliases) in plugin_aliases {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(plugin.clone())
                    .or_insert_with(IndexMap::new)
                    .insert(from, to);
            }
        }

        for (plugin, plugin_aliases) in &self.aliases {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(plugin.clone())
                    .or_insert_with(IndexMap::new)
                    .insert(from.clone(), to.clone());
            }
        }

        aliases
    }

    pub fn get_shims_dir(&self) -> Result<PathBuf> {
        match self.settings.shims_dir.clone() {
            Some(mut shims_dir) => {
                if shims_dir.starts_with("~") {
                    shims_dir = dirs::HOME.join(shims_dir.strip_prefix("~")?);
                }
                Ok(shims_dir)
            }
            None => err_no_shims_dir(),
        }
    }
}

fn load_rtxrc() -> Result<RtxToml> {
    let settings_path = dirs::CONFIG.join("config.toml");
    match settings_path.exists() {
        false => {
            trace!("settings does not exist {:?}", settings_path);
            Ok(RtxToml::init(&settings_path))
        }
        true => match RtxToml::from_file(&settings_path)
            .wrap_err_with(|| err_load_settings(&settings_path))
        {
            Ok(cf) => Ok(cf),
            Err(err) => match RtxToml::migrate(&settings_path) {
                Ok(cf) => Ok(cf),
                Err(e) => {
                    error!("Error migrating config.toml: {:#}", e);
                    Err(err)
                }
            },
        },
    }
}

fn load_plugins() -> Result<IndexMap<PluginName, Arc<Plugin>>> {
    let plugins = Plugin::list()?
        .into_par_iter()
        .map(|p| (p.name.clone(), Arc::new(p)))
        .collect::<Vec<_>>()
        .into_iter()
        .sorted_by_cached_key(|(p, _)| p.to_string())
        .collect();
    Ok(plugins)
}

fn load_legacy_files(
    settings: &Settings,
    plugins: &IndexMap<PluginName, Arc<Plugin>>,
) -> IndexMap<String, PluginName> {
    if !settings.legacy_version_file {
        return IndexMap::new();
    }
    plugins
        .values()
        .collect_vec()
        .into_par_iter()
        .filter_map(|plugin| match plugin.legacy_filenames(settings) {
            Ok(filenames) => Some(
                filenames
                    .iter()
                    .map(|f| (f.to_string(), plugin.name.clone()))
                    .collect_vec(),
            ),
            Err(err) => {
                eprintln!("Error: {err}");
                None
            }
        })
        .collect::<Vec<Vec<(String, PluginName)>>>()
        .into_iter()
        .flatten()
        .collect()
}

fn load_config_filenames(legacy_filenames: &IndexMap<String, PluginName>) -> Vec<PathBuf> {
    let mut filenames = vec![
        env::RTX_DEFAULT_CONFIG_FILENAME.as_str(),
        env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str(),
    ];
    for filename in legacy_filenames.keys() {
        filenames.push(filename.as_str());
    }
    filenames.reverse();

    let mut config_files = file::FindUp::new(&dirs::CURRENT, &filenames).collect::<Vec<_>>();

    match env::RTX_GLOBAL_FILE.clone() {
        Some(global) => {
            if global.is_file() {
                config_files.push(global);
            }
        }
        None => {
            let home_config = dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
            if home_config.is_file() {
                config_files.push(home_config);
            }
            let global_config = dirs::CONFIG.join("config.toml");
            if global_config.is_file() {
                config_files.push(global_config);
            }
        }
    };

    config_files.into_iter().unique().collect()
}

fn load_all_config_files(
    settings: &Settings,
    config_filenames: &[PathBuf],
    plugins: &IndexMap<PluginName, Arc<Plugin>>,
    legacy_filenames: &IndexMap<String, PluginName>,
    mut existing: IndexMap<PathBuf, Box<dyn ConfigFile>>,
) -> IndexMap<PathBuf, Box<dyn ConfigFile>> {
    config_filenames
        .iter()
        .unique()
        .map(|f| (f.clone(), existing.shift_remove(f)))
        .collect_vec()
        .into_par_iter()
        .map(|(f, existing)| match existing {
            // already parsed so just return it
            Some(cf) => Some((f, cf)),
            // need to parse this config file
            None => match parse_config_file(&f, settings, legacy_filenames, plugins) {
                Ok(cf) => Some((f, cf)),
                Err(err) => {
                    warn!("error parsing: {} {:#}", f.display(), err);
                    None
                }
            },
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flatten()
        .collect()
}

fn parse_config_file(
    f: &PathBuf,
    settings: &Settings,
    legacy_filenames: &IndexMap<String, PluginName>,
    plugins: &IndexMap<PluginName, Arc<Plugin>>,
) -> Result<Box<dyn ConfigFile>> {
    match legacy_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => LegacyVersionFile::parse(settings, f.into(), plugins.get(plugin).unwrap())
            .map(|f| Box::new(f) as Box<dyn ConfigFile>),
        None => config_file::parse(f),
    }
}

fn load_env(config_files: &IndexMap<PathBuf, Box<dyn ConfigFile>>) -> IndexMap<String, String> {
    let mut env = IndexMap::new();
    for cf in config_files.values() {
        env.extend(cf.env());
    }
    env
}

fn load_aliases(config_files: &IndexMap<PathBuf, Box<dyn ConfigFile>>) -> AliasMap {
    let mut aliases: AliasMap = IndexMap::new();

    for config_file in config_files.values() {
        for (plugin, plugin_aliases) in config_file.aliases() {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(plugin.clone())
                    .or_insert_with(IndexMap::new)
                    .insert(from, to);
            }
        }
    }

    aliases
}

fn err_load_settings(settings_path: &Path) -> Report {
    eyre!(
        "error loading settings from {}",
        settings_path.to_string_lossy()
    )
}

fn err_no_shims_dir() -> Result<PathBuf> {
    return Err(eyre!(indoc::formatdoc!(
        r#"
           rtx is not configured to use shims.
           Please set the `{}` setting to a directory.
           "#,
        style("shims_dir").yellow()
    )));
}

impl Display for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let plugins = self
            .plugins
            .keys()
            .map(|p| p.to_string())
            .collect::<Vec<_>>();
        let config_files = self
            .config_files
            .iter()
            .map(|(p, _)| {
                p.to_string_lossy()
                    .to_string()
                    .replace(&dirs::HOME.to_string_lossy().to_string(), "~")
            })
            .collect::<Vec<_>>();
        writeln!(f, "Files: {}", config_files.join(", "))?;
        write!(f, "Installed Plugins: {}", plugins.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_display_snapshot;

    use super::*;

    #[test]
    fn test_load() {
        let config = Config::load().unwrap();
        assert_display_snapshot!(config);
    }
}
