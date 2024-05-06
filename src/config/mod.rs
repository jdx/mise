use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use eyre::{Context, Result};
use indexmap::IndexMap;
use itertools::Itertools;
use once_cell::sync::{Lazy, OnceCell};
use rayon::prelude::*;

pub use settings::Settings;

use crate::cli::args::ForgeArg;
use crate::cli::version;
use crate::config::config_file::legacy_version::LegacyVersionFile;
use crate::config::config_file::mise_toml::MiseToml;
use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvResults;
use crate::config::tracking::Tracker;
use crate::file::display_path;
use crate::forge::Forge;
use crate::shorthands::{get_shorthands, Shorthands};
use crate::task::Task;
use crate::ui::style;
use crate::{dirs, env, file, forge};

pub mod config_file;
mod env_directive;
pub mod settings;
pub mod tracking;

type AliasMap = BTreeMap<ForgeArg, BTreeMap<String, String>>;
type ConfigMap = IndexMap<PathBuf, Box<dyn ConfigFile>>;
type EnvWithSources = IndexMap<String, (String, PathBuf)>;

#[derive(Default)]
pub struct Config {
    pub aliases: AliasMap,
    pub config_files: ConfigMap,
    pub project_root: Option<PathBuf>,
    env: OnceCell<EnvResults>,
    env_with_sources: OnceCell<EnvWithSources>,
    all_aliases: OnceLock<AliasMap>,
    repo_urls: HashMap<String, String>,
    shorthands: OnceLock<HashMap<String, String>>,
    tasks: OnceCell<BTreeMap<String, Task>>,
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

        let repo_urls = config_files.values().flat_map(|cf| cf.plugins()).collect();

        let config = Self {
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            aliases: load_aliases(&config_files),
            all_aliases: OnceLock::new(),
            shorthands: OnceLock::new(),
            tasks: OnceCell::new(),
            project_root: get_project_root(&config_files),
            config_files,
            repo_urls,
        };

        config.validate()?;

        debug!("{config:#?}");

