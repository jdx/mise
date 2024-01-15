use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use either::Either;
use eyre::{Context, Result};
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::{Lazy, OnceCell};
use rayon::prelude::*;

pub use settings::Settings;

use crate::cli::args::ForgeArg;
use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::config::tracking::Tracker;
use crate::file::display_path;
use crate::forge::Forge;
use crate::shorthands::{get_shorthands, Shorthands};
use crate::task::Task;
use crate::ui::style;
use crate::{dirs, env, file, forge};

pub mod config_file;
pub mod settings;
mod tracking;

type AliasMap = BTreeMap<ForgeArg, BTreeMap<String, String>>;
type ConfigMap = IndexMap<PathBuf, Box<dyn ConfigFile>>;

#[derive(Default)]
pub struct Config {
    pub aliases: AliasMap,
    pub config_files: ConfigMap,
    pub env: BTreeMap<String, String>,
    pub env_sources: HashMap<String, PathBuf>,
    pub path_dirs: Vec<PathBuf>,
    pub project_root: Option<PathBuf>,
    all_aliases: OnceCell<AliasMap>,
    repo_urls: HashMap<String, String>,
    shorthands: OnceCell<HashMap<String, String>>,
    tasks: OnceCell<HashMap<String, Task>>,
    tasks_with_aliases: OnceCell<HashMap<String, Task>>,
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
        let settings = Settings::try_get()?;
        trace!("Settings: {:#?}", settings);

        let legacy_files = load_legacy_files(&settings);
        let config_filenames = legacy_files
            .keys()
            .chain(DEFAULT_CONFIG_FILENAMES.iter())
            .cloned()
            .collect_vec();
        let config_paths = load_config_paths(&config_filenames);
        let config_files = load_all_config_files(&config_paths, &legacy_files)?;

        let (env, env_sources) = load_env(&settings, &config_files);
        let repo_urls = config_files.values().flat_map(|cf| cf.plugins()).collect();

        let config = Self {
            env,
            env_sources,
            path_dirs: load_path_dirs(&config_files),
            aliases: load_aliases(&config_files),
            all_aliases: OnceCell::new(),
            shorthands: OnceCell::new(),
            tasks: OnceCell::new(),
            tasks_with_aliases: OnceCell::new(),
            project_root: get_project_root(&config_files),
            config_files,
            repo_urls,
        };

        debug!("{config:#?}");

