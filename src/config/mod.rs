use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use eyre::{ensure, eyre, Context, Result};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
use once_cell::sync::{Lazy, OnceCell};
use rayon::prelude::*;
pub use settings::Settings;
use walkdir::WalkDir;

use crate::backend::ABackend;
use crate::cli::version;
use crate::config::config_file::idiomatic_version::IdiomaticVersionFile;
use crate::config::config_file::mise_toml::{MiseToml, Tasks};
use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvResults;
use crate::config::tracking::Tracker;
use crate::file::display_path;
use crate::shorthands::{get_shorthands, Shorthands};
use crate::task::Task;
use crate::toolset::{
    install_state, ToolRequestSet, ToolRequestSetBuilder, ToolVersion, Toolset, ToolsetBuilder,
};
use crate::ui::style;
use crate::{backend, dirs, env, file, lockfile, registry, runtime_symlinks, shims};

pub mod config_file;
pub mod env_directive;
pub mod settings;
pub mod tracking;

use crate::hook_env::WatchFilePattern;
use crate::hooks::Hook;
use crate::plugins::PluginType;
use crate::watch_files::WatchFile;
pub use settings::SETTINGS;

type AliasMap = IndexMap<String, Alias>;
type ConfigMap = IndexMap<PathBuf, Box<dyn ConfigFile>>;
type EnvWithSources = IndexMap<String, (String, PathBuf)>;

#[derive(Default)]
pub struct Config {
    pub config_files: ConfigMap,
    pub project_root: Option<PathBuf>,
    pub all_aliases: AliasMap,
    pub repo_urls: HashMap<String, String>,
    pub vars: IndexMap<String, String>,
    aliases: AliasMap,
    env: OnceCell<EnvResults>,
    env_with_sources: OnceCell<EnvWithSources>,
    shorthands: OnceLock<Shorthands>,
    tasks: OnceCell<BTreeMap<String, Task>>,
    tool_request_set: OnceCell<ToolRequestSet>,
    toolset: OnceCell<Toolset>,
}

#[derive(Debug, Clone, Default)]
pub struct Alias {
    pub backend: Option<String>,
    pub versions: IndexMap<String, String>,
}

static _CONFIG: RwLock<Option<Arc<Config>>> = RwLock::new(None);

pub fn is_loaded() -> bool {
    _CONFIG.read().unwrap().is_some()
}

