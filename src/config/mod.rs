use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use color_eyre::eyre::{eyre, Result};
use console::style;
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

pub use settings::{MissingRuntimeBehavior, Settings};

use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::tracking::Tracker;
use crate::env::CI;
use crate::plugins::{Plugin, PluginName};
use crate::shorthands::{get_shorthands, Shorthands};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{cli, dirs, duration, env, file, hook_env};

pub mod config_file;
mod settings;
mod tracking;

type AliasMap = IndexMap<PluginName, IndexMap<String, String>>;
type ConfigMap = IndexMap<PathBuf, Box<dyn ConfigFile>>;

#[derive(Debug, Default)]
pub struct Config {
    pub settings: Settings,
    pub global_config: RtxToml,
    pub legacy_files: IndexMap<String, PluginName>,
    pub config_files: ConfigMap,
    pub plugins: IndexMap<PluginName, Arc<Plugin>>,
    pub env: IndexMap<String, String>,
    pub path_dirs: Vec<PathBuf>,
    pub aliases: AliasMap,
    pub all_aliases: OnceCell<AliasMap>,
    pub should_exit_early: bool,
    pub project_root: Option<PathBuf>,
    shorthands: OnceCell<HashMap<String, String>>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let global_config = load_rtxrc()?;
        let mut settings = global_config.settings();
        let config_filenames = load_config_filenames(&IndexMap::new());
        let mut plugins = load_plugins(&settings.build())?;
        let config_files = load_all_config_files(
            &settings.build(),
            &config_filenames,
            &plugins,
            &IndexMap::new(),
            IndexMap::new(),
        )?;
        for cf in config_files.values() {
            settings.merge(cf.settings());
        }
        let settings = settings.build();
        trace!("Settings: {:#?}", settings);

        let legacy_files = load_legacy_files(&settings, &plugins);
        let config_filenames = load_config_filenames(&legacy_files);
        let config_track = track_config_files(&config_filenames);

        let config_files = load_all_config_files(
            &settings,
            &config_filenames,
            &plugins,
            &legacy_files,
            config_files,
        );
        let config_files = config_files?;
        let watch_files = config_files
            .values()
            .flat_map(|cf| cf.watch_files())
            .collect_vec();
        let should_exit_early = hook_env::should_exit_early(&watch_files);

        for cf in config_files.values() {
            for (plugin_name, repo_url) in cf.plugins() {
                plugins.entry(plugin_name.clone()).or_insert_with(|| {
                    let mut plugin = Plugin::new(&settings, &plugin_name);
                    plugin.repo_url = Some(repo_url);
                    Arc::new(plugin)
                });
            }
        }
        config_track.join().unwrap();