        Ok(config)
    }
    pub fn env(&self) -> eyre::Result<IndexMap<String, String>> {
        Ok(self
            .env_with_sources()?
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect())
    }
    pub fn env_with_sources(&self) -> eyre::Result<&EnvWithSources> {
        self.env_with_sources.get_or_try_init(|| {
            let mut env = self.env_results()?.env.clone();
            let settings = Settings::get();
            for env_file in settings.env_files() {
                match dotenvy::from_path_iter(&env_file) {
                    Ok(iter) => {
                        for item in iter {
                            let (k, v) = item.unwrap_or_else(|err| {
                                warn!("env_file: {err}");
                                Default::default()
                            });
                            env.insert(k, (v, env_file.clone()));
                        }
                    }
                    Err(err) => trace!("env_file: {err}"),
                }
            }
            Ok(env)
        })
    }
    pub fn env_results(&self) -> eyre::Result<&EnvResults> {
        self.env.get_or_try_init(|| self.load_env())
    }
    pub fn path_dirs(&self) -> eyre::Result<&Vec<PathBuf>> {
        Ok(&self.env_results()?.env_paths)
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

    pub fn tasks(&self) -> Result<&BTreeMap<String, Task>> {
        self.tasks.get_or_try_init(|| self.load_all_tasks())
    }

    pub fn tasks_with_aliases(&self) -> Result<BTreeMap<String, &Task>> {
        Ok(self
            .tasks()?
            .iter()
            .flat_map(|(_, t)| {
                t.aliases
                    .iter()
                    .map(|a| (a.to_string(), t))
                    .chain(once((t.name.clone(), t)))
                    .collect::<Vec<_>>()
            })
            .collect())
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

    pub fn load_all_tasks(&self) -> Result<BTreeMap<String, Task>> {
        Ok(file::all_dirs()?
            .into_iter()
            .filter(|d| {
                if cfg!(test) {
                    d.starts_with(*dirs::HOME)
                } else {
                    true
                }
            })
            .collect_vec()
            .into_par_iter()
            .map(|d| self.load_tasks_in_dir(&d))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .chain(self.load_global_tasks()?)
            .chain(self.load_system_tasks()?)
            .rev()
            .inspect(|t| trace!("loading task {t} from {}", display_path(&t.config_source)))
            .map(|t| (t.name.clone(), t))
            .collect())
    }

    fn load_tasks_in_dir(&self, dir: &Path) -> Result<Vec<Task>> {
        let configs = self.configs_at_root(dir);
        let config_tasks = configs.iter().flat_map(|cf| cf.tasks()).cloned();
        let includes = configs
            .iter()
            .find_map(|cf| cf.task_config().includes.clone())
            .unwrap_or_else(default_task_includes);
        let file_tasks = includes.into_iter().flat_map(|p| {
            let p = match p.is_absolute() {
                true => p,
                false => dir.join(p),
            };
            self.load_tasks_includes(&p).unwrap_or_else(|err| {
                warn!("loading tasks in {}: {err}", display_path(&p));
                vec![]
            })
        });
        Ok(file_tasks.into_iter().chain(config_tasks).collect())
    }

    fn load_global_tasks(&self) -> Result<Vec<Task>> {
        let cf = self.config_files.get(&*env::MISE_GLOBAL_CONFIG_FILE);
        Ok(self
            .load_config_tasks(&cf)
            .into_iter()
            .chain(self.load_file_tasks(&cf))
            .collect())
    }

    fn load_system_tasks(&self) -> Result<Vec<Task>> {
        let cf = self.config_files.get(&dirs::SYSTEM.join("config.toml"));
        Ok(self
            .load_config_tasks(&cf)
            .into_iter()
            .chain(self.load_file_tasks(&cf))
            .collect())
    }

    #[allow(clippy::borrowed_box)]
    fn load_config_tasks(&self, cf: &Option<&Box<dyn ConfigFile>>) -> Vec<Task> {
        cf.map(|cf| cf.tasks())
            .unwrap_or_default()
            .into_iter()
            .cloned()
            .collect()
    }

    #[allow(clippy::borrowed_box)]
    fn load_file_tasks(&self, cf: &Option<&Box<dyn ConfigFile>>) -> Vec<Task> {
        let includes = match cf {
            Some(cf) => cf
                .task_config()
                .includes
                .clone()
                .unwrap_or(vec!["tasks".into()])
                .into_iter()
                .map(|p| cf.get_path().parent().unwrap().join(p))
                .collect(),
            None => vec![dirs::CONFIG.join("tasks")],
        };
        includes
            .into_iter()
            .flat_map(|p| {
                self.load_tasks_includes(&p).unwrap_or_else(|err| {
                    warn!("loading tasks in {}: {err}", display_path(&p));
                    vec![]
                })
            })
            .collect()
    }

    fn load_tasks_includes(&self, root: &Path) -> Result<Vec<Task>> {
        file::recursive_ls(root)?
            .into_iter()
            .filter(|p| file::is_executable(p))
            .map(|path| Task::from_path(&path))
            .collect()
    }

    fn configs_at_root(&self, dir: &Path) -> Vec<&dyn ConfigFile> {
        DEFAULT_CONFIG_FILENAMES
            .iter()
            .map(|f| dir.join(f))
            .filter_map(|f| self.config_files.get(&f).map(|cf| cf.as_ref()))
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

    fn validate(&self) -> Result<()> {
        for cf in self.config_files.values() {
            if let Some(min) = cf.min_version() {
                let cur = &*version::V;
                ensure!(
                    cur >= min,
                    "mise version {} is required, but you are using {}",
                    style::eyellow(min),
                    style::eyellow(cur)
                );
            }
        }
        Ok(())
    }

    fn load_env(&self) -> Result<EnvResults> {
        let entries = self
            .config_files
            .iter()
            .rev()
            .flat_map(|(source, cf)| cf.env_entries().into_iter().map(|e| (e, source.clone())))
            .collect();
        EnvResults::resolve(&env::PRISTINE_ENV, entries)
    }

    pub fn watch_files(&self) -> eyre::Result<BTreeSet<PathBuf>> {
        let env_results = self.env_results()?;
        Ok(self
            .config_files
            .keys()
            .map(|p| p.to_path_buf())
            .chain(env_results.env_files.clone())
            .chain(env_results.env_scripts.clone())
            .chain(Settings::get().env_files())
            .collect())
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
    let mut config_files = Vec::new();

    // The current directory is not always available, e.g.
    // when a directory was deleted or inside FUSE mounts.
    match &*dirs::CWD {
        Some(current_dir) => {
            config_files.extend(file::FindUp::new(current_dir, config_filenames));
        }
        None => {}
    };

    config_files.extend(global_config_files());
    config_files.extend(system_config_files());

    config_files.into_iter().unique().collect()
}

pub fn is_global_config(path: &Path) -> bool {
    global_config_files()
        .iter()
        .chain(system_config_files().iter())
        .any(|p| p == path)
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
        if let Ok(env) = self.env() {
            if !env.is_empty() {
                s.field("Env", &env);
                // s.field("Env Sources", &self.env_sources);
            }
        }
        let path_dirs = self.path_dirs().cloned().unwrap_or_default();
        if !path_dirs.is_empty() {
            s.field("Path Dirs", &path_dirs);
        }
        if !self.aliases.is_empty() {
            s.field("Aliases", &self.aliases);
        }
        s.finish()
    }
}

fn default_task_includes() -> Vec<PathBuf> {
    vec![".mise/tasks".into(), ".config/mise/tasks".into()]
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