        Ok(config)
    }
    pub fn get_shorthands(&self) -> &Shorthands {
        self.shorthands
            .get_or_init(|| get_shorthands(&Settings::get()))
    }

    pub fn get_repo_url(&self, plugin_name: &String) -> Option<String> {
        match self.repo_urls.get(plugin_name) {
            Some(url) => Some(url),
            None => self.get_shorthands().get(plugin_name),
        }
        .cloned()
    }

    pub fn get_all_aliases(&self) -> &AliasMap {
        self.all_aliases.get_or_init(|| self.load_all_aliases())
    }

    pub fn tasks(&self) -> &HashMap<String, Task> {
        self.tasks.get_or_init(|| {
            self.load_all_tasks()
                .into_iter()
                .filter(|(n, t)| *n == t.name)
                .collect()
        })
    }

    pub fn tasks_with_aliases(&self) -> &HashMap<String, Task> {
        self.tasks_with_aliases
            .get_or_init(|| self.load_all_tasks())
    }

    pub fn is_activated(&self) -> bool {
        env::var("__MISE_DIFF").is_ok()
    }

    pub fn resolve_alias(&self, forge: &dyn Forge, v: &str) -> Result<String> {
        if let Some(plugin_aliases) = self.aliases.get(forge.fa()) {
            if let Some(alias) = plugin_aliases.get(v) {
                return Ok(alias.clone());
            }
        }
        if let Some(alias) = forge.get_aliases()?.get(v) {
            return Ok(alias.clone());
        }
        Ok(v.to_string())
    }

    fn load_all_aliases(&self) -> AliasMap {
        let mut aliases: AliasMap = self.aliases.clone();
        let plugin_aliases: Vec<_> = forge::list()
            .into_par_iter()
            .map(|forge| {
                let aliases = forge.get_aliases().unwrap_or_else(|err| {
                    warn!("get_aliases: {err}");
                    BTreeMap::new()
                });
                (forge.fa().clone(), aliases)
            })
            .collect();
        for (fa, plugin_aliases) in plugin_aliases {
            for (from, to) in plugin_aliases {
                aliases.entry(fa.clone()).or_default().insert(from, to);
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

    pub fn load_all_tasks(&self) -> HashMap<String, Task> {
        self.config_files
            .values()
            .collect_vec()
            .into_par_iter()
            .flat_map(|cf| {
                match cf.project_root() {
                    Some(pr) => vec![
                        pr.join(".mise").join("tasks"),
                        pr.join(".config").join("mise").join("tasks"),
                    ],
                    None => vec![],
                }
                .into_par_iter()
                .flat_map(|dir| file::ls(&dir).map_err(|err| warn!("load_all_tasks: {err}")))
                .flatten()
                .map(Either::Right)
                .chain(rayon::iter::once(Either::Left(cf)))
            })
            .collect::<Vec<Either<&Box<dyn ConfigFile>, PathBuf>>>()
            .into_iter()
            .rev()
            .unique()
            .collect_vec()
            .into_par_iter()
            .flat_map(|either| match either {
                Either::Left(cf) => cf.tasks().into_iter().cloned().collect(),
                Either::Right(path) => match Task::from_path(path) {
                    Ok(task) => vec![task],
                    Err(err) => {
                        warn!("Error loading task: {:#}", err);
                        vec![]
                    }
                },
            })
            .flat_map(|t| {
                t.aliases
                    .iter()
                    .map(|a| (a.to_string(), t.clone()))
                    .chain(once((t.name.clone(), t.clone())))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        let config_files = Tracker::list_all()?
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
        crate::shims::reshim(&ts)?;
        crate::runtime_symlinks::rebuild(self)?;
        Ok(())
    }

    pub fn global_config(&self) -> Result<MiseToml> {
        let settings_path = env::MISE_GLOBAL_CONFIG_FILE.to_path_buf();
        match settings_path.exists() {
            false => {
                trace!("settings does not exist {:?}", settings_path);
                Ok(MiseToml::init(&settings_path))
            }
            true => MiseToml::from_file(&settings_path)
                .wrap_err_with(|| eyre!("Error parsing {}", display_path(&settings_path))),
        }
    }

    #[cfg(test)]
    pub fn reset() {
        Settings::reset(None);
        CONFIG.write().unwrap().take();
    }
}

fn get_project_root(config_files: &ConfigMap) -> Option<PathBuf> {
    config_files
        .values()
        .find_map(|cf| cf.project_root())
        .map(|pr| pr.to_path_buf())
}

fn load_legacy_files(settings: &Settings) -> BTreeMap<String, Vec<String>> {
    if !settings.legacy_version_file {
        return BTreeMap::new();
    }
    let legacy = forge::list()
        .into_par_iter()
        .filter(|tool| {
            !settings
                .legacy_version_file_disable_tools
                .contains(tool.id())
        })
        .filter_map(|tool| match tool.legacy_filenames() {
            Ok(filenames) => Some(
                filenames
                    .iter()
                    .map(|f| (f.to_string(), tool.id().to_string()))
                    .collect_vec(),
            ),
            Err(err) => {
                eprintln!("Error: {err}");
                None
            }
        })
        .collect::<Vec<Vec<(String, String)>>>()
        .into_iter()
        .flatten()
        .collect::<Vec<(String, String)>>();

    let mut legacy_filenames = BTreeMap::new();
    for (filename, plugin) in legacy {
        legacy_filenames
            .entry(filename)
            .or_insert_with(Vec::new)
            .push(plugin);
    }
    legacy_filenames
}

pub static DEFAULT_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    if *env::MISE_DEFAULT_CONFIG_FILENAME == ".mise.toml" {
        let mut filenames = vec![
            env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.clone(), // .tool-versions
            ".config/mise/config.toml".into(),
            ".config/mise.toml".into(),
            ".mise/config.toml".into(),
            ".rtx.toml".into(),
            env::MISE_DEFAULT_CONFIG_FILENAME.clone(), // .mise.toml
            ".config/mise/config.local.toml".into(),
            ".config/mise.local.toml".into(),
            ".mise/config.local.toml".into(),
            ".rtx.local.toml".into(),
            ".mise.local.toml".into(),
        ];
        if let Some(env) = &*env::MISE_ENV {
            filenames.push(format!(".config/mise/config.{env}.toml"));
            filenames.push(format!(".config/mise.{env}.toml"));
            filenames.push(format!(".mise/config.{env}.local.toml"));
            filenames.push(format!(".mise.{env}.toml"));
            filenames.push(format!(".config/mise/config.{env}.local.toml"));
            filenames.push(format!(".config/mise.{env}.local.toml"));
            filenames.push(format!(".mise/config.{env}.local.toml"));
            filenames.push(format!(".mise.{env}.local.toml"));
        }
        filenames
    } else {
        vec![
            env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.clone(),
            env::MISE_DEFAULT_CONFIG_FILENAME.clone(),
        ]
    }
});

pub fn load_config_paths(config_filenames: &[String]) -> Vec<PathBuf> {
    // In cases where the current dir is not available,
    // we simply don't load any configs.
    //
    // This can happen for any reason in a shell, the directory
    // being deleted or when inside fuse mounts.
    let Ok(current_dir) = env::current_dir() else {
        debug!("current dir not available");
        return Vec::new();
    };

    let mut config_files = file::FindUp::new(&current_dir, config_filenames).collect::<Vec<_>>();

    for cf in global_config_files() {
        config_files.push(cf);
    }
    for cf in system_config_files() {
        config_files.push(cf);
    }

    config_files.into_iter().unique().collect()
}

pub fn global_config_files() -> Vec<PathBuf> {
    let mut config_files = vec![];
    if env::var_path("MISE_CONFIG_FILE").is_none()
        && env::var_path("MISE_GLOBAL_CONFIG_FILE").is_none()
        && !*env::MISE_USE_TOML
    {
        // only add ~/.tool-versions if MISE_CONFIG_FILE is not set
        // because that's how the user overrides the default
        let home_config = dirs::HOME.join(env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
        if home_config.is_file() {
            config_files.push(home_config);
        }
    };
    let global_config = env::MISE_GLOBAL_CONFIG_FILE.clone();
    if global_config.is_file() {
        config_files.push(global_config);
    }
    config_files
}

pub fn system_config_files() -> Vec<PathBuf> {
    let mut config_files = vec![];
    let system = dirs::SYSTEM.join("config.toml");
    if system.is_file() {
        config_files.push(system);
    }
    config_files
}

fn load_all_config_files(
    config_filenames: &[PathBuf],
    legacy_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<ConfigMap> {
    Ok(config_filenames
        .iter()
        .unique()
        .collect_vec()
        .into_par_iter()
        .map(|f| {
            let cf = parse_config_file(f, legacy_filenames).wrap_err_with(|| {
                format!(
                    "error parsing config file: {}",
                    style::ebold(display_path(f))
                )
            })?;
            if let Err(err) = Tracker::track(f) {
                warn!("tracking config: {err:#}");
            }
            Ok((f.clone(), cf))
        })
        .collect::<Vec<Result<_>>>()
        .into_iter()
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect())
}

fn parse_config_file(
    f: &PathBuf,
    legacy_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<Box<dyn ConfigFile>> {
    match legacy_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => {
            let tools = forge::list()
                .into_iter()
                .filter(|f| plugin.contains(&f.to_string()))
                .collect::<Vec<_>>();
            LegacyVersionFile::parse(f.into(), tools).map(|f| Box::new(f) as Box<dyn ConfigFile>)
        }
        None => config_file::parse(f),
    }
}

fn load_env(
    settings: &Settings,
    config_files: &ConfigMap,
) -> (BTreeMap<String, String>, HashMap<String, PathBuf>) {
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
    if let Some(env_file) = &settings.env_file {
        match dotenvy::from_filename_iter(env_file) {
            Ok(iter) => {
                for item in iter {
                    let (k, v) = item.unwrap_or_else(|err| {
                        warn!("env_file: {err}");
                        Default::default()
                    });
                    env.insert(k.clone(), v);
                    env_sources.insert(k, env_file.clone());
                }
            }
            Err(err) => trace!("env_file: {err}"),
        }
    }
    (env, env_sources)
}

fn load_path_dirs(config_files: &ConfigMap) -> Vec<PathBuf> {
    let mut path_dirs = vec![];
    for cf in config_files.values().rev() {
        path_dirs.extend(cf.env_path());
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

impl Debug for Config {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let config_files = self
            .config_files
            .iter()
            .map(|(p, _)| display_path(p))
            .collect::<Vec<_>>();
        let mut s = f.debug_struct("Config");
        s.field("Config Files", &config_files);
        if let Some(tasks) = self.tasks.get() {
            s.field(
                "Tasks",
                &tasks.values().map(|t| t.to_string()).collect_vec(),
            );
        }
        if !self.env.is_empty() {
            s.field("Env", &self.env);
            // s.field("Env Sources", &self.env_sources);
        }
        if !self.path_dirs.is_empty() {
            s.field("Path Dirs", &self.path_dirs);
        }
        if !self.aliases.is_empty() {
            s.field("Aliases", &self.aliases);
        }
        s.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load() {
        let config = Config::load().unwrap();
        assert_debug_snapshot!(config);
    }
}