        let config = Self {
            env: load_env(&config_files),
            path_dirs: load_path_dirs(&config_files),
            aliases: load_aliases(&config_files),
            all_aliases: OnceCell::new(),
            shorthands: OnceCell::new(),
            project_root: get_project_root(&config_files),
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

    pub fn resolve_alias(&self, plugin_name: &PluginName, v: &str) -> Result<String> {
        if let Some(plugin_aliases) = self.aliases.get(plugin_name) {
            if let Some(alias) = plugin_aliases.get(v) {
                return Ok(alias.clone());
            }
        }
        if let Some(plugin) = self.plugins.get(plugin_name) {
            if let Some(alias) = plugin.get_aliases(&self.settings)?.get(v) {
                return Ok(alias.clone());
            }
        }
        Ok(v.to_string())
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

    pub fn get_tracked_config_files(&self) -> Result<IndexMap<PathBuf, Box<dyn ConfigFile>>> {
        let tracker = Tracker::new();
        let config_files = tracker
            .list_all()?
            .into_par_iter()
            .map(|path| {
                match config_file::parse(&path, config_file::is_trusted(&self.settings, &path)) {
                    Ok(cf) => Some((path, cf)),
                    Err(err) => {
                        error!("Error loading config file: {:#}", err);
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect();
        Ok(config_files)
    }

    pub fn rtx_bin(&self) -> Option<PathBuf> {
        for path in &*env::PATH {
            let rtx_bin = path.join("rtx");
            if file::is_executable(&rtx_bin) {
                return Some(rtx_bin);
            }
        }
        None
    }

    pub fn autoupdate(&self) {
        if *CI {
            return;
        }
        self.check_for_new_version();
        let thread_pool = match ThreadPoolBuilder::new()
            .num_threads(self.settings.jobs)
            .build()
        {
            Ok(thread_pool) => thread_pool,
            Err(err) => {
                warn!("Error building thread pool: {:#}", err);
                return;
            }
        };
        if let Err(err) = thread_pool.install(|| -> Result<()> {
            let plugins: Vec<Arc<Plugin>> = self
                .plugins
                .values()
                .collect_vec()
                .into_par_iter()
                .filter(|p| match p.needs_autoupdate(&self.settings) {
                    Ok(should_autoupdate) => should_autoupdate,
                    Err(err) => {
                        debug!("Error checking for autoupdate: {:#}", err);
                        false
                    }
                })
                .cloned()
                .collect();
            if plugins.is_empty() {
                return Ok(());
            }
            let mpr = MultiProgressReport::new(self.settings.verbose);
            plugins
                .into_par_iter()
                .map(|plugin| {
                    let mut pr = mpr.add();
                    plugin.autoupdate(&mut pr)?;
                    pr.clear();
                    Ok(())
                })
                .collect::<Result<_>>()?;
            Ok(())
        }) {
            warn!("Error autoupdating: {:#}", err);
        }
    }

    pub fn check_for_new_version(&self) {
        if !console::user_attended_stderr() || *env::RTX_HIDE_UPDATE_WARNING {
            return; // not a tty so don't bother
        }
        if let Some(latest) = cli::version::check_for_new_version(duration::WEEKLY) {
            warn!(
                "newer rtx version {} available, currently on {}",
                latest,
                env!("CARGO_PKG_VERSION")
            );
        }
    }
}

fn get_project_root(config_files: &ConfigMap) -> Option<PathBuf> {
    for (p, cf) in config_files.into_iter() {
        if p == &get_global_rtx_toml() {
            // ~/.config/rtx/config.toml is not a project config file
            continue;
        }
        match cf.get_type() {
            ConfigFileType::RtxToml | ConfigFileType::ToolVersions => {
                return Some(p.parent()?.to_path_buf());
            }
            _ => {}
        }
    }
    None
}

fn load_rtxrc() -> Result<RtxToml> {
    let settings_path = env::RTX_CONFIG_FILE
        .clone()
        .unwrap_or(dirs::CONFIG.join("config.toml"));
    let is_trusted = config_file::is_trusted(&Settings::default(), &settings_path);
    match settings_path.exists() {
        false => {
            trace!("settings does not exist {:?}", settings_path);
            Ok(RtxToml::init(&settings_path, is_trusted))
        }
        true => match RtxToml::from_file(&settings_path, is_trusted) {
            Ok(cf) => Ok(cf),
            Err(err) => match RtxToml::migrate(&settings_path, is_trusted) {
                Ok(cf) => Ok(cf),
                Err(e) => {
                    trace!("Error migrating config.toml: {:#}", e);
                    Err(eyre!(
                        "Error parsing {}: {:#}",
                        &settings_path.display(),
                        err
                    ))
                }
            },
        },
    }
}

fn load_plugins(settings: &Settings) -> Result<IndexMap<PluginName, Arc<Plugin>>> {
    let plugins = Plugin::list(settings)?
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

    if env::RTX_CONFIG_FILE.is_none() && !*env::RTX_USE_TOML {
        // only add ~/.tool-versions if RTX_CONFIG_FILE is not set
        // because that's how the user overrides the default
        let home_config = dirs::HOME.join(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
        if home_config.is_file() {
            config_files.push(home_config);
        }
    };
    let global_config = get_global_rtx_toml();
    if global_config.is_file() {
        config_files.push(global_config);
    }

    config_files.into_iter().unique().collect()
}

fn get_global_rtx_toml() -> PathBuf {
    match env::RTX_CONFIG_FILE.clone() {
        Some(global) => global,
        None => dirs::CONFIG.join("config.toml"),
    }
}

fn load_all_config_files(
    settings: &Settings,
    config_filenames: &[PathBuf],
    plugins: &IndexMap<PluginName, Arc<Plugin>>,
    legacy_filenames: &IndexMap<String, PluginName>,
    mut existing: IndexMap<PathBuf, Box<dyn ConfigFile>>,
) -> Result<ConfigMap> {
    Ok(config_filenames
        .iter()
        .unique()
        .map(|f| (f.clone(), existing.shift_remove(f)))
        .collect_vec()
        .into_par_iter()
        .map(|(f, existing)| match existing {
            // already parsed so just return it
            Some(cf) => Ok((f, cf)),
            // need to parse this config file
            None => match parse_config_file(&f, settings, legacy_filenames, plugins) {
                Ok(cf) => Ok((f, cf)),
                Err(err) => Err(eyre!("error parsing: {} {:#}", f.display(), err)),
            },
        })
        .collect::<Vec<Result<_>>>()
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect())
}

fn parse_config_file(
    f: &PathBuf,
    settings: &Settings,
    legacy_filenames: &IndexMap<String, PluginName>,
    plugins: &IndexMap<PluginName, Arc<Plugin>>,
) -> Result<Box<dyn ConfigFile>> {
    let is_trusted = config_file::is_trusted(settings, f);
    match legacy_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => LegacyVersionFile::parse(settings, f.into(), plugins.get(plugin).unwrap())
            .map(|f| Box::new(f) as Box<dyn ConfigFile>),
        None => config_file::parse(f, is_trusted),
    }
}

fn load_env(config_files: &IndexMap<PathBuf, Box<dyn ConfigFile>>) -> IndexMap<String, String> {
    let mut env = IndexMap::new();
    for cf in config_files.values().rev() {
        env.extend(cf.env());
    }
    env
}

fn load_path_dirs(config_files: &IndexMap<PathBuf, Box<dyn ConfigFile>>) -> Vec<PathBuf> {
    let mut path_dirs = vec![];
    for cf in config_files.values().rev() {
        path_dirs.extend(cf.path_dirs());
    }
    path_dirs
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

fn err_no_shims_dir() -> Result<PathBuf> {
    Err(eyre!(indoc::formatdoc!(
        r#"
           rtx is not configured to use shims.
           Please set the `{}` setting to a directory.
           "#,
        style("shims_dir").yellow()
    )))
}

fn track_config_files(config_filenames: &[PathBuf]) -> thread::JoinHandle<()> {
    let config_filenames = config_filenames.to_vec();
    let track = move || -> Result<()> {
        let mut tracker = Tracker::new();
        for config_file in &config_filenames {
            tracker.track(config_file)?;
        }
        Ok(())
    };
    thread::spawn(move || {
        if let Err(err) = track() {
            warn!("tracking config files: {:#}", err);
        }
    })
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
