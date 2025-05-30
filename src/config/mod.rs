use config_file::ConfigFileType;
use eyre::{Context, Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
pub use settings::Settings;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::{sync::OnceCell, task::JoinSet};
use walkdir::WalkDir;

use crate::config::config_file::idiomatic_version::IdiomaticVersionFile;
use crate::config::config_file::mise_toml::{MiseToml, Tasks};
use crate::config::config_file::{ConfigFile, config_trust_root};
use crate::config::env_directive::{EnvResolveOptions, EnvResults};
use crate::config::tracking::Tracker;
use crate::env::{MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;
use crate::shorthands::{Shorthands, get_shorthands};
use crate::task::Task;
use crate::toolset::{ToolRequestSet, ToolRequestSetBuilder, ToolVersion, Toolset, install_state};
use crate::ui::style;
use crate::{backend, dirs, env, file, lockfile, registry, runtime_symlinks, shims, timeout};
use crate::{backend::ABackend, cli::version::VERSION};
use crate::{backend::Backend, cli::version};

pub mod config_file;
pub mod env_directive;
pub mod settings;
pub mod tracking;

use crate::cli::self_update::SelfUpdate;
use crate::env_diff::EnvMap;
use crate::hook_env::WatchFilePattern;
use crate::hooks::Hook;
use crate::plugins::PluginType;
use crate::tera::BASE_CONTEXT;
use crate::watch_files::WatchFile;
use crate::wildcard::Wildcard;

type AliasMap = IndexMap<String, Alias>;
type ConfigMap = IndexMap<PathBuf, Arc<dyn ConfigFile>>;
pub type EnvWithSources = IndexMap<String, (String, PathBuf)>;

pub struct Config {
    pub config_files: ConfigMap,
    pub project_root: Option<PathBuf>,
    pub all_aliases: AliasMap,
    pub repo_urls: HashMap<String, String>,
    pub vars: IndexMap<String, String>,
    pub tera_ctx: tera::Context,
    pub shorthands: Shorthands,
    aliases: AliasMap,
    env: OnceCell<EnvResults>,
    env_with_sources: OnceCell<EnvWithSources>,
    hooks: OnceCell<Vec<(PathBuf, Hook)>>,
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
static _REDACTIONS: Lazy<Mutex<Arc<IndexSet<String>>>> = Lazy::new(Default::default);

pub fn is_loaded() -> bool {
    _CONFIG.read().unwrap().is_some()
}

impl Config {
    pub async fn get() -> Result<Arc<Self>> {
        if let Some(config) = &*_CONFIG.read().unwrap() {
            return Ok(config.clone());
        }
        measure!("load config", { Self::load().await })
    }
    pub fn get_() -> Arc<Self> {
        (*_CONFIG.read().unwrap()).clone().unwrap()
    }
    pub async fn reset() -> Result<Arc<Self>> {
        backend::reset().await?;
        timeout::run_with_timeout_async(
            async || {
                _CONFIG.write().unwrap().take();
                *GLOBAL_CONFIG_FILES.lock().unwrap() = None;
                *SYSTEM_CONFIG_FILES.lock().unwrap() = None;
                GLOB_RESULTS.lock().unwrap().clear();
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await?;
        Config::load().await
    }

    #[async_backtrace::framed]
    pub async fn load() -> Result<Arc<Self>> {
        backend::load_tools().await?;
        let idiomatic_files = measure!("config::load idiomatic_files", {
            load_idiomatic_files().await
        });
        let config_filenames = idiomatic_files
            .keys()
            .chain(DEFAULT_CONFIG_FILENAMES.iter())
            .cloned()
            .collect_vec();
        let config_paths = measure!("config::load config_paths", {
            load_config_paths(&config_filenames, false)
        });
        trace!("config_paths: {config_paths:?}");
        let config_files = measure!("config::load config_files", {
            load_all_config_files(&config_paths, &idiomatic_files).await?
        });
        measure!("config::load warn_about_idiomatic_version_files", {
            warn_about_idiomatic_version_files(&config_files);
        });

        let mut config = Self {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files,
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: get_shorthands(&Settings::get()),
            hooks: OnceCell::new(),
            tasks: OnceCell::new(),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases: Default::default(),
            aliases: Default::default(),
            project_root: Default::default(),
            repo_urls: Default::default(),
            vars: Default::default(),
        };
        let vars_config = Arc::new(Self {
            tera_ctx: config.tera_ctx.clone(),
            config_files: config.config_files.clone(),
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: config.shorthands.clone(),
            hooks: OnceCell::new(),
            tasks: OnceCell::new(),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases: config.all_aliases.clone(),
            aliases: config.aliases.clone(),
            project_root: config.project_root.clone(),
            repo_urls: config.repo_urls.clone(),
            vars: config.vars.clone(),
        });
        let vars_results = measure!("config::load vars_results", {
            load_vars(&vars_config).await?
        });
        let vars: IndexMap<String, String> = vars_results
            .vars
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect();
        config.tera_ctx.insert("vars", &vars);

        config.vars = vars;
        config.aliases = load_aliases(&config.config_files)?;
        config.project_root = get_project_root(&config.config_files);
        config.repo_urls = load_plugins(&config.config_files)?;
        measure!("config::load validate", {
            config.validate()?;
        });

        config.all_aliases = measure!("config::load all_aliases", { config.load_all_aliases() });

        measure!("config::load redactions", {
            config.add_redactions(
                config.redaction_keys(),
                &config.vars.clone().into_iter().collect(),
            );
        });

        if log::log_enabled!(log::Level::Trace) {
            trace!("config: {config:#?}");
        } else if log::log_enabled!(log::Level::Debug) {
            for p in config.config_files.keys() {
                debug!("config: {}", display_path(p));
            }
        }

        time!("load done");

        measure!("config::load install_state", {
            for (plugin, url) in &config.repo_urls {
                let plugin_type = match url.contains("vfox-") {
                    true => PluginType::Vfox,
                    false => PluginType::Asdf,
                };
                install_state::add_plugin(plugin, plugin_type).await?;
            }
        });

        measure!("config::load remove_aliased_tools", {
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
        });

        let config = Arc::new(config);
        *_CONFIG.write().unwrap() = Some(config.clone());
        Ok(config)
    }
    pub fn env_maybe(&self) -> Option<IndexMap<String, String>> {
        self.env_with_sources.get().map(|env| {
            env.iter()
                .map(|(k, (v, _))| (k.clone(), v.clone()))
                .collect()
        })
    }
    pub async fn env(self: &Arc<Self>) -> eyre::Result<IndexMap<String, String>> {
        Ok(self
            .env_with_sources()
            .await?
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect())
    }
    pub async fn env_with_sources(self: &Arc<Self>) -> eyre::Result<&EnvWithSources> {
        self.env_with_sources
            .get_or_try_init(async || {
                let mut env = self.env_results().await?.env.clone();
                for env_file in Settings::get().env_files() {
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
            .await
    }
    pub async fn env_results(self: &Arc<Self>) -> Result<&EnvResults> {
        self.env
            .get_or_try_init(|| async { self.load_env().await })
            .await
    }
    pub async fn path_dirs(self: &Arc<Self>) -> eyre::Result<&Vec<PathBuf>> {
        Ok(&self.env_results().await?.env_paths)
    }
    pub async fn get_tool_request_set(&self) -> eyre::Result<&ToolRequestSet> {
        self.tool_request_set
            .get_or_try_init(async || ToolRequestSetBuilder::new().build(self).await)
            .await
    }

    pub async fn get_toolset(self: &Arc<Self>) -> Result<&Toolset> {
        self.toolset
            .get_or_try_init(|| async {
                let mut ts = Toolset::from(self.get_tool_request_set().await?.clone());
                ts.resolve(self).await?;
                Ok(ts)
            })
            .await
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
        self.shorthands
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

    pub async fn tasks(&self) -> Result<&BTreeMap<String, Task>> {
        self.tasks
            .get_or_try_init(|| async {
                measure!("config::load_all_tasks", { self.load_all_tasks().await })
            })
            .await
    }

    pub async fn tasks_with_aliases(&self) -> Result<BTreeMap<String, &Task>> {
        Ok(self
            .tasks()
            .await?
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

    pub async fn resolve_alias(&self, backend: &ABackend, v: &str) -> Result<String> {
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
            .into_iter()
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

    async fn load_all_tasks(&self) -> Result<BTreeMap<String, Task>> {
        let config = Config::get().await?;
        time!("load_all_tasks");
        // let (file_tasks, global_tasks, system_tasks) = tokio::join!(
        //     {
        //         let config = config.clone();
        //         tokio::task::spawn(async move { load_local_tasks(&config).await })
        //     },
        //     {
        //         let config = config.clone();
        //         tokio::task::spawn(async move { load_global_tasks(&config).await })
        //     },
        //     {
        //         let config = config.clone();
        //         tokio::task::spawn(async move { load_system_tasks(&config).await })
        //     },
        // );
        let file_tasks = load_local_tasks(&config).await?;
        let global_tasks = load_global_tasks(&config).await?;
        let system_tasks = load_system_tasks(&config).await?;
        let mut tasks: BTreeMap<String, Task> = file_tasks
            .into_iter()
            .chain(global_tasks)
            .chain(system_tasks)
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
        let all_tasks = tasks.clone();
        for task in tasks.values_mut() {
            task.display_name = task.display_name(&all_tasks);
        }
        time!("load_all_tasks {count}", count = tasks.len(),);
        Ok(tasks)
    }

    pub fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        let config_files = Tracker::list_all()?
            .into_iter()
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
        let settings_path = global_config_path();
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
                if cur < min {
                    let min = style::eyellow(min);
                    let cur = style::eyellow(cur);
                    if SelfUpdate::is_available() {
                        bail!(
                            "mise version {min} is required, but you are using {cur}\n\
                            Run `mise self-update` to update mise",
                        );
                    } else {
                        bail!("mise version {min} is required, but you are using {cur}");
                    }
                }
            }
        }
        Ok(())
    }

    async fn load_env(self: &Arc<Self>) -> Result<EnvResults> {
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
        let env_results = EnvResults::resolve(
            self,
            self.tera_ctx.clone(),
            &env::PRISTINE_ENV,
            entries,
            EnvResolveOptions::default(),
        )
        .await?;
        let redact_keys = self
            .redaction_keys()
            .into_iter()
            .chain(env_results.redactions.clone())
            .collect_vec();
        self.add_redactions(
            redact_keys,
            &env_results
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.0.clone()))
                .collect(),
        );
        if log::log_enabled!(log::Level::Trace) {
            trace!("{env_results:#?}");
        } else if !env_results.is_empty() {
            debug!("{env_results:?}");
        }
        Ok(env_results)
    }

    pub async fn hooks(&self) -> Result<&Vec<(PathBuf, Hook)>> {
        self.hooks
            .get_or_try_init(|| async {
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
            })
            .await
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

    pub async fn watch_files(self: &Arc<Self>) -> Result<BTreeSet<WatchFilePattern>> {
        let env_results = self.env_results().await?;
        Ok(self
            .config_files
            .iter()
            .map(|(p, cf)| {
                let mut watch_files: Vec<WatchFilePattern> = vec![p.as_path().into()];
                if let Some(parent) = p.parent() {
                    watch_files.push(parent.join("mise.lock").into());
                }
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
            .chain(
                Settings::get()
                    .env_files()
                    .iter()
                    .map(|p| p.as_path().into()),
            )
            .collect())
    }

    pub fn redaction_keys(&self) -> Vec<String> {
        self.config_files
            .values()
            .flat_map(|cf| cf.redactions().0.iter())
            .cloned()
            .collect()
    }
    pub fn add_redactions(&self, redactions: impl IntoIterator<Item = String>, env: &EnvMap) {
        let mut r = _REDACTIONS.lock().unwrap();
        let redactions = redactions.into_iter().flat_map(|r| {
            let matcher = Wildcard::new(vec![r]);
            env.iter()
                .filter(|(k, _)| matcher.match_any(k))
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>()
        });
        *r = Arc::new(r.iter().cloned().chain(redactions).collect());
    }

    pub fn redactions(&self) -> Arc<IndexSet<String>> {
        let r = _REDACTIONS.lock().unwrap();
        r.deref().clone()

        // self.redactions.get_or_try_init(|| {
        //     let mut redactions = Redactions::default();
        //     for cf in self.config_files.values() {
        //         let r = cf.redactions();
        //         if !r.is_empty() {
        //             let mut r = r.clone();
        //             let (tera, ctx) = self.tera(&cf.config_root());
        //             r.render(&mut tera.clone(), &ctx)?;
        //             redactions.merge(r);
        //         }
        //     }
        //     if redactions.is_empty() {
        //         return Ok(Default::default());
        //     }
        //
        //     let ts = self.get_toolset()?;
        //     let env = ts.full_env()?;
        //
        //     let env_matcher = Wildcard::new(redactions.env.clone());
        //     let var_matcher = Wildcard::new(redactions.vars.clone());
        //
        //     let env_vals = env
        //         .into_iter()
        //         .filter(|(k, _)| env_matcher.match_any(k))
        //         .map(|(_, v)| v);
        //     let var_vals = self
        //         .vars
        //         .iter()
        //         .filter(|(k, _)| var_matcher.match_any(k))
        //         .map(|(_, v)| v.to_string());
        //     Ok(env_vals.chain(var_vals).collect())
        // })
    }

    pub fn redact(&self, mut input: String) -> String {
        for redaction in self.redactions().deref() {
            input = input.replace(redaction, "[redacted]");
        }
        input
    }
}

fn configs_at_root<'a>(dir: &Path, config_files: &'a ConfigMap) -> Vec<&'a Arc<dyn ConfigFile>> {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .rev()
        .map(|f| dir.join(f))
        .filter_map(|f| config_files.get(&f))
        .collect()
}

fn get_project_root(config_files: &ConfigMap) -> Option<PathBuf> {
    let project_root = config_files
        .values()
        .find_map(|cf| cf.project_root())
        .map(|pr| pr.to_path_buf());
    trace!("project_root: {project_root:?}");
    project_root
}

async fn load_idiomatic_files() -> BTreeMap<String, Vec<String>> {
    if !Settings::get().idiomatic_version_file {
        return BTreeMap::new();
    }
    if !Settings::get()
        .idiomatic_version_file_disable_tools
        .is_empty()
    {
        deprecated!(
            "idiomatic_version_file_disable_tools",
            "is deprecated, use idiomatic_version_file_enable_tools instead"
        );
    }
    let mut jset = JoinSet::new();
    let tool_is_enabled = |tool: &dyn Backend| {
        if let Some(enable_tools) = &Settings::get().idiomatic_version_file_enable_tools {
            enable_tools.contains(tool.id())
        } else if !Settings::get()
            .idiomatic_version_file_disable_tools
            .is_empty()
        {
            !Settings::get()
                .idiomatic_version_file_disable_tools
                .contains(tool.id())
        } else {
            true
        }
    };
    for tool in backend::list() {
        jset.spawn(async move {
            if !tool_is_enabled(&*tool) {
                return vec![];
            }
            match tool.idiomatic_filenames() {
                Ok(filenames) => filenames
                    .iter()
                    .map(|f| (f.to_string(), tool.id().to_string()))
                    .collect::<Vec<_>>(),
                Err(err) => {
                    eprintln!("Error: {err}");
                    vec![]
                }
            }
        });
    }
    let idiomatic = jset
        .join_all()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

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
            ".config/mise/mise.toml",
            ".config/mise.toml",
            ".mise/config.toml",
            "mise/config.toml",
            ".rtx.toml",
            "mise.toml",
            &*env::MISE_DEFAULT_CONFIG_FILENAME, // mise.toml
            ".mise.toml",
            ".config/mise/config.local.toml",
            ".config/mise/mise.local.toml",
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
    for env in &*env::MISE_ENV {
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
pub static IGNORED_CONFIG_FILES: Lazy<IndexSet<PathBuf>> = Lazy::new(|| {
    load_config_paths(&DEFAULT_CONFIG_FILENAMES, true)
        .into_iter()
        .filter(|p| config_file::is_ignored(p))
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

pub fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    for dir in file::all_dirs().unwrap_or_default() {
        if let Some(cf) = self::config_files_in_dir(&dir).last() {
            if !is_global_config(cf) {
                return cf.clone();
            }
        }
    }
    match Settings::get().asdf_compat {
        true => p.join(&*MISE_DEFAULT_TOOL_VERSIONS_FILENAME),
        false => p.join(&*MISE_DEFAULT_CONFIG_FILENAME),
    }
}

pub fn load_config_paths(config_filenames: &[String], include_ignored: bool) -> Vec<PathBuf> {
    if Settings::no_config() {
        return vec![];
    }
    let dirs = file::all_dirs().unwrap_or_default();

    let mut config_files = dirs
        .iter()
        .flat_map(|dir| {
            if !include_ignored
                && env::MISE_IGNORED_CONFIG_PATHS
                    .iter()
                    .any(|p| dir.starts_with(p))
            {
                vec![]
            } else {
                config_filenames
                    .iter()
                    .rev()
                    .flat_map(|f| glob(dir, f).unwrap_or_default().into_iter().rev())
                    .collect()
            }
        })
        .collect::<Vec<_>>();

    config_files.extend(global_config_files());
    config_files.extend(system_config_files());

    config_files
        .into_iter()
        .unique_by(|p| file::desymlink_path(p))
        .filter(|p| {
            include_ignored
                || !(config_file::is_ignored(&config_trust_root(p)) || config_file::is_ignored(p))
        })
        .collect()
}

pub fn is_global_config(path: &Path) -> bool {
    global_config_files().contains(path) || system_config_files().contains(path)
}

pub fn is_system_config(path: &Path) -> bool {
    system_config_files().contains(path)
}

static GLOBAL_CONFIG_FILES: Lazy<Mutex<Option<IndexSet<PathBuf>>>> = Lazy::new(Default::default);
static SYSTEM_CONFIG_FILES: Lazy<Mutex<Option<IndexSet<PathBuf>>>> = Lazy::new(Default::default);

pub fn global_config_files() -> IndexSet<PathBuf> {
    let mut g = GLOBAL_CONFIG_FILES.lock().unwrap();
    if let Some(g) = &*g {
        return g.clone();
    }
    if let Some(global_config_file) = &*env::MISE_GLOBAL_CONFIG_FILE {
        return vec![global_config_file.clone()].into_iter().collect();
    }
    let mut config_files = IndexSet::new();
    if !*env::MISE_USE_TOML {
        // only add ~/.tool-versions if MISE_CONFIG_FILE is not set
        // because that's how the user overrides the default
        config_files.insert(dirs::HOME.join(env::MISE_DEFAULT_TOOL_VERSIONS_FILENAME.as_str()));
    };
    config_files.extend(config_files_from_dir(&dirs::CONFIG));
    *g = Some(config_files.clone());
    config_files
}

pub fn system_config_files() -> IndexSet<PathBuf> {
    let mut s = SYSTEM_CONFIG_FILES.lock().unwrap();
    if let Some(s) = &*s {
        return s.clone();
    }
    if let Some(p) = &*env::MISE_SYSTEM_CONFIG_FILE {
        return vec![p.clone()].into_iter().collect();
    }
    let config_files = config_files_from_dir(&dirs::SYSTEM);
    *s = Some(config_files.clone());
    config_files
}

static CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    let mut filenames = vec!["config.toml".to_string(), "mise.toml".to_string()];
    for env in &*env::MISE_ENV {
        filenames.push(format!("config.{env}.toml"));
        filenames.push(format!("mise.{env}.toml"));
    }
    filenames.push("config.local.toml".to_string());
    filenames.push("mise.local.toml".to_string());
    for env in &*env::MISE_ENV {
        filenames.push(format!("config.{env}.local.toml"));
        filenames.push(format!("mise.{env}.local.toml"));
    }
    filenames
});

fn config_files_from_dir(dir: &Path) -> IndexSet<PathBuf> {
    let mut files = IndexSet::new();
    for p in file::ls(&dir.join("conf.d")).unwrap_or_default() {
        if let Some(file_name) = p.file_name().map(|f| f.to_string_lossy().to_string()) {
            if !file_name.starts_with(".") && file_name.ends_with(".toml") {
                files.insert(p);
            }
        }
    }
    files.extend(CONFIG_FILENAMES.iter().map(|f| dir.join(f)));
    files.into_iter().filter(|p| p.is_file()).collect()
}

/// the top-most global config file or the path to where it should be written to
pub fn global_config_path() -> PathBuf {
    global_config_files()
        .last()
        .cloned()
        .or_else(|| env::MISE_GLOBAL_CONFIG_FILE.clone())
        .unwrap_or_else(|| dirs::CONFIG.join("config.toml"))
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

async fn load_all_config_files(
    config_filenames: &[PathBuf],
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<ConfigMap> {
    backend::load_tools().await?;
    Ok(config_filenames
        .iter()
        .unique()
        .map(|f| {
            if f.is_dir() {
                return Ok(None);
            }
            let cf = match parse_config_file(f, idiomatic_filenames) {
                Ok(cfg) => cfg,
                Err(err) => {
                    if err.to_string().contains("are not trusted.") {
                        warn!("{err}");
                        return Ok(None);
                    }
                    return Err(err.wrap_err(format!(
                        "error parsing config file: {}",
                        style::ebold(display_path(f))
                    )));
                }
            };
            if let Err(err) = Tracker::track(f) {
                warn!("tracking config: {err:#}");
            }
            Ok(Some((f.clone(), cf)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

fn parse_config_file(
    f: &PathBuf,
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<Arc<dyn ConfigFile>> {
    match idiomatic_filenames.get(&f.file_name().unwrap().to_string_lossy().to_string()) {
        Some(plugin) => {
            trace!("idiomatic version file: {}", display_path(f));
            let tools = backend::list()
                .into_iter()
                .filter(|f| plugin.contains(&f.to_string()))
                .collect::<Vec<_>>();
            IdiomaticVersionFile::parse(f.into(), tools).map(|f| Arc::new(f) as Arc<dyn ConfigFile>)
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

async fn load_vars(config: &Arc<Config>) -> Result<EnvResults> {
    time!("load_vars start");
    let entries = config
        .config_files
        .iter()
        .rev()
        .map(|(source, cf)| {
            cf.vars_entries()
                .map(|ee| ee.into_iter().map(|e| (e, source.clone())))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let vars_results = EnvResults::resolve(
        config,
        config.tera_ctx.clone(),
        &env::PRISTINE_ENV,
        entries,
        EnvResolveOptions {
            vars: true,
            ..Default::default()
        },
    )
    .await?;
    time!("load_vars done");
    if log::log_enabled!(log::Level::Trace) {
        trace!("{vars_results:#?}");
    } else if !vars_results.is_empty() {
        debug!("{vars_results:?}");
    }
    Ok(vars_results)
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
            if !env_results.env_scripts.is_empty() {
                s.field("Scripts", &env_results.env_scripts);
            }
            if !env_results.env_files.is_empty() {
                s.field("Files", &env_results.env_files);
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

#[async_backtrace::framed]
pub async fn rebuild_shims_and_runtime_symlinks(
    config: &Arc<Config>,
    ts: &Toolset,
    new_versions: &[ToolVersion],
) -> Result<()> {
    measure!("rebuilding shims", {
        shims::reshim(config, ts, false)
            .await
            .wrap_err("failed to rebuild shims")?;
    });
    measure!("rebuilding runtime symlinks", {
        runtime_symlinks::rebuild(config)
            .await
            .wrap_err("failed to rebuild runtime symlinks")?;
    });
    measure!("updating lockfiles", {
        lockfile::update_lockfiles(config, ts, new_versions)
            .wrap_err("failed to update lockfiles")?;
    });

    Ok(())
}

fn warn_about_idiomatic_version_files(config_files: &ConfigMap) {
    if Settings::get()
        .idiomatic_version_file_enable_tools
        .as_ref()
        .is_some()
    {
        return;
    }
    debug_assert!(
        !VERSION.starts_with("2025.10"),
        "default idiomatic version files to disabled"
    );
    let Some((p, tool)) = config_files
        .iter()
        .filter(|(_, cf)| cf.config_type() == ConfigFileType::IdiomaticVersion)
        .filter_map(|(p, cf)| cf.to_tool_request_set().ok().map(|ts| (p, ts.tools)))
        .filter_map(|(p, tools)| tools.first().map(|(ba, _)| (p, ba.to_string())))
        .next()
    else {
        return;
    };
    deprecated!(
        "idiomatic_version_file_enable_tools",
        r#"
Idiomatic version files like {} are currently enabled by default. However, this will change in mise 2025.10.0 to instead default to disabled.

You can remove this warning by explicitly enabling idiomatic version files for {} with:

    mise settings add idiomatic_version_file_enable_tools {}

You can disable idiomatic version files with:

    mise settings add idiomatic_version_file_enable_tools "[]"

See https://github.com/jdx/mise/discussions/4345 for more information."#,
        display_path(p),
        tool,
        tool
    );
}

async fn load_local_tasks(config: &Arc<Config>) -> Result<Vec<Task>> {
    let mut tasks = vec![];
    for d in file::all_dirs()? {
        if cfg!(test) && !d.starts_with(*dirs::HOME) {
            continue;
        }
        tasks.extend(load_tasks_in_dir(config, &d, &config.config_files).await?);
    }
    Ok(tasks)
}

async fn load_global_tasks(config: &Arc<Config>) -> Result<Vec<Task>> {
    let global_config_files = config
        .config_files
        .values()
        .filter(|cf| is_global_config(cf.get_path()))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    for cf in global_config_files {
        let cf = cf.clone();
        let config = config.clone();
        tasks.extend(load_config_and_file_tasks(&config, cf).await?);
    }
    Ok(tasks)
}

async fn load_system_tasks(config: &Arc<Config>) -> Result<Vec<Task>> {
    let system_config_files = config
        .config_files
        .values()
        .filter(|cf| is_system_config(cf.get_path()))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    for cf in system_config_files {
        let cf = cf.clone();
        let config = config.clone();
        tasks.extend(load_config_and_file_tasks(&config, cf).await?);
    }
    Ok(tasks)
}

async fn load_config_and_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
) -> Result<Vec<Task>> {
    let project_root = cf.project_root().unwrap_or(&*env::HOME);
    let tasks = load_config_tasks(config, cf.clone(), project_root).await?;
    let file_tasks = load_file_tasks(config, cf.clone(), project_root).await?;
    Ok(tasks.into_iter().chain(file_tasks).collect())
}

async fn load_config_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let is_global = is_global_config(cf.get_path());
    let config_root = Arc::new(config_root.to_path_buf());
    let mut tasks = vec![];
    for t in cf.tasks().into_iter() {
        let config_root = config_root.clone();
        let config = config.clone();
        let mut t = t.clone();
        if is_global {
            t.global = true;
        }
        match t.render(&config, &config_root).await {
            Ok(()) => {
                tasks.push(t);
            }
            Err(e) => {
                return Err(e);
            }
        }
    }
    Ok(tasks)
}

async fn load_tasks_includes(
    config: &Arc<Config>,
    root: &Path,
    config_root: &Path,
) -> Result<Vec<Task>> {
    if !root.is_dir() {
        return Ok(vec![]);
    }
    let files = WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        // skip hidden directories (if the root is hidden that's ok)
        .filter_entry(|e| e.path() == root || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_ok(|e| e.file_type().is_file())
        .map_ok(|e| e.path().to_path_buf())
        .try_collect::<_, Vec<PathBuf>, _>()?
        .into_iter()
        .filter(|p| file::is_executable(p))
        .filter(|p| {
            !Settings::get()
                .task_disable_paths
                .iter()
                .any(|d| p.starts_with(d))
        })
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    let root = Arc::new(root.to_path_buf());
    let config_root = Arc::new(config_root.to_path_buf());
    for path in files {
        let root = root.clone();
        let config_root = config_root.clone();
        let config = config.clone();
        tasks.push(Task::from_path(&config, &path, &root, &config_root).await?);
    }
    Ok(tasks)
}

async fn load_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let includes = cf
        .task_config()
        .includes
        .clone()
        .unwrap_or(vec!["tasks".into()])
        .into_iter()
        .map(|p| cf.get_path().parent().unwrap().join(p))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    let config_root = Arc::new(config_root.to_path_buf());
    for p in includes {
        let config_root = config_root.clone();
        let config = config.clone();
        tasks.extend(load_tasks_includes(&config, &p, &config_root).await?);
    }
    Ok(tasks)
}

pub fn task_includes_for_dir(dir: &Path, config_files: &ConfigMap) -> Vec<PathBuf> {
    configs_at_root(dir, config_files)
        .iter()
        .rev()
        .find_map(|cf| cf.task_config().includes.clone())
        .unwrap_or_else(default_task_includes)
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { dir.join(p) })
        .filter(|p| p.exists())
        .collect::<Vec<_>>()
        .into_iter()
        .unique()
        .collect::<Vec<_>>()
}

pub async fn load_tasks_in_dir(
    config: &Arc<Config>,
    dir: &Path,
    config_files: &ConfigMap,
) -> Result<Vec<Task>> {
    let configs = configs_at_root(dir, config_files);
    let mut config_tasks = vec![];
    for cf in configs {
        let dir = dir.to_path_buf();
        config_tasks.extend(load_config_tasks(config, cf.clone(), &dir).await?);
    }
    let includes = task_includes_for_dir(dir, config_files);
    let extra_tasks = includes
        .iter()
        .filter(|p| p.is_file() && p.extension().unwrap_or_default().to_string_lossy() == "toml");
    for p in extra_tasks {
        let p = p.clone();
        let dir = dir.to_path_buf();
        let config = config.clone();
        config_tasks.extend(load_task_file(&config, &p, &dir).await?);
    }
    let mut file_tasks = vec![];
    for p in includes {
        let dir = dir.to_path_buf();
        let p = p.clone();
        let config = config.clone();
        file_tasks.extend(load_tasks_includes(&config, &p, &dir).await?);
    }
    let mut tasks = file_tasks
        .into_iter()
        .chain(config_tasks)
        .sorted_by_cached_key(|t| t.name.clone())
        .collect::<Vec<_>>();
    let all_tasks = tasks
        .clone()
        .into_iter()
        .map(|t| (t.name.clone(), t))
        .collect::<BTreeMap<_, _>>();
    for task in tasks.iter_mut() {
        task.display_name = task.display_name(&all_tasks);
    }
    Ok(tasks)
}

async fn load_task_file(
    config: &Arc<Config>,
    path: &Path,
    config_root: &Path,
) -> Result<Vec<Task>> {
    let raw = file::read_to_string_async(path).await?;
    let mut tasks = toml::from_str::<Tasks>(&raw)
        .wrap_err_with(|| format!("Error parsing task file: {}", display_path(path)))?
        .0;
    for (name, task) in &mut tasks {
        task.name = name.clone();
        task.config_source = path.to_path_buf();
        task.config_root = Some(config_root.to_path_buf());
    }
    let mut out = vec![];
    for (_, mut task) in tasks {
        let config_root = config_root.to_path_buf();
        if let Err(err) = task.render(config, &config_root).await {
            warn!("rendering task: {err:?}");
        }
        out.push(task);
    }
    Ok(out)
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use insta::assert_debug_snapshot;
    use std::collections::BTreeMap;
    use std::fs::{self, File};
    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn test_load() {
        let config = Config::reset().await.unwrap();
        assert_debug_snapshot!(config);
    }

    #[tokio::test]
    async fn test_load_all_config_files_skips_directories() -> Result<()> {
        let _config = Config::get().await?;
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        let sub_dir = temp_path.join("subdir");
        fs::create_dir(&sub_dir)?;

        let file1_path = temp_path.join("config1.toml");
        let file2_path = temp_path.join("config2.toml");
        File::create(&file1_path)?;
        File::create(&file2_path)?;

        fs::write(&file1_path, "key1 = 'value1'")?;
        fs::write(&file2_path, "key2 = 'value2'")?;

        let config_filenames = vec![file1_path.clone(), file2_path.clone(), sub_dir.clone()];
        let idiomatic_filenames = BTreeMap::new();

        let result = load_all_config_files(&config_filenames, &idiomatic_filenames).await?;

        // the result should have only two entries for the files, the directory should not be present
        assert_eq!(result.len(), 2);

        // Check that the directory is not in the result
        assert!(result.contains_key(&file1_path));
        assert!(result.contains_key(&file2_path));
        assert!(!result.contains_key(&sub_dir));

        Ok(())
    }
}
