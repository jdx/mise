use std::collections::{BTreeMap, HashMap};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::thread;

use eyre::Context;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::OnceCell;
use rayon::prelude::*;

pub use settings::{Settings, SettingsPartial};

use crate::cli::Cli;
use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::rtx_toml::RtxToml;
use crate::config::config_file::{ConfigFile, ConfigFileType};
use crate::config::tracking::Tracker;
use crate::file::display_path;
use crate::plugins::core::{PluginMap, CORE_PLUGINS, EXPERIMENTAL_CORE_PLUGINS};
use crate::plugins::{ExternalPlugin, Plugin, PluginName, PluginType};
use crate::shorthands::{get_shorthands, Shorthands};
use crate::{dirs, env, file, hook_env};

pub mod config_file;
mod settings;
mod tracking;

type AliasMap = BTreeMap<PluginName, BTreeMap<String, String>>;
type ConfigMap = IndexMap<PathBuf, Box<dyn ConfigFile>>;
type ToolMap = BTreeMap<PluginName, Arc<dyn Plugin>>;

#[derive(Debug, Default)]
pub struct Config {
    pub global_config: RtxToml,
    pub config_files: ConfigMap,
    pub env: BTreeMap<String, String>,
    pub env_sources: HashMap<String, PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub aliases: AliasMap,
    pub all_aliases: OnceCell<AliasMap>,
    pub should_exit_early: bool,
    pub project_root: Option<PathBuf>,
    plugins: RwLock<ToolMap>,
    shorthands: OnceCell<HashMap<String, String>>,
    repo_urls: HashMap<PluginName, String>,
}

static CONFIG: RwLock<Option<Arc<Config>>> = RwLock::new(None);

