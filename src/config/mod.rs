use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result, WrapErr};
use color_eyre::Report;
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use rayon::prelude::*;

pub use settings::{MissingRuntimeBehavior, Settings};

use crate::config::config_file::rtxrc::RTXFile;
use crate::plugins::{Plugin, PluginName};
use crate::shorthands::{get_shorthands, Shorthands};
use crate::{dirs, env, file};

pub mod config_file;
mod settings;

type AliasMap = IndexMap<PluginName, IndexMap<String, String>>;

#[derive(Debug, Default)]
pub struct Config {
    pub settings: Settings,
    pub rtxrc: RTXFile,
    pub legacy_files: IndexMap<String, PluginName>,
    pub config_files: Vec<PathBuf>,
    pub aliases: AliasMap,
    pub plugins: IndexMap<PluginName, Arc<Plugin>>,
    shorthands: OnceCell<HashMap<String, String>>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let rtxrc = load_rtxrc()?;
        let settings = rtxrc.settings();
        let plugins = load_plugins()?;
        let legacy_files = load_legacy_files(&settings, &plugins);
        let config_files = find_all_config_files(&legacy_files);
        let aliases = load_aliases(&settings, &plugins);

        let config = Self {
            settings,
            legacy_files,
            config_files,
            aliases,
            rtxrc,
            plugins,
            shorthands: OnceCell::new(),
        };

        debug!("{}", &config);

        Ok(config)
    }

    pub fn get_shorthands(&self) -> &Shorthands {
        self.shorthands
            .get_or_init(|| get_shorthands(&self.settings))
    }
}

fn load_rtxrc() -> Result<RTXFile> {
    let settings_path = dirs::CONFIG.join("config.toml");
    let rtxrc = if !settings_path.exists() {
        trace!("settings does not exist {:?}", settings_path);
        RTXFile::init(&settings_path)
    } else {
        let rtxrc = RTXFile::from_file(&settings_path)
            .wrap_err_with(|| err_load_settings(&settings_path))?;
        trace!("Settings: {:#?}", rtxrc.settings());
        rtxrc
    };

    Ok(rtxrc)
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

fn find_all_config_files(legacy_filenames: &IndexMap<String, PluginName>) -> Vec<PathBuf> {
    let mut filenames = vec![
        // ".rtxrc.toml",
        // ".rtxrc",
        env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str(),
    ];
    for filename in legacy_filenames.keys() {
        filenames.push(filename.as_str());
    }
    filenames.reverse();

    let mut config_files = file::FindUp::new(&dirs::CURRENT, &filenames).collect::<Vec<_>>();

    let home_config = dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
    if home_config.is_file() {
        config_files.push(home_config);
    }

    config_files.into_iter().unique().collect()
}

fn load_aliases(settings: &Settings, plugins: &IndexMap<PluginName, Arc<Plugin>>) -> AliasMap {
    let mut aliases: AliasMap = IndexMap::new();
    let plugin_aliases: Vec<_> = plugins
        .values()
        .par_bridge()
        .map(|plugin| {
            let aliases = match plugin.get_aliases(settings) {
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

    for (plugin, plugin_aliases) in &settings.aliases {
        for (from, to) in plugin_aliases {
            aliases
                .entry(plugin.clone())
                .or_insert_with(IndexMap::new)
                .insert(from.clone(), to.clone());
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
            .map(|p| {
                p.to_string_lossy()
                    .to_string()
                    .replace(&dirs::HOME.to_string_lossy().to_string(), "~")
            })
            .collect::<Vec<_>>();
        writeln!(f, "Config:")?;
        writeln!(f, "  Files: {}", config_files.join(", "))?;
        write!(f, "  Installed Plugins: {}", plugins.join(", "))
    }
}