impl Config {
    pub fn get() -> Arc<Self> {
        Self::try_get().unwrap()
    }
    pub fn try_get() -> Result<Arc<Self>> {
        if let Some(config) = &*_CONFIG.read().unwrap() {
            return Ok(config.clone());
        }
        let config = Arc::new(Self::load()?);
        *_CONFIG.write().unwrap() = Some(config.clone());
        Ok(config)
    }
    pub fn load() -> Result<Self> {
        reset();
        time!("load start");
        let idiomatic_files = load_idiomatic_files();
        time!("load idiomatic_files");
        let config_filenames = idiomatic_files
            .keys()
            .chain(DEFAULT_CONFIG_FILENAMES.iter())
            .cloned()
            .collect_vec();
        time!("load config_filenames");
        let config_paths = load_config_paths(&config_filenames, false);
        time!("load config_paths");
        trace!("config_paths: {config_paths:?}");
        let config_files = load_all_config_files(&config_paths, &idiomatic_files)?;
        time!("load config_files");

        let mut config = Self {
            aliases: load_aliases(&config_files)?,
            project_root: get_project_root(&config_files),
            repo_urls: load_plugins(&config_files)?,
            vars: load_vars(&config_files)?,
            config_files,
            ..Default::default()
        };
        time!("load build");

        config.validate()?;
        time!("load validate");

        config.all_aliases = config.load_all_aliases();
        time!("load all aliases");

        if log::log_enabled!(log::Level::Trace) {
            trace!("config: {config:#?}");
        } else if log::log_enabled!(log::Level::Debug) {
            for p in config.config_files.keys() {
                debug!("config: {}", display_path(p));
            }
        }
        time!("load done");

        for (plugin, url) in &config.repo_urls {
            let plugin_type = match url.contains("vfox-") {
                true => PluginType::Vfox,
                false => PluginType::Asdf,
            };
            install_state::add_plugin(plugin, plugin_type)?;
        }

        for short in config
            .all_aliases
            .iter()
            .filter(|(_, a)| a.backend.is_some())
            .map(|(s, _)| s)
            .chain(config.repo_urls.keys())
        {
            // we need to remove aliased tools so they get re-added with updated "full" values
            backend::remove(short);
        }

        Ok(config)
    }
    pub fn env_maybe(&self) -> Option<IndexMap<String, String>> {
        self.env_with_sources.get().map(|env| {
            env.iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect()
        })
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
            for env_file in SETTINGS.env_files() {
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
        self.shorthands.get_or_init(|| get_shorthands(&SETTINGS))
    }
    pub fn get_tool_request_set(&self) -> eyre::Result<&ToolRequestSet> {
        self.tool_request_set
            .get_or_try_init(|| ToolRequestSetBuilder::new().build())
    }

    pub fn get_toolset(&self) -> eyre::Result<&Toolset> {
        self.toolset.get_or_try_init(|| {
            let mut ts = Toolset::from(self.get_tool_request_set()?.clone());
            ts.resolve()?;
            Ok(ts)
        })
    }

    pub fn get_repo_url(&self, plugin_name: &str) -> Option<String> {
        let plugin_name = self
            .all_aliases
            .get(plugin_name)
            .and_then(|a| a.backend.clone())
            .or_else(|| self.repo_urls.get(plugin_name).cloned())
            .unwrap_or(plugin_name.to_string());
        let plugin_name = plugin_name.strip_prefix("asdf:").unwrap_or(&plugin_name);
        let plugin_name = plugin_name.strip_prefix("vfox:").unwrap_or(plugin_name);
        self.get_shorthands()
            .get(plugin_name)
            .map(|full| registry::full_to_url(&full[0]))
            .or_else(|| {
                if plugin_name.starts_with("https://") || plugin_name.split('/').count() == 2 {
                    Some(registry::full_to_url(plugin_name))
                } else {
                    None
                }
            })
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

    pub fn resolve_alias(&self, backend: &ABackend, v: &str) -> Result<String> {
        if let Some(plugin_aliases) = self.all_aliases.get(&backend.ba().short) {
            if let Some(alias) = plugin_aliases.versions.get(v) {
                return Ok(alias.clone());
            }
        }
        if let Some(alias) = backend.get_aliases()?.get(v) {
            return Ok(alias.clone());
        }
        Ok(v.to_string())
    }

    fn load_all_aliases(&self) -> AliasMap {
        let mut aliases: AliasMap = self.aliases.clone();
        let plugin_aliases: Vec<_> = backend::list()
            .into_par_iter()
            .map(|backend| {
                let aliases = backend.get_aliases().unwrap_or_else(|err| {
                    warn!("get_aliases: {err}");
                    BTreeMap::new()
                });
                (backend.ba().clone(), aliases)
            })
            .collect();
        for (ba, plugin_aliases) in plugin_aliases {
            for (from, to) in plugin_aliases {
                aliases
                    .entry(ba.short.to_string())
                    .or_default()
                    .versions
                    .insert(from, to);
            }
        }

        for (short, plugin_aliases) in &self.aliases {
            let alias = aliases.entry(short.clone()).or_default();
            if let Some(full) = &plugin_aliases.backend {
                alias.backend = Some(full.clone());
            }
            for (from, to) in &plugin_aliases.versions {
                alias.versions.insert(from.clone(), to.clone());
            }
        }

        aliases
    }

    fn load_all_tasks(&self) -> Result<BTreeMap<String, Task>> {
        time!("load_all_tasks");
        let mut file_tasks = None;
        let mut global_tasks = None;
        let mut system_tasks = None;
        rayon::scope(|s| {
            s.spawn(|_| {
                file_tasks = Some(self.load_file_tasks_recursively());
            });
            global_tasks = Some(self.load_global_tasks());
            system_tasks = Some(self.load_system_tasks());
        });
        let tasks: BTreeMap<String, Task> = file_tasks
            .unwrap()?
            .into_iter()
            .chain(global_tasks.unwrap()?)
            .chain(system_tasks.unwrap()?)
            .rev()
            .inspect(|t| {
                trace!(
                    "loaded task {} – {}",
                    &t.name,
                    display_path(&t.config_source)
                )
            })
            .map(|t| (t.name.clone(), t))
            .collect();
        time!("load_all_tasks {count}", count = tasks.len(),);
        Ok(tasks)
    }

    pub fn task_includes_for_dir(&self, dir: &Path) -> Vec<PathBuf> {
        self.configs_at_root(dir)
            .iter()
            .rev()
            .find_map(|cf| cf.task_config().includes.clone())
            .unwrap_or_else(default_task_includes)
            .into_par_iter()
            .map(|p| if p.is_absolute() { p } else { dir.join(p) })
            .filter(|p| p.exists())
            .collect::<Vec<_>>()
            .into_iter()
            .unique()
            .collect::<Vec<_>>()
    }

    pub fn load_tasks_in_dir(&self, dir: &Path) -> Result<Vec<Task>> {
        let configs = self.configs_at_root(dir);
        let config_tasks = configs
            .par_iter()
            .flat_map(|cf| cf.tasks())
            .cloned()
            .collect::<Vec<_>>();
        let includes = self.task_includes_for_dir(dir);
        let extra_tasks = includes
            .par_iter()
            .filter(|p| {
                p.is_file() && p.extension().unwrap_or_default().to_string_lossy() == "toml"
            })
            .map(|p| {
                self.load_task_file(p)
                    .wrap_err_with(|| format!("loading tasks in {}", display_path(p)))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let file_tasks = includes
            .into_par_iter()
            .flat_map(|p| {
                self.load_tasks_includes(&p, dir).unwrap_or_else(|err| {
                    warn!("loading tasks in {}: {err}", display_path(&p));
                    vec![]
                })
            })
            .collect::<Vec<_>>();
        Ok(file_tasks
            .into_iter()
            .chain(config_tasks)
            .chain(extra_tasks)
            .sorted_by_cached_key(|t| t.name.clone())
            .collect())
    }

    fn load_task_file(&self, path: &Path) -> Result<Vec<Task>> {
        let raw = file::read_to_string(path)?;
        let mut tasks = toml::from_str::<Tasks>(&raw)
            .wrap_err_with(|| format!("Error parsing task file: {}", display_path(path)))?
            .0;
        for (name, task) in &mut tasks {
            task.name = name.clone();
            task.config_source = path.to_path_buf();
        }
        Ok(tasks.into_values().collect())
    }

    fn load_file_tasks_recursively(&self) -> Result<Vec<Task>> {
        let file_tasks = file::all_dirs()?
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
            .collect_vec();
        Ok(file_tasks)
    }

    fn load_global_tasks(&self) -> Result<Vec<Task>> {
        let cf = self.config_files.get(&*env::MISE_GLOBAL_CONFIG_FILE);
        let config_root = cf.and_then(|cf| cf.project_root()).unwrap_or(&*env::HOME);
        Ok(self
            .load_config_tasks(&cf)
            .into_iter()
            .chain(self.load_file_tasks(&cf, config_root))
            .collect())
    }

    fn load_system_tasks(&self) -> Result<Vec<Task>> {
        let cf = self.config_files.get(&*env::MISE_SYSTEM_CONFIG_FILE);
        let config_root = cf
            .and_then(|cf| cf.project_root())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        Ok(self
            .load_config_tasks(&cf)
            .into_iter()
            .chain(self.load_file_tasks(&cf, &config_root))
            .collect())
    }

    fn load_config_tasks(&self, cf: &Option<&Box<dyn ConfigFile>>) -> Vec<Task> {
        cf.map(|cf| cf.tasks())
            .unwrap_or_default()
            .into_iter()
            .cloned()
            .collect()
    }

    fn load_file_tasks(&self, cf: &Option<&Box<dyn ConfigFile>>, config_root: &Path) -> Vec<Task> {
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
                self.load_tasks_includes(&p, config_root)
                    .unwrap_or_else(|err| {
                        warn!("loading tasks in {}: {err}", display_path(&p));
                        vec![]
                    })
            })
            .collect()
    }

    fn load_tasks_includes(&self, root: &Path, config_root: &Path) -> Result<Vec<Task>> {
        if !root.is_dir() {
            return Ok(vec![]);
        }
        WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            // skip hidden directories (if the root is hidden that's ok)
            .filter_entry(|e| e.path() == root || !e.file_name().to_string_lossy().starts_with('.'))
            .filter_ok(|e| e.file_type().is_file())
            .map_ok(|e| e.path().to_path_buf())
            .try_collect::<_, Vec<PathBuf>, _>()?
            .into_par_iter()
            .filter(|p| file::is_executable(p))
            .filter(|p| !SETTINGS.task_disable_paths.iter().any(|d| p.starts_with(d)))
            .map(|path| Task::from_path(&path, root, config_root))
            .collect()
    }

    fn configs_at_root(&self, dir: &Path) -> Vec<&dyn ConfigFile> {
        DEFAULT_CONFIG_FILENAMES
            .iter()
            .rev()
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

    fn validate(&self) -> eyre::Result<()> {
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

    fn load_env(&self) -> eyre::Result<EnvResults> {
        time!("load_env start");
        let entries = self
            .config_files
            .iter()
            .rev()
            .map(|(source, cf)| {
                cf.env_entries()
                    .map(|ee| ee.into_iter().map(|e| (e, source.clone())))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        // trace!("load_env: entries: {:#?}", entries);
        let env_results = EnvResults::resolve(&env::PRISTINE_ENV, entries)?;
        time!("load_env done");
        if log::log_enabled!(log::Level::Trace) {
            trace!("{env_results:#?}");
        } else {
            debug!("{env_results:?}");
        }
        Ok(env_results)
    }

    pub fn hooks(&self) -> Result<Vec<(PathBuf, Hook)>> {
        self.config_files
            .values()
            .map(|cf| Ok((cf.project_root(), cf.hooks()?)))
            .filter_map_ok(|(root, hooks)| root.map(|r| (r.to_path_buf(), hooks)))
            .map_ok(|(root, hooks)| {
                hooks
                    .into_iter()
                    .map(|h| (root.clone(), h))
                    .collect::<Vec<_>>()
            })
            .flatten_ok()
            .collect()
    }

    pub fn watch_file_hooks(&self) -> Result<IndexSet<(PathBuf, WatchFile)>> {
        Ok(self
            .config_files
            .values()
            .map(|cf| Ok((cf.project_root(), cf.watch_files()?)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(root, watch_files)| root.map(|r| (r.to_path_buf(), watch_files)))
            .flat_map(|(root, watch_files)| {
                watch_files
                    .iter()
                    .map(|wf| (root.clone(), wf.clone()))
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub fn watch_files(&self) -> Result<BTreeSet<WatchFilePattern>> {
        let env_results = self.env_results()?;
        Ok(self
            .config_files
            .iter()
            .map(|(p, cf)| {
                let mut watch_files: Vec<WatchFilePattern> = vec![p.as_path().into()];
                watch_files.extend(cf.watch_files()?.iter().map(|wf| WatchFilePattern {
                    root: cf.project_root().map(|pr| pr.to_path_buf()),
                    patterns: wf.patterns.clone(),
                }));
                Ok(watch_files)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .chain(env_results.env_files.iter().map(|p| p.as_path().into()))
            .chain(env_results.env_scripts.iter().map(|p| p.as_path().into()))
            .chain(SETTINGS.env_files().iter().map(|p| p.as_path().into()))
            .collect())
    }
}

fn get_project_root(config_files: &ConfigMap) -> Option<PathBuf> {
    let project_root = config_files
        .values()
        .find_map(|cf| cf.project_root())
        .map(|pr| pr.to_path_buf());
    trace!("project_root: {project_root:?}");
    project_root
}

fn load_idiomatic_files() -> BTreeMap<String, Vec<String>> {
    if !SETTINGS.idiomatic_version_file {
        return BTreeMap::new();
    }
    let idiomatic = backend::list()
        .into_par_iter()
        .filter(|tool| {
            !SETTINGS
                .idiomatic_version_file_disable_tools
                .contains(tool.id())
        })
        .filter_map(|tool| match tool.idiomatic_filenames() {
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

    let mut idiomatic_filenames = BTreeMap::new();
    for (filename, plugin) in idiomatic {
        idiomatic_filenames
            .entry(filename)
            .or_insert_with(Vec::new)
            .push(plugin);
    }
    idiomatic_filenames
}

static LOCAL_CONFIG_FILENAMES: Lazy<IndexSet<&'static str>> = Lazy::new(|| {
    let mut paths: IndexSet<&'static str> = IndexSet::new();
    if let Some(o) = &*env::MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES {
        paths.extend(o.iter().map(|s| s.as_str()));
    } else {
        paths.extend([
            ".tool-versions",
            &*env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME, // .tool-versions
        ]);
    }
    if !env::MISE_OVERRIDE_CONFIG_FILENAMES.is_empty() {
        paths.extend(
            env::MISE_OVERRIDE_CONFIG_FILENAMES
                .iter()
                .map(|s| s.as_str()),
        )
    } else {
        paths.extend([
            ".config/mise/conf.d/*.toml",
            ".config/mise/config.toml",
            ".config/mise.toml",
            ".mise/config.toml",
            "mise/config.toml",
            ".mise/config.toml",
            ".rtx.toml",
            "mise.toml",
            &*env::MISE_DEFAULT_CONFIG_FILENAME, // mise.toml
            ".mise.toml",
            ".config/mise/config.local.toml",
            ".config/mise.local.toml",
            ".mise/config.local.toml",
            ".rtx.local.toml",
            "mise.local.toml",
            ".mise.local.toml",
        ]);
    }

    paths
});
pub static DEFAULT_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    let mut filenames = LOCAL_CONFIG_FILENAMES
        .iter()
        .map(|f| f.to_string())
        .collect_vec();
    if let Some(env) = &*env::MISE_ENV {
        filenames.push(format!(".config/mise/config.{env}.toml"));
        filenames.push(format!(".config/mise.{env}.toml"));
        filenames.push(format!("mise/config.{env}.toml"));
        filenames.push(format!("mise.{env}.toml"));
        filenames.push(format!(".mise/config.{env}.toml"));
        filenames.push(format!(".mise.{env}.toml"));
        filenames.push(format!(".config/mise/config.{env}.local.toml"));
        filenames.push(format!(".config/mise.{env}.local.toml"));
        filenames.push(format!("mise/config.{env}.local.toml"));
        filenames.push(format!("mise.{env}.local.toml"));
        filenames.push(format!(".mise/config.{env}.local.toml"));
        filenames.push(format!(".mise.{env}.local.toml"));
    }
    filenames
});
static TOML_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .filter(|s| s.ends_with(".toml"))
        .map(|s| s.to_string())
        .collect()
});
pub static ALL_CONFIG_FILES: Lazy<IndexSet<PathBuf>> = Lazy::new(|| {
    load_config_paths(&DEFAULT_CONFIG_FILENAMES, false)
        .into_iter()
        .collect()
});
// pub static LOCAL_CONFIG_FILES: Lazy<Vec<PathBuf>> = Lazy::new(|| {
//     ALL_CONFIG_FILES
//         .iter()
//         .filter(|cf| !is_global_config(cf))
//         .cloned()
//         .collect()
// });

type GlobResults = HashMap<(PathBuf, String), Vec<PathBuf>>;
static GLOB_RESULTS: Lazy<Mutex<GlobResults>> = Lazy::new(Default::default);

pub fn glob(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut results = GLOB_RESULTS.lock().unwrap();
    let key = (dir.to_path_buf(), pattern.to_string());
    if let Some(glob) = results.get(&key) {
        return Ok(glob.clone());
    }
    let paths = glob::glob(dir.join(pattern).to_string_lossy().as_ref())?
        .filter_map(|p| p.ok())
        .collect_vec();
    results.insert(key, paths.clone());
    Ok(paths)
}

pub fn config_files_in_dir(dir: &Path) -> IndexSet<PathBuf> {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .flat_map(|f| glob(dir, f).unwrap_or_default())
        .collect()
}

pub fn load_config_paths(config_filenames: &[String], include_ignored: bool) -> Vec<PathBuf> {
    let dirs = file::all_dirs().unwrap_or_default();

    let mut config_files = dirs
        .iter()
        .flat_map(|dir| {
            config_filenames
                .iter()
                .rev()
                .flat_map(|f| glob(dir, f).unwrap_or_default().into_iter().rev())
        })
        .collect_vec();

    config_files.extend(global_config_files());
    config_files.extend(system_config_files());

    config_files
        .into_iter()
        .unique_by(|p| file::desymlink_path(p))
        .filter(|p| include_ignored || !config_file::is_ignored(p))
        .collect()
}

pub fn is_global_config(path: &Path) -> bool {
    global_config_files().contains(path) || system_config_files().contains(path)
}

static GLOBAL_CONFIG_FILES: Lazy<Mutex<Option<IndexSet<PathBuf>>>> = Lazy::new(Default::default);
static SYSTEM_CONFIG_FILES: Lazy<Mutex<Option<IndexSet<PathBuf>>>> = Lazy::new(Default::default);

pub fn global_config_files() -> IndexSet<PathBuf> {
    let mut g = GLOBAL_CONFIG_FILES.lock().unwrap();
    if let Some(g) = &*g {
        return g.clone();
    }
    let mut config_files = IndexSet::new();
    if env::var_path("MISE_CONFIG_FILE").is_none()
        && env::var_path("MISE_GLOBAL_CONFIG_FILE").is_none()
        && !*env::MISE_USE_TOML
    {
        // only add ~/.tool-versions if MISE_CONFIG_FILE is not set
        // because that's how the user overrides the default
        let home_config = dirs::HOME.join(env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str());
        if home_config.is_file() {
            config_files.insert(home_config);
        }
    };
    let global_config = env::MISE_GLOBAL_CONFIG_FILE.clone();
    let global_local_config = global_config.with_extension("local.toml");
    for f in [global_config, global_local_config] {
        if f.is_file() {
            config_files.insert(f);
        }
    }
    if let Some(env) = &*env::MISE_ENV {
        let global_profile_files = vec![
            dirs::CONFIG.join(format!("config.{env}.toml")),
            dirs::CONFIG.join(format!("config.{env}.local.toml")),
            dirs::CONFIG.join(format!("mise.{env}.toml")),
            dirs::CONFIG.join(format!("mise.{env}.local.toml")),
        ];
        for f in global_profile_files {
            if f.is_file() {
                config_files.insert(f);
            }
        }
    }
    *g = Some(config_files.clone());
    config_files
}

pub fn system_config_files() -> IndexSet<PathBuf> {
    let mut s = SYSTEM_CONFIG_FILES.lock().unwrap();
    if let Some(s) = &*s {
        return s.clone();
    }
    let mut config_files = IndexSet::new();
    if env::MISE_SYSTEM_CONFIG_FILE.is_file() {
        config_files.insert(env::MISE_SYSTEM_CONFIG_FILE.clone());
    }
    let system_local = env::MISE_SYSTEM_CONFIG_FILE.with_extension("local.toml");
    if system_local.is_file() {
        config_files.insert(system_local);
    }
    *s = Some(config_files.clone());
    config_files
}

/// the top-most global config file or the path to where it should be written to
pub fn global_config_path() -> PathBuf {
    global_config_files()
        .last()
        .cloned()
        .unwrap_or_else(|| env::MISE_GLOBAL_CONFIG_FILE.clone())
}

/// the top-most mise.toml (local or global)
pub fn top_toml_config() -> Option<PathBuf> {
    load_config_paths(&TOML_CONFIG_FILENAMES, false)
        .iter()
        .find(|p| p.to_string_lossy().ends_with(".toml"))
        .map(|p| p.to_path_buf())
}

pub static ALL_TOML_CONFIG_FILES: Lazy<IndexSet<PathBuf>> = Lazy::new(|| {
    load_config_paths(&TOML_CONFIG_FILENAMES, false)
        .into_iter()
        .collect()
});

/// list of all non-global mise.tomls
pub fn local_toml_config_paths() -> Vec<&'static PathBuf> {
    ALL_TOML_CONFIG_FILES
        .iter()
        .filter(|p| !is_global_config(p))
        .collect()
}

/// either the top local mise.toml or the path to where it should be written to
pub fn local_toml_config_path() -> PathBuf {
    static CWD: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("."));
    local_toml_config_paths()
        .into_iter()
        .next_back()
        .cloned()
        .unwrap_or_else(|| {
            dirs::CWD
                .as_ref()
                .unwrap_or(&CWD)
                .join(&*env::MISE_DEFAULT_CONFIG_FILENAME)
        })
}

fn load_all_config_files(
    config_filenames: &[PathBuf],
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<ConfigMap> {
    Ok(config_filenames
        .iter()
        .unique()
        .collect_vec()
        .into_par_iter()
        .map(|f| {
            let cf = parse_config_file(f, idiomatic_filenames).wrap_err_with(|| {
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
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .collect())
}

fn parse_config_file(
    f: &PathBuf,
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<Box<dyn ConfigFile>> {
    match idiomatic_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => {
            trace!("idiomatic version file: {}", display_path(f));
            let tools = backend::list()
                .into_iter()
                .filter(|f| plugin.contains(&f.to_string()))
                .collect::<Vec<_>>();
            IdiomaticVersionFile::parse(f.into(), tools).map(|f| Box::new(f) as Box<dyn ConfigFile>)
        }
        None => config_file::parse(f),
    }
}

fn load_aliases(config_files: &ConfigMap) -> Result<AliasMap> {
    let mut aliases: AliasMap = AliasMap::new();

    for config_file in config_files.values() {
        for (plugin, plugin_aliases) in config_file.aliases()? {
            let alias = aliases.entry(plugin.clone()).or_default();
            if let Some(full) = plugin_aliases.backend {
                alias.backend = Some(full);
            }
            for (from, to) in plugin_aliases.versions {
                alias.versions.insert(from, to);
            }
        }
    }
    trace!("load_aliases: {}", aliases.len());

    Ok(aliases)
}

fn load_plugins(config_files: &ConfigMap) -> Result<HashMap<String, String>> {
    let mut plugins = HashMap::new();
    for config_file in config_files.values() {
        for (plugin, url) in config_file.plugins()? {
            plugins.insert(plugin.clone(), url.clone());
        }
    }
    trace!("load_plugins: {}", plugins.len());
    Ok(plugins)
}

fn load_vars(config_files: &ConfigMap) -> Result<IndexMap<String, String>> {
    let mut vars = IndexMap::new();
    for config_file in config_files.values() {
        for (k, v) in config_file.vars()?.clone() {
            vars.insert(k, v);
        }
    }
    trace!("load_vars: {}", vars.len());
    Ok(vars)
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
        if let Some(env) = self.env_maybe() {
            if !env.is_empty() {
                s.field("Env", &env);
                // s.field("Env Sources", &self.env_sources);
            }
        }
        if let Some(env_results) = self.env.get() {
            if !env_results.env_files.is_empty() {
                s.field("Path Dirs", &env_results.env_paths);
            }
        }
        if !self.aliases.is_empty() {
            s.field("Aliases", &self.aliases);
        }
        s.finish()
    }
}

fn default_task_includes() -> Vec<PathBuf> {
    vec![
        PathBuf::from("mise-tasks"),
        PathBuf::from(".mise-tasks"),
        PathBuf::from(".mise").join("tasks"),
        PathBuf::from(".config").join("mise").join("tasks"),
        PathBuf::from("mise").join("tasks"),
    ]
}

pub fn rebuild_shims_and_runtime_symlinks(new_versions: &[ToolVersion]) -> Result<()> {
    let config = Config::load()?;
    let ts = ToolsetBuilder::new().build(&config)?;
    trace!("rebuilding shims");
    shims::reshim(&ts, false).wrap_err("failed to rebuild shims")?;
    trace!("rebuilding runtime symlinks");
    runtime_symlinks::rebuild(&config).wrap_err("failed to rebuild runtime symlinks")?;
    trace!("updating lockfiles");
    lockfile::update_lockfiles(&config, &ts, new_versions)
        .wrap_err("failed to update lockfiles")?;

    Ok(())
}

fn reset() {
    install_state::reset();
    backend::reset();
    Settings::reset(None);
    _CONFIG.write().unwrap().take();
    *GLOBAL_CONFIG_FILES.lock().unwrap() = None;
    *SYSTEM_CONFIG_FILES.lock().unwrap() = None;
    GLOB_RESULTS.lock().unwrap().clear()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use insta::assert_debug_snapshot;

    use super::*;

    #[test]
    fn test_load() {
        let config = Config::load().unwrap();
        assert_debug_snapshot!(config);
    }
}