impl Config {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }
    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(config) = &*CONFIG.read().unwrap() {
            return Ok(config.clone());
        }
        let config = Arc::new(Self::load()?);
        *CONFIG.write().unwrap() = Some(config.clone());
        Ok(config)
    }
    pub fn load() -> Result<Self> {
        let cli_settings = Cli::new().settings(&env::ARGS.read().unwrap());
        Settings::add_partial(cli_settings);
        let global_config = load_rtxrc()?;
        Settings::add_partial(global_config.settings()?);
        let config_filenames = load_config_filenames(&BTreeMap::new());
        let settings = Settings::try_get()?;
        let plugins = load_plugins(&settings)?;
        let config_files = load_all_config_files(
            &config_filenames,
            &plugins,
            &BTreeMap::new(),
            ConfigMap::new(),
        )?;
        for cf in config_files.values() {
            Settings::add_partial(cf.settings()?);
        }
        let settings = Settings::try_get()?;
        trace!("Settings: {:#?}", settings);

        let legacy_files = load_legacy_files(&settings, &plugins);
        let config_filenames = load_config_filenames(&legacy_files);
        let config_track = track_config_files(&config_filenames);

        let config_files =
            load_all_config_files(&config_filenames, &plugins, &legacy_files, config_files);
        let config_files = config_files?;
        let watch_files = config_files
            .values()
            .flat_map(|cf| cf.watch_files())
            .collect_vec();
        let should_exit_early = hook_env::should_exit_early(&watch_files);

        let mut repo_urls = HashMap::new();
        for cf in config_files.values() {
            for (plugin_name, repo_url) in cf.plugins() {
                repo_urls.insert(plugin_name, repo_url);
            }
        }
        config_track.join().unwrap();

        let (env, env_sources) = load_env(&config_files);

        let config = Self {
            env,
            env_sources,
            path_dirs: load_path_dirs(&config_files),
            aliases: load_aliases(&config_files),
            all_aliases: OnceCell::new(),
            shorthands: OnceCell::new(),
            project_root: get_project_root(&config_files),
            config_files,
            global_config,
            plugins: RwLock::new(plugins),
            should_exit_early,
            repo_urls,
        };

        debug!("{}", &config);

        Ok(config)
    }
    pub fn get_shorthands(&self) -> &Shorthands {
        self.shorthands.get_or_init(get_shorthands)
    }

    pub fn get_repo_url(&self, plugin_name: &PluginName) -> Option<String> {
        match self.repo_urls.get(plugin_name) {
            Some(url) => Some(url),
            None => self.get_shorthands().get(plugin_name),
        }
        .cloned()
    }

    pub fn get_all_aliases(&self) -> &AliasMap {
        self.all_aliases.get_or_init(|| self.load_all_aliases())
    }

    pub fn is_activated(&self) -> bool {
        env::var("__RTX_DIFF").is_ok()
    }

    pub fn resolve_alias(&self, plugin_name: &str, v: &str) -> Result<String> {
        if let Some(plugin_aliases) = self.aliases.get(plugin_name) {
            if let Some(alias) = plugin_aliases.get(v) {
                return Ok(alias.clone());
            }
        }
        if let Some(plugin) = self.plugins.read().unwrap().get(plugin_name) {
            if let Some(alias) = plugin.get_aliases()?.get(v) {
                return Ok(alias.clone());
            }
        }
        Ok(v.to_string())
    }

    pub fn external_plugins(&self) -> Vec<(String, Arc<dyn Plugin>)> {
        self.list_plugins()
            .into_iter()
            .filter(|tool| matches!(tool.get_type(), PluginType::External))
            .map(|tool| (tool.name().to_string(), tool.clone()))
            .collect()
    }

    pub fn get_or_create_plugin(&self, plugin_name: &str) -> Arc<dyn Plugin> {
        if let Some(plugin) = self.plugins.read().unwrap().get(plugin_name) {
            return plugin.clone();
        }
        let plugin = ExternalPlugin::newa(plugin_name.to_string());
        self.plugins
            .write()
            .unwrap()
            .insert(plugin_name.to_string(), plugin.clone());
        plugin
    }
    pub fn list_plugins(&self) -> Vec<Arc<dyn Plugin>> {
        self.plugins.read().unwrap().values().cloned().collect()
    }

    fn load_all_aliases(&self) -> AliasMap {
        let mut aliases: AliasMap = self.aliases.clone();
        let plugin_aliases: Vec<_> = self
            .list_plugins()
            .into_par_iter()
            .map(|plugin| {
                let aliases = plugin.get_aliases().unwrap_or_else(|err| {
                    warn!("get_aliases: {err}");
                    BTreeMap::new()
                });
                (plugin.name().to_string(), aliases)
            })
            .collect();
        for (plugin, plugin_aliases) in plugin_aliases {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(plugin.to_string())
                    .or_default()
                    .insert(from, to);
            }
        }

        for (plugin, plugin_aliases) in &self.aliases {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(plugin.clone())
                    .or_default()
                    .insert(from.clone(), to.clone());
            }
        }

        aliases
    }

    pub fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        let tracker = Tracker::new();
        let config_files = tracker
            .list_all()?
            .into_par_iter()
            .map(|path| match config_file::parse(&path) {
                Ok(cf) => Some((path, cf)),
                Err(err) => {
                    error!("Error loading config file: {:#}", err);
                    None
                }
            })
            .collect::<Vec<_>>()
            .into_iter()
            .flatten()
            .collect();
        Ok(config_files)
    }

    pub fn rebuild_shims_and_runtime_symlinks(&self) -> Result<()> {
        let ts = crate::toolset::ToolsetBuilder::new().build(self)?;
        crate::shims::reshim(self, &ts)?;
        crate::runtime_symlinks::rebuild(self)?;
        Ok(())
    }

    #[cfg(test)]
    pub fn reset() {
        Settings::reset();
        CONFIG.write().unwrap().take();
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
    match settings_path.exists() {
        false => {
            trace!("settings does not exist {:?}", settings_path);
            Ok(RtxToml::init(&settings_path))
        }
        true => RtxToml::from_file(&settings_path)
            .wrap_err_with(|| eyre!("Error parsing {}", display_path(&settings_path))),
    }
}

fn load_plugins(settings: &Settings) -> Result<PluginMap> {
    let mut tools = CORE_PLUGINS.clone();
    if settings.experimental {
        tools.extend(EXPERIMENTAL_CORE_PLUGINS.clone());
    }
    let external = ExternalPlugin::list()?
        .into_iter()
        .map(|e| (e.name().into(), e))
        .collect::<Vec<(PluginName, Arc<dyn Plugin>)>>();
    tools.extend(external);
    for tool in &settings.disable_tools {
        tools.remove(tool);
    }
    Ok(tools)
}

fn load_legacy_files(settings: &Settings, tools: &PluginMap) -> BTreeMap<String, Vec<PluginName>> {
    if !settings.legacy_version_file {
        return BTreeMap::new();
    }
    let legacy = tools
        .values()
        .collect_vec()
        .into_par_iter()
        .filter(|tool| {
            !settings
                .legacy_version_file_disable_tools
                .contains(tool.name())
        })
        .filter_map(|tool| match tool.legacy_filenames() {
            Ok(filenames) => Some(
                filenames
                    .iter()
                    .map(|f| (f.to_string(), tool.name().to_string()))
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
        .collect::<Vec<(String, PluginName)>>();

    let mut legacy_filenames = BTreeMap::new();
    for (filename, plugin) in legacy {
        legacy_filenames
            .entry(filename)
            .or_insert_with(Vec::new)
            .push(plugin);
    }
    legacy_filenames
}

fn load_config_filenames(legacy_filenames: &BTreeMap<String, Vec<PluginName>>) -> Vec<PathBuf> {
    let mut filenames = legacy_filenames.keys().cloned().collect_vec();
    filenames.push(env::RTX_DEFAULT_TOOL_VERSIONS_FILENAME.clone());
    filenames.push(env::RTX_DEFAULT_CONFIG_FILENAME.clone());
    if *env::RTX_DEFAULT_CONFIG_FILENAME == ".rtx.toml" {
        filenames.push(".rtx.local.toml".to_string());
        if let Some(env) = &*env::RTX_ENV {
            filenames.push(format!(".rtx.{}.toml", env));
            filenames.push(format!(".rtx.{}.local.toml", env));
        }
    }

    let mut config_files = file::FindUp::new(&dirs::CURRENT, &filenames).collect::<Vec<_>>();

    for cf in global_config_files() {
        config_files.push(cf);
    }

    config_files.into_iter().unique().collect()
}

fn get_global_rtx_toml() -> PathBuf {
    env::RTX_CONFIG_FILE
        .clone()
        .unwrap_or_else(|| dirs::CONFIG.join("config.toml"))
}

pub fn global_config_files() -> Vec<PathBuf> {
    let mut config_files = vec![];
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
    config_files
}

fn load_all_config_files(
    config_filenames: &[PathBuf],
    tools: &PluginMap,
    legacy_filenames: &BTreeMap<String, Vec<PluginName>>,
    mut existing: ConfigMap,
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
            None => {
                let cf = parse_config_file(&f, legacy_filenames, tools)
                    .wrap_err_with(|| format!("error parsing config file: {}", display_path(&f)))?;
                Ok((f, cf))
            }
        })
        .collect::<Vec<Result<_>>>()
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect())
}

fn parse_config_file(
    f: &PathBuf,
    legacy_filenames: &BTreeMap<String, Vec<PluginName>>,
    tools: &ToolMap,
) -> Result<Box<dyn ConfigFile>> {
    match legacy_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => {
            let tools = tools
                .iter()
                .filter(|(k, _)| plugin.contains(k))
                .map(|(_, t)| t)
                .collect::<Vec<_>>();
            LegacyVersionFile::parse(f.into(), &tools).map(|f| Box::new(f) as Box<dyn ConfigFile>)
        }
        None => config_file::parse(f),
    }
}

fn load_env(config_files: &ConfigMap) -> (BTreeMap<String, String>, HashMap<String, PathBuf>) {
    let mut env = BTreeMap::new();
    let mut env_sources = HashMap::new();
    for (source, cf) in config_files.iter().rev() {
        env.extend(cf.env());
        for k in cf.env().keys() {
            env_sources.insert(k.clone(), source.clone());
        }
        for k in cf.env_remove() {
            // remove values set to "false"
            env.remove(&k);
        }
    }
    (env, env_sources)
}

fn load_path_dirs(config_files: &ConfigMap) -> Vec<PathBuf> {
    let mut path_dirs = vec![];
    for cf in config_files.values().rev() {
        path_dirs.extend(cf.path_dirs());
    }
    path_dirs
}

fn load_aliases(config_files: &ConfigMap) -> AliasMap {
    let mut aliases: AliasMap = AliasMap::new();

    for config_file in config_files.values() {
        for (plugin, plugin_aliases) in config_file.aliases() {
            for (from, to) in plugin_aliases {
                aliases.entry(plugin.clone()).or_default().insert(from, to);
            }
        }
    }

    aliases
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
            .list_plugins()
            .into_iter()
            .filter(|t| matches!(t.get_type(), PluginType::External))
            .map(|t| t.name().to_string())
            .collect::<Vec<_>>();
        let config_files = self
            .config_files
            .iter()
            .map(|(p, _)| display_path(p))
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
