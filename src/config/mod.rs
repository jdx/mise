use dashmap::DashMap;
use eyre::{Context, Result, bail, eyre};
use indexmap::{IndexMap, IndexSet};
use itertools::Itertools;
pub use settings::Settings;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env::join_paths;
use std::fmt::{Debug, Formatter};
use std::iter::once;
use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime};
use tokio::{sync::OnceCell, task::JoinSet};
use walkdir::WalkDir;

use crate::backend::ABackend;
use crate::cli::args::BackendArg;
use crate::cli::version;
use crate::config::config_file::idiomatic_version::IdiomaticVersionFile;
use crate::config::config_file::min_version::MinVersionSpec;
use crate::config::config_file::mise_toml::{MiseToml, Tasks};
use crate::config::config_file::{ConfigFile, config_trust_root};
use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::tracking::Tracker;
use crate::env::{MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;
use crate::shorthands::{Shorthands, get_shorthands};
use crate::task::task_file_providers::TaskFileProvidersBuilder;
use crate::task::{Task, TaskTemplate};
use crate::tera::take_tera_accessed_files;
use crate::toolset::env_cache::{CachedNonToolEnv, compute_settings_hash, get_file_mtime};
use crate::toolset::{
    ToolRequestSet, ToolRequestSetBuilder, ToolVersion, ToolVersionOptions, Toolset, install_state,
};
use crate::ui::style;
use crate::{backend, dirs, env, file, lockfile, registry, runtime_symlinks, shims, timeout};

pub mod config_file;
pub mod env_directive;
pub mod miserc;
pub mod settings;
pub mod tracking;

use crate::env_diff::EnvMap;
use crate::hook_env::WatchFilePattern;
use crate::hooks::Hook;
use crate::plugins::PluginType;
use crate::redactions::Redactor;
use crate::tera::BASE_CONTEXT;
use crate::watch_files::WatchFile;
use crate::wildcard::Wildcard;

type AliasMap = IndexMap<String, Alias>;
pub(crate) type ConfigMap = IndexMap<PathBuf, Arc<dyn ConfigFile>>;
pub type EnvWithSources = IndexMap<String, (String, PathBuf)>;

pub struct Config {
    pub config_files: ConfigMap,
    pub project_root: Option<PathBuf>,
    pub all_aliases: AliasMap,
    pub repo_urls: HashMap<String, String>,
    pub vars: IndexMap<String, String>,
    pub tera_ctx: tera::Context,
    pub shorthands: Shorthands,
    pub shell_aliases: EnvWithSources,
    /// Files accessed by tera template functions (read_file, hash_file, etc.)
    /// during shell alias template rendering, used to watch for changes in hook-env.
    pub tera_files: Vec<PathBuf>,
    aliases: AliasMap,
    env: OnceCell<EnvResults>,
    env_with_sources: OnceCell<EnvWithSources>,
    hooks: OnceCell<Vec<(PathBuf, Hook)>>,
    tasks_cache: Arc<DashMap<crate::task::TaskLoadContext, Arc<BTreeMap<String, Task>>>>,
    tool_request_set: OnceCell<ToolRequestSet>,
    toolset: OnceCell<Toolset>,
    vars_loader: Option<Arc<Config>>,
    vars_results: OnceCell<EnvResults>,
}

#[derive(Debug, Clone, Default)]
pub struct Alias {
    pub backend: Option<String>,
    pub versions: IndexMap<String, String>,
}

static _CONFIG: RwLock<Option<Arc<Config>>> = RwLock::new(None);
static _REDACTOR: Lazy<Mutex<Redactor>> = Lazy::new(Default::default);

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
    pub fn maybe_get() -> Option<Arc<Self>> {
        _CONFIG.read().unwrap().as_ref().cloned()
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

        let mut config = Self {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files,
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: get_shorthands(&Settings::get()),
            hooks: OnceCell::new(),
            tasks_cache: Arc::new(DashMap::new()),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases: Default::default(),
            aliases: Default::default(),
            project_root: Default::default(),
            repo_urls: Default::default(),
            shell_aliases: Default::default(),
            tera_files: Default::default(),
            vars: Default::default(),
            vars_loader: None,
            vars_results: OnceCell::new(),
        };
        let vars_config = Arc::new(Self {
            tera_ctx: config.tera_ctx.clone(),
            config_files: config.config_files.clone(),
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: config.shorthands.clone(),
            hooks: OnceCell::new(),
            tasks_cache: Arc::new(DashMap::new()),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases: config.all_aliases.clone(),
            aliases: config.aliases.clone(),
            project_root: config.project_root.clone(),
            repo_urls: config.repo_urls.clone(),
            shell_aliases: config.shell_aliases.clone(),
            tera_files: config.tera_files.clone(),
            vars: config.vars.clone(),
            vars_loader: None,
            vars_results: OnceCell::new(),
        });
        let vars_results = measure!("config::load vars_results", {
            let results = load_vars(&vars_config).await?;
            vars_config.vars_results.set(results.clone()).ok();
            config.vars_results.set(results.clone()).ok();
            config.vars_loader = Some(vars_config.clone());
            results
        });
        let vars: IndexMap<String, String> = vars_results
            .vars
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect();
        config.tera_ctx.insert("vars", &vars);

        config.vars = vars;
        config.aliases = load_aliases(&config.config_files)?;
        // Clear any previously tracked files before loading shell aliases
        let _ = take_tera_accessed_files();
        config.shell_aliases = load_shell_aliases(&config.config_files)?;
        config.tera_files = take_tera_accessed_files();
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
                // check plugin type, fallback to asdf
                let (mut plugin_type, has_explicit_prefix) = match plugin {
                    p if p.starts_with("vfox:") => (PluginType::Vfox, true),
                    p if p.starts_with("vfox-backend:") => (PluginType::VfoxBackend, true),
                    p if p.starts_with("asdf:") => (PluginType::Asdf, true),
                    _ => (PluginType::Asdf, false),
                };
                // keep backward compatibility for vfox plugins, but only if no explicit prefix
                if !has_explicit_prefix && url.contains("vfox-") {
                    plugin_type = PluginType::Vfox;
                }

                let plugin = plugin
                    .strip_prefix("vfox:")
                    .or_else(|| plugin.strip_prefix("vfox-backend:"))
                    .or_else(|| plugin.strip_prefix("asdf:"))
                    .unwrap_or(plugin);

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
        config.env_results().await?;
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

    pub async fn vars_results(self: &Arc<Self>) -> Result<&EnvResults> {
        if let Some(loader) = &self.vars_loader
            && let Some(results) = loader.vars_results.get()
        {
            return Ok(results);
        }
        self.vars_results
            .get_or_try_init(|| async move { load_vars(self).await })
            .await
    }

    pub fn env_results_cached(&self) -> Option<&EnvResults> {
        self.env.get()
    }
    pub fn vars_results_cached(&self) -> Option<&EnvResults> {
        self.vars_results.get()
    }
    pub async fn path_dirs(self: &Arc<Self>) -> eyre::Result<&Vec<PathBuf>> {
        Ok(&self.env_results().await?.env_paths)
    }
    pub async fn get_tool_request_set(self: &Arc<Self>) -> eyre::Result<&ToolRequestSet> {
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

    pub async fn get_tool_opts(
        self: &Arc<Self>,
        backend_arg: &Arc<BackendArg>,
    ) -> Result<Option<ToolVersionOptions>> {
        let trs = self.get_tool_request_set().await?;
        // Try matching by resolved full name first for aliased tools.
        // e.g., ba.short="treesize" resolves to full="gitlab:FBibonne/treesize"
        // while the config entry has short="gitlab-f-bibonne-treesize" with api_url set.
        // We check the resolved name first because the direct short match might find
        // a CLI-created tool request without options.
        let full = backend_arg.full();
        let resolved_ba = BackendArg::new(full, None);
        let tool_request = trs
            .iter()
            .find(|tr| tr.0.short == resolved_ba.short)
            .or_else(|| trs.iter().find(|tr| tr.0.short == backend_arg.short));
        Ok(tool_request.and_then(|tr| tr.1.first().map(|req| req.options())))
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

        if let Some(url) = self
            .repo_urls
            .keys()
            .find(|k| k.ends_with(&format!(":{plugin_name}")))
            .and_then(|k| self.repo_urls.get(k))
        {
            return Some(url.clone());
        }

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

    pub fn is_monorepo(&self) -> bool {
        find_monorepo_root(&self.config_files).is_some()
    }

    pub async fn tasks(&self) -> Result<Arc<BTreeMap<String, Task>>> {
        self.tasks_with_context(None).await
    }

    pub async fn tasks_with_context(
        &self,
        ctx: Option<&crate::task::TaskLoadContext>,
    ) -> Result<Arc<BTreeMap<String, Task>>> {
        // Use the entire context as cache key
        // Default context (None) becomes TaskLoadContext::default()
        let cache_key = ctx.cloned().unwrap_or_default();

        // Check if already cached
        if let Some(cached) = self.tasks_cache.get(&cache_key) {
            return Ok(cached.value().clone());
        }

        // Not cached, load tasks
        let tasks = measure!("config::load_all_tasks_with_context", {
            self.load_all_tasks_with_context(ctx).await?
        });
        let tasks_arc = Arc::new(tasks);

        // Insert into cache
        self.tasks_cache.insert(cache_key, tasks_arc.clone());

        Ok(tasks_arc)
    }

    pub async fn tasks_with_aliases(&self) -> Result<BTreeMap<String, Task>> {
        let tasks = self.tasks().await?;
        Ok(tasks
            .iter()
            .flat_map(|(_, t)| {
                t.aliases
                    .iter()
                    .map(|a| (a.to_string(), t.clone()))
                    .chain(once((t.name.clone(), t.clone())))
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub async fn resolve_alias(&self, backend: &ABackend, v: &str) -> Result<String> {
        if let Some(plugin_aliases) = self.all_aliases.get(&backend.ba().short)
            && let Some(alias) = plugin_aliases.versions.get(v)
        {
            return Ok(alias.clone());
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

    async fn load_all_tasks_with_context(
        &self,
        ctx: Option<&crate::task::TaskLoadContext>,
    ) -> Result<BTreeMap<String, Task>> {
        let config = Config::get().await?;
        time!("load_all_tasks");

        // Collect all task templates from config hierarchy (experimental feature)
        let templates = if Settings::get().experimental {
            collect_task_templates(&config.config_files)
        } else {
            IndexMap::new()
        };

        let local_tasks = load_local_tasks_with_context(&config, ctx, &templates).await?;
        let global_tasks = load_global_tasks(&config, &templates).await?;
        let mut tasks: BTreeMap<String, Task> = local_tasks
            .into_iter()
            .chain(global_tasks)
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

    pub async fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        let mut config_files: ConfigMap = ConfigMap::default();
        for path in Tracker::list_all()?.into_iter() {
            // Pre-check trust to avoid interactive prompts when loading
            // tracked configs (e.g., during `mise upgrade`). Only MiseToml files
            // call trust_check during parsing, but we can't cheaply distinguish
            // file types here, so we check trust for all files and fall through
            // to parse for trusted files. Untrusted non-MiseToml files (like
            // .tool-versions) don't need trust and will parse fine regardless.
            let trust_root = config_file::config_trust_root(&path);
            if !config_file::is_trusted(&trust_root) && !config_file::is_trusted(&path) {
                debug!("skipping untrusted tracked config: {}", display_path(&path));
                continue;
            }
            match config_file::parse(&path).await {
                Ok(cf) => {
                    config_files.insert(path, cf);
                }
                Err(err) => {
                    warn!(
                        "error loading tracked config file {}: {err:#}",
                        display_path(&path)
                    );
                }
            }
        }
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
        self.validate_versions()?;
        Ok(())
    }

    fn validate_versions(&self) -> eyre::Result<()> {
        for cf in self.config_files.values() {
            if let Some(spec) = cf.min_version() {
                Self::enforce_min_version_spec(spec)?;
            }
        }
        Ok(())
    }

    pub fn enforce_min_version_spec(spec: &MinVersionSpec) -> eyre::Result<()> {
        let cur = &*version::V;
        if let Some(required) = spec.hard_violation(cur) {
            let min = style::eyellow(required);
            let cur = style::eyellow(cur);
            let msg = format!("mise version {min} is required, but you are using {cur}");
            bail!(crate::cli::self_update::append_self_update_instructions(
                msg
            ));
        } else if let Some(recommended) = spec.soft_violation(cur) {
            let min = style::eyellow(recommended);
            let cur = style::eyellow(cur);
            let msg = format!("mise version {min} is recommended, but you are using {cur}");
            warn!(
                "{}",
                crate::cli::self_update::append_self_update_instructions(msg)
            );
        }
        Ok(())
    }

    async fn load_env(self: &Arc<Self>) -> Result<EnvResults> {
        if Settings::no_env() || Settings::get().no_env.unwrap_or(false) {
            return Ok(EnvResults::default());
        }
        time!("load_env start");
        let cache_enabled = CachedNonToolEnv::is_enabled();
        let cache_key = if cache_enabled {
            let config_files: Vec<(PathBuf, u64)> = self
                .config_files
                .keys()
                .map(|p| (p.clone(), get_file_mtime(p).unwrap_or(0)))
                .collect();
            let settings_hash = compute_settings_hash();
            let base_path = join_paths(env::PATH.iter())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            Some(CachedNonToolEnv::compute_cache_key(
                &config_files,
                &settings_hash,
                &base_path,
            ))
        } else {
            None
        };
        if let Some(cache_key) = cache_key.as_ref()
            && let Some(cached) = CachedNonToolEnv::load(cache_key)?
        {
            let env_results = EnvResults {
                env: cached.env.clone(),
                vars: Default::default(),
                env_remove: cached.env_remove.clone(),
                env_files: cached.env_files.clone(),
                env_paths: cached.env_paths.clone(),
                env_scripts: cached.env_scripts.clone(),
                redactions: cached.redactions.clone(),
                tool_add_paths: Vec::new(),
                watch_files: cached.watch_files.clone(),
                has_uncacheable: false,
            };
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
            trace!("env_cache: using cached non-tool env results");
            return Ok(env_results);
        }
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
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::NonToolsOnly,
                warn_on_missing_required: *env::WARN_ON_MISSING_REQUIRED_ENV,
            },
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
        if cache_enabled
            && !env_results.has_uncacheable
            && let Some(cache_key) = cache_key
        {
            let mut watch_files = env_results.watch_files.clone();
            watch_files.extend(env_results.env_files.clone());
            watch_files.extend(env_results.env_scripts.clone());
            let watch_file_mtimes: Vec<u64> = watch_files
                .iter()
                .map(|p| get_file_mtime(p).unwrap_or(0))
                .collect();
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let cached = CachedNonToolEnv {
                env: env_results.env.clone(),
                env_remove: env_results.env_remove.clone(),
                env_files: env_results.env_files.clone(),
                env_paths: env_results.env_paths.clone(),
                env_scripts: env_results.env_scripts.clone(),
                redactions: env_results.redactions.clone(),
                watch_files,
                watch_file_mtimes,
                created_at: now,
                mise_version: env!("CARGO_PKG_VERSION").to_string(),
                cache_key_debug: cache_key.clone(),
            };
            if let Err(e) = cached.save(&cache_key) {
                debug!("env_cache: failed to save non-tool env cache: {}", e);
            }
        }
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
                    .map(|cf| {
                        let is_global = cf.project_root().is_none();
                        let root = cf.project_root().unwrap_or_else(|| cf.config_root());
                        let mut hooks = cf.hooks()?;
                        if is_global {
                            for h in &mut hooks {
                                h.global = true;
                            }
                        }
                        Ok((root, hooks))
                    })
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
            .chain(self.tera_files.iter().map(|p| p.as_path().into()))
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
        let mut r = _REDACTOR.lock().unwrap();
        let new_redactions = redactions.into_iter().flat_map(|pattern| {
            let matcher = Wildcard::new(vec![pattern]);
            env.iter()
                .filter(|(k, _)| matcher.match_any(k))
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>()
        });
        *r = r.with_additional(new_redactions);
    }

    /// Get the current redaction patterns.
    pub fn redactions(&self) -> Arc<IndexSet<String>> {
        _REDACTOR.lock().unwrap().patterns_arc()
    }

    /// Redact sensitive values from a string using Aho-Corasick for efficiency.
    pub fn redact(&self, input: &str) -> String {
        _REDACTOR.lock().unwrap().redact(input)
    }
}

fn configs_at_root<'a>(dir: &Path, config_files: &'a ConfigMap) -> Vec<&'a Arc<dyn ConfigFile>> {
    let mut configs: Vec<&'a Arc<dyn ConfigFile>> = DEFAULT_CONFIG_FILENAMES
        .iter()
        .rev()
        .flat_map(|f| {
            if f.contains('*') {
                // Handle glob patterns by matching against actual config file paths
                glob(dir, f)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|path| config_files.get(&path))
                    .collect::<Vec<_>>()
            } else {
                // Handle regular filenames
                config_files
                    .get(&dir.join(f))
                    .into_iter()
                    .collect::<Vec<_>>()
            }
        })
        .collect();
    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    configs.retain(|cf| seen.insert(cf.get_path().to_path_buf()));
    configs
}

fn get_project_root(config_files: &ConfigMap) -> Option<PathBuf> {
    let project_root = config_files
        .values()
        .find_map(|cf| cf.project_root())
        .map(|pr| pr.to_path_buf());
    trace!("project_root: {project_root:?}");
    project_root
}

fn find_monorepo_root(config_files: &ConfigMap) -> Option<PathBuf> {
    find_monorepo_config(config_files).and_then(|cf| cf.project_root().map(|p| p.to_path_buf()))
}

/// Find the config file that has experimental_monorepo_root = true
fn find_monorepo_config(config_files: &ConfigMap) -> Option<&Arc<dyn ConfigFile>> {
    // This feature requires experimental mode
    if !Settings::get().experimental {
        return None;
    }
    config_files
        .values()
        .find(|cf| cf.experimental_monorepo_root() == Some(true))
}

async fn load_idiomatic_files() -> BTreeMap<String, Vec<String>> {
    let enable_tools = Settings::get().idiomatic_version_file_enable_tools.clone();
    if enable_tools.is_empty() {
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
    for tool in backend::list() {
        let enable_tools = enable_tools.clone();
        jset.spawn(async move {
            if !enable_tools.contains(tool.id()) {
                return vec![];
            }
            match tool.idiomatic_filenames().await {
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
            "mise/config.local.toml",
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
        .filter(|p| config_file::is_ignored(&config_trust_root(p)) || config_file::is_ignored(p))
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

fn all_dirs() -> Result<Vec<PathBuf>> {
    file::all_dirs(env::current_dir()?, &env::MISE_CEILING_PATHS)
}

/// Get all directories in the hierarchy from a starting directory up to ceiling paths
fn all_dirs_from(start_dir: &Path) -> Result<Vec<PathBuf>> {
    file::all_dirs(start_dir, &env::MISE_CEILING_PATHS)
}

/// Returns true if a path is a .tool-versions file (lower priority for writes)
fn is_tool_versions_file(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|f| f.to_string_lossy().ends_with(".tool-versions"))
}

/// Get the first (lowest precedence) config file, but skip .tool-versions unless
/// it's the only option. This ensures commands like `mise use` write to mise.toml
/// instead of mise.local.toml or .tool-versions when multiple configs exist.
/// See: https://github.com/jdx/mise/discussions/6475
fn first_config_file(files: &IndexSet<PathBuf>) -> Option<&PathBuf> {
    files
        .iter()
        .find(|p| !is_tool_versions_file(p) && !is_conf_d_file(p))
        .or_else(|| files.first())
}

fn is_conf_d_file(p: &Path) -> bool {
    p.parent()
        .is_some_and(|d| d.file_name().is_some_and(|n| n == "conf.d"))
}

pub fn config_file_from_dir(p: &Path) -> PathBuf {
    if !p.is_dir() {
        return p.to_path_buf();
    }
    for dir in all_dirs().unwrap_or_default() {
        let files = self::config_files_in_dir(&dir);
        if let Some(cf) = first_config_file(&files)
            && !is_global_config(cf)
        {
            return cf.clone();
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
    let dirs = all_dirs().unwrap_or_default();

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
            if is_default_config_dir_override_filtered(p) {
                return false;
            }
            include_ignored
                || !(config_file::is_ignored(&config_trust_root(p)) || config_file::is_ignored(p))
        })
        .collect()
}

/// Load config hierarchy from a specific directory (for monorepo tasks)
/// This loads all config files from start_dir up through parent directories,
/// including MISE_ENV-specific configs
pub fn load_config_hierarchy_from_dir(start_dir: &Path) -> Result<Vec<PathBuf>> {
    if Settings::no_config() {
        return Ok(vec![]);
    }

    let config_filenames = DEFAULT_CONFIG_FILENAMES.iter().cloned().collect_vec();

    // Get all directories from start_dir up to root/ceiling
    let dirs = all_dirs_from(start_dir)?;

    let mut config_files = dirs
        .iter()
        .flat_map(|dir| {
            if env::MISE_IGNORED_CONFIG_PATHS
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

    // Add global and system configs
    config_files.extend(global_config_files());
    config_files.extend(system_config_files());

    let paths = config_files
        .into_iter()
        .unique_by(|p| file::desymlink_path(p))
        .filter(|p| {
            if is_default_config_dir_override_filtered(p) {
                return false;
            }
            !(config_file::is_ignored(&config_trust_root(p)) || config_file::is_ignored(p))
        })
        .collect();

    Ok(paths)
}

pub fn is_global_config(path: &Path) -> bool {
    global_config_files().contains(path) || system_config_files().contains(path)
}

/// Returns true if the path should be filtered out due to MISE_CONFIG_DIR override.
/// When MISE_CONFIG_DIR is set to a non-default location, this filters out configs
/// found under the default location (~/.config/mise) during traversal.
/// See: https://github.com/jdx/mise/discussions/7015
fn is_default_config_dir_override_filtered(path: &Path) -> bool {
    *env::MISE_CONFIG_DIR_OVERRIDDEN
        && !global_config_files().contains(path)
        && path.starts_with(&*env::MISE_DEFAULT_CONFIG_DIR)
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
        if let Some(file_name) = p.file_name().map(|f| f.to_string_lossy().to_string())
            && !file_name.starts_with(".")
            && file_name.ends_with(".toml")
        {
            files.insert(p);
        }
    }
    files.extend(CONFIG_FILENAMES.iter().map(|f| dir.join(f)));
    files.into_iter().filter(|p| p.is_file()).collect()
}

/// the preferred global config file to write to, or the path where it should be created.
/// Uses first_config_file() to pick the lowest-precedence non-local TOML (i.e., config.toml
/// rather than config.local.toml) so that `mise use -g` writes to config.toml.
/// See: https://github.com/jdx/mise/discussions/8236
pub fn global_config_path() -> PathBuf {
    let files = global_config_files();
    first_config_file(&files)
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

/// The last (lowest precedence) local mise.toml or the path to where it should be written to.
/// This ensures commands write to mise.toml instead of mise.local.toml when both exist.
/// Note: local_toml_config_paths() returns files in highest-to-lowest precedence order,
/// so we use .last() to get the lowest precedence file.
pub fn local_toml_config_path() -> PathBuf {
    static CWD: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("."));
    local_toml_config_paths()
        .into_iter()
        .last()
        .cloned()
        .unwrap_or_else(|| {
            dirs::CWD
                .as_ref()
                .unwrap_or(&CWD)
                .join(&*env::MISE_DEFAULT_CONFIG_FILENAME)
        })
}

/// Options for resolving target config file path
#[derive(Debug, Default)]
pub struct ConfigPathOptions {
    pub global: bool,
    pub path: Option<PathBuf>,
    pub env: Option<String>,
    pub cwd: Option<PathBuf>,
    pub prefer_toml: bool,
    pub prevent_home_local: bool,
}

/// Unified config file path resolution for both `mise use` and `mise set`
///
/// This function centralizes the logic for determining which config file to target
/// based on various options, ensuring consistent behavior between commands.
pub fn resolve_target_config_path(opts: ConfigPathOptions) -> Result<PathBuf> {
    let cwd = match opts.cwd {
        Some(ref path) => path.clone(),
        None => env::current_dir()?,
    };

    // If path is provided, handle it (file or directory) - explicit paths take precedence
    if let Some(ref path) = opts.path {
        if path.is_file() {
            return Ok(path.clone());
        } else if path.is_dir() {
            let resolved = config_file_from_dir(path);
            if opts.prefer_toml && !resolved.to_string_lossy().ends_with(".toml") {
                // For TOML-only commands, ensure we get a TOML file in the specified directory
                return Ok(path.join(&*env::MISE_DEFAULT_CONFIG_FILENAME));
            }
            return Ok(resolved);
        } else {
            // Path doesn't exist yet, return it as-is
            return Ok(path.clone());
        }
    }

    // If global flag is set and no explicit path provided, use global config
    if opts.global {
        return Ok(global_config_path());
    }

    // If env-specific config is requested
    if let Some(ref env_name) = opts.env {
        let dotfile_path = cwd.join(format!(".mise.{}.toml", env_name));
        if dotfile_path.exists() {
            return Ok(dotfile_path);
        } else {
            return Ok(cwd.join(format!("mise.{}.toml", env_name)));
        }
    }

    // If we're in HOME directory and prevent_home_local is true, use global config
    if opts.prevent_home_local && env::in_home_dir() {
        return Ok(global_config_path());
    }

    // Default: determine based on current directory
    if opts.prefer_toml {
        // For mise set, prefer TOML and use local_toml_config_path logic
        Ok(local_toml_config_path())
    } else {
        // For mise use, use existing config_file_from_dir logic which respects ASDF compat
        Ok(config_file_from_dir(&cwd))
    }
}

async fn load_all_config_files(
    config_filenames: &[PathBuf],
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<ConfigMap> {
    backend::load_tools().await?;
    let mut config_map = ConfigMap::default();
    for f in config_filenames.iter().unique() {
        if f.is_dir() {
            continue;
        }
        let cf = match parse_config_file(f, idiomatic_filenames).await {
            Ok(cfg) => cfg,
            Err(err) => {
                return Err(err.wrap_err(format!(
                    "error parsing config file: {}",
                    style::ebold(display_path(f))
                )));
            }
        };
        if let Err(err) = Tracker::track(f) {
            warn!("tracking config: {err:#}");
        }

        // Mark monorepo roots so descendant configs are implicitly trusted
        if cf.experimental_monorepo_root() == Some(true)
            && let Err(err) = config_file::mark_as_monorepo_root(f)
        {
            warn!("failed to mark monorepo root: {err:#}");
        }

        config_map.insert(f.clone(), cf);
    }
    Ok(config_map)
}

/// Load config files from a list of paths (for monorepo task config contexts)
pub async fn load_config_files_from_paths(config_paths: &[PathBuf]) -> Result<ConfigMap> {
    backend::load_tools().await?;
    let idiomatic_filenames = BTreeMap::new(); // TODO: support idiomatic files in config hierarchy loading
    let mut config_map = ConfigMap::default();

    for f in config_paths.iter().unique() {
        if f.is_dir() {
            continue;
        }
        let cf = match parse_config_file(f, &idiomatic_filenames).await {
            Ok(cfg) => cfg,
            Err(err) => {
                return Err(err.wrap_err(format!(
                    "error parsing config file: {}",
                    style::ebold(display_path(f))
                )));
            }
        };

        config_map.insert(f.clone(), cf);
    }
    Ok(config_map)
}

async fn parse_config_file(
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
            IdiomaticVersionFile::parse(f.into(), tools)
                .await
                .map(|f| Arc::new(f) as Arc<dyn ConfigFile>)
        }
        None => config_file::parse(f).await,
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

fn load_shell_aliases(config_files: &ConfigMap) -> Result<EnvWithSources> {
    let mut shell_aliases: EnvWithSources = EnvWithSources::new();

    // Iterate in reverse order (global -> local) so child directories override parent configs
    for config_file in config_files.values().rev() {
        let path = config_file.get_path().to_path_buf();
        for (name, cmd) in config_file.shell_aliases()? {
            shell_aliases.insert(name, (cmd, path.clone()));
        }
    }
    trace!("load_shell_aliases: {}", shell_aliases.len());

    Ok(shell_aliases)
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
            tools: ToolsFilter::NonToolsOnly,
            warn_on_missing_required: false,
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
        // Note: tasks are now lazily loaded and cached, so we can't access them synchronously here
        // Try to get the default (current hierarchy) cache entry
        let default_ctx = crate::task::TaskLoadContext::default();
        if let Some(tasks) = self.tasks_cache.get(&default_ctx) {
            s.field(
                "Tasks",
                &tasks.values().map(|t| t.to_string()).collect_vec(),
            );
        }
        if let Some(env) = self.env_maybe()
            && !env.is_empty()
        {
            s.field("Env", &env);
            // s.field("Env Sources", &self.env_sources);
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

/// Collect all task templates from the config file hierarchy.
/// Templates from child configs (closer to cwd) override templates from parent configs.
fn collect_task_templates(config_files: &ConfigMap) -> IndexMap<String, TaskTemplate> {
    let mut templates = IndexMap::new();

    // Iterate in reverse order (global -> local) so child directories override parent configs
    for cf in config_files.values().rev() {
        for (name, template) in cf.task_templates() {
            templates.insert(name, template);
        }
    }

    templates
}

/// Resolve a task template and merge it into the task.
/// Returns an error if the template is not found or if experimental mode is not enabled.
fn resolve_task_template(
    task: &mut Task,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<()> {
    if let Some(template_name) = &task.extends {
        if !Settings::get().experimental {
            bail!(
                "Task '{}' uses 'extends = \"{}\"' which requires 'experimental = true' in settings",
                task.name,
                template_name
            );
        }

        let template = templates.get(template_name).ok_or_else(|| {
            eyre!(
                "Task '{}' extends template '{}' which was not found. \
                 Available templates: {}",
                task.name,
                template_name,
                if templates.is_empty() {
                    "(none)".to_string()
                } else {
                    templates.keys().join(", ")
                }
            )
        })?;

        task.merge_template(template);
    }
    Ok(())
}

fn default_task_includes() -> Vec<String> {
    vec![
        "mise-tasks".to_string(),
        ".mise-tasks".to_string(),
        ".mise/tasks".to_string(),
        ".config/mise/tasks".to_string(),
        "mise/tasks".to_string(),
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
    if !new_versions.is_empty() {
        measure!("auto-locking platforms", {
            if let Err(e) = lockfile::auto_lock_new_versions(config, new_versions).await {
                warn!("failed to auto-lock platforms for new versions: {e}");
            }
        });
    }

    Ok(())
}

fn prefix_monorepo_task_names(tasks: &mut [Task], dir: &Path, monorepo_root: &Path) {
    const MONOREPO_PATH_PREFIX: &str = "//";
    const MONOREPO_TASK_SEPARATOR: &str = ":";

    if let Ok(rel_path) = dir.strip_prefix(monorepo_root) {
        let prefix = rel_path
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        for task in tasks.iter_mut() {
            task.name = format!(
                "{}{}{}{}",
                MONOREPO_PATH_PREFIX, prefix, MONOREPO_TASK_SEPARATOR, task.name
            );
        }
    }
}

async fn load_local_tasks_with_context(
    config: &Arc<Config>,
    ctx: Option<&crate::task::TaskLoadContext>,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<Vec<Task>> {
    let mut tasks = vec![];
    let monorepo_config = find_monorepo_config(&config.config_files);
    let monorepo_root = monorepo_config.and_then(|cf| cf.project_root().map(|p| p.to_path_buf()));

    // Load tasks from parent directories (current working directory up to root)

    let local_config_files = config
        .config_files
        .iter()
        .filter(|(_, cf)| !is_global_config(cf.get_path()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect::<IndexMap<_, _>>();
    for d in all_dirs()? {
        if cfg!(test) && !d.starts_with(*dirs::HOME) {
            continue;
        }
        let mut dir_tasks = load_tasks_in_dir(config, &d, &local_config_files, templates).await?;

        if let Some(ref monorepo_root) = monorepo_root {
            prefix_monorepo_task_names(&mut dir_tasks, &d, monorepo_root);
        }

        tasks.extend(dir_tasks);
    }

    // Determine if we should load monorepo tasks from subdirectories
    // We should load subdirs if:
    // 1. load_all is true (--all flag or wildcard patterns like //...:task)
    // 2. OR we have specific path_hints (patterns like //foo/bar:task)
    let should_load_subdirs = ctx.is_some_and(|c| c.load_all || !c.path_hints.is_empty());

    // If in a monorepo, also discover and load tasks from subdirectories
    if let Some(monorepo_root) = &monorepo_root {
        // By default, only load tasks from current directory hierarchy (already loaded above)
        // With --all flag or path hints, also load tasks from matching subdirectories
        if !should_load_subdirs {
            // Default: don't load any additional monorepo subdirs (they're not in our hierarchy)
            return Ok(tasks);
        }

        // Get config_roots from [monorepo] section if defined
        let config_roots = monorepo_config
            .and_then(|cf| cf.monorepo())
            .map(|m| &m.config_roots);
        let subdirs = discover_monorepo_subdirs(monorepo_root, config_roots, ctx)?;

        // Load tasks from subdirectories in parallel
        let subdir_tasks_futures: Vec<_> = subdirs
            .into_iter()
            .filter(|subdir| !cfg!(test) || subdir.starts_with(*dirs::HOME))
            .map(|subdir| {
                let config = config.clone();
                let monorepo_root = monorepo_root.clone();
                let templates = templates.clone();
                async move {
                    // Use IndexMap to deduplicate tasks by name within this subdirectory
                    // Later inserts win, so file tasks override config tasks with the same name
                    let mut task_map: IndexMap<String, Task> = IndexMap::new();

                    // Load config files from subdirectory
                    // Use .rev() so later files (like mise.local.toml) have higher precedence
                    // Use glob() with .rev() for conf.d patterns so later files (02-override.toml) override earlier ones
                    let config_paths: Vec<PathBuf> = DEFAULT_CONFIG_FILENAMES
                        .iter()
                        .rev()
                        .flat_map(|f| {
                            if f.contains('*') {
                                glob(&subdir, f).unwrap_or_default().into_iter().rev().collect()
                            } else {
                                let path = subdir.join(f);
                                if path.exists() {
                                    vec![path]
                                } else {
                                    vec![]
                                }
                            }
                        })
                        .collect();

                    // Deduplicate config paths while preserving precedence order
                    let mut seen = std::collections::HashSet::new();
                    let config_paths: Vec<PathBuf> = config_paths
                        .into_iter()
                        .filter(|p| seen.insert(p.clone()))
                        .collect();

                    let found_config = !config_paths.is_empty();
                    for config_path in config_paths {
                        match config_file::parse(&config_path).await {
                            Ok(cf) => {
                                let mut subdir_tasks =
                                    load_config_and_file_tasks(&config, cf.clone(), &templates).await?;

                                prefix_monorepo_task_names(&mut subdir_tasks, &subdir, &monorepo_root);
                                for task in subdir_tasks.iter_mut() {
                                    // Store reference to config file for later use
                                    task.cf = Some(cf.clone());
                                }

                                // Add tasks to map - later tasks override earlier ones with same name
                                for task in subdir_tasks {
                                    task_map.insert(task.name.clone(), task);
                                }
                            }
                            Err(err) => {
                                let rel_path = subdir
                                    .strip_prefix(&monorepo_root)
                                    .unwrap_or(&subdir);
                                warn!(
                                    "Failed to parse config file {} in monorepo subdirectory {}: {}. Tasks from this directory will not be loaded.",
                                    config_path.display(),
                                    rel_path.display(),
                                    err
                                );
                            }
                        }
                    }

                    // If no config file exists, still load default task include dirs
                    if !found_config {
                        let includes = task_includes_for_dir(&subdir, &config.config_files);
                        for include in includes {
                            let mut subdir_tasks =
                                load_tasks_includes(&config, &include, &subdir).await?;
                            prefix_monorepo_task_names(&mut subdir_tasks, &subdir, &monorepo_root);
                            for task in subdir_tasks {
                                task_map.insert(task.name.clone(), task);
                            }
                        }
                    }

                    Ok::<Vec<Task>, eyre::Report>(task_map.into_values().collect())
                }
            })
            .collect();

        // Wait for all subdirectory tasks to load
        use tokio::task::JoinSet;
        let mut join_set = JoinSet::new();
        for future in subdir_tasks_futures {
            join_set.spawn(future);
        }

        while let Some(result) = join_set.join_next().await {
            tasks.extend(result??);
        }
    }

    Ok(tasks)
}

/// Expand [monorepo].config_roots patterns to actual directories.
/// Supports explicit paths and single-level globs (*).
/// Recursive globs (**) are not supported.
fn expand_config_roots(
    root: &Path,
    patterns: &[String],
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<Vec<PathBuf>> {
    let mut subdirs = Vec::new();

    for pattern in patterns {
        // Reject absolute paths and parent directory escapes
        if pattern.starts_with('/') || pattern.starts_with("..") || pattern.contains("/../") {
            warn!(
                "[monorepo] config_roots: '{}' must be a relative path within the monorepo",
                pattern
            );
            continue;
        }

        // Reject recursive glob patterns (**)
        if pattern.contains("**") {
            warn!(
                "[monorepo] config_roots: recursive glob '**' not supported in '{}', use single-level '*' instead",
                pattern
            );
            continue;
        }

        if pattern.contains('*') {
            // Single-level glob expansion
            let full_pattern = root.join(pattern);
            match glob::glob(&full_pattern.to_string_lossy()) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(path) => {
                                // Verify path is within monorepo root
                                if path.strip_prefix(root).is_err() {
                                    warn!(
                                        "[monorepo] config_roots: glob matched path outside monorepo root: {}",
                                        path.display()
                                    );
                                    continue;
                                }
                                if path.is_dir() && has_mise_config(&path) {
                                    subdirs.push(path);
                                }
                            }
                            Err(e) => {
                                warn!("[monorepo] config_roots glob error: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("[monorepo] config_roots invalid glob pattern '{pattern}': {e}");
                }
            }
        } else {
            // Explicit path
            let path = root.join(pattern);
            // Verify path is within monorepo root after resolution
            if let Ok(canonical) = path.canonicalize()
                && let Ok(canonical_root) = root.canonicalize()
                && !canonical.starts_with(&canonical_root)
            {
                warn!(
                    "[monorepo] config_roots: '{}' resolves outside monorepo root",
                    pattern
                );
                continue;
            }
            if path.is_dir() {
                if has_mise_config(&path) {
                    subdirs.push(path);
                } else {
                    warn!(
                        "[monorepo] config_roots: '{}' has no mise config file",
                        pattern
                    );
                }
            } else {
                warn!("[monorepo] config_roots: '{}' does not exist", pattern);
            }
        }
    }

    // Apply TaskLoadContext filtering if provided
    if let Some(ctx) = ctx {
        subdirs.retain(|dir| {
            let rel_path = dir
                .strip_prefix(root)
                .ok()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            ctx.should_load_subdir(rel_path, root.to_str().unwrap_or(""))
        });
    }

    Ok(subdirs)
}

/// Check if a directory contains a mise config file or file tasks directory
fn has_mise_config(dir: &Path) -> bool {
    DEFAULT_CONFIG_FILENAMES
        .iter()
        .any(|f| dir.join(f).exists())
        || dir.join(".mise/tasks").is_dir()
        || dir.join("mise-tasks").is_dir()
}

fn discover_monorepo_subdirs(
    root: &Path,
    config_roots: Option<&Vec<String>>,
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<Vec<PathBuf>> {
    // If [monorepo].config_roots is defined, use explicit paths instead of walking
    if let Some(patterns) = config_roots
        && !patterns.is_empty()
    {
        return expand_config_roots(root, patterns, ctx);
    }

    // Fall back to filesystem walking (deprecated)
    deprecated!(
        "monorepo_auto_discovery",
        "Automatic monorepo discovery is deprecated. \
         Please define [monorepo].config_roots in your root mise.toml. \
         See https://mise.jdx.dev/tasks/monorepo.html#explicit-config-roots"
    );
    const DEFAULT_IGNORED_DIRS: &[&str] = &["node_modules", "target", "dist", "build"];
    let has_task_includes = |dir: &Path| {
        default_task_includes()
            .into_iter()
            .any(|include| dir.join(include).exists())
    };

    let mut subdirs = Vec::new();
    let settings = Settings::get();
    let respect_gitignore = settings.task.monorepo_respect_gitignore;
    let max_depth = settings.task.monorepo_depth as usize;

    // Build the list of excluded directories
    // If user defined custom exclude dirs, use only those, otherwise use defaults
    let excluded_dirs: Vec<&str> = if settings.task.monorepo_exclude_dirs.is_empty() {
        DEFAULT_IGNORED_DIRS.to_vec()
    } else {
        settings
            .task
            .monorepo_exclude_dirs
            .iter()
            .map(|s| s.as_str())
            .collect()
    };

    if respect_gitignore {
        // Use the `ignore` crate which respects .gitignore files
        let walker = ignore::WalkBuilder::new(root)
            .max_depth(Some(max_depth))
            .hidden(true) // Skip hidden files/dirs
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global .gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .require_git(false) // Don't require a git repo
            .build();

        for entry in walker {
            let entry = entry?;
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                let dir = entry.path();

                // Skip if depth is 0 (root itself)
                if dir == root {
                    continue;
                }

                // Check against excluded directories
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if excluded_dirs.contains(&name) {
                    continue;
                }

                // Check if this directory has a mise config file
                let has_config = DEFAULT_CONFIG_FILENAMES
                    .iter()
                    .any(|f| dir.join(f).exists());
                let has_task_includes = has_task_includes(dir);
                if has_config || has_task_includes {
                    // Apply context filtering if provided
                    if let Some(ctx) = ctx {
                        let rel_path = dir
                            .strip_prefix(root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .unwrap_or("");
                        if ctx.should_load_subdir(rel_path, root.to_str().unwrap_or("")) {
                            subdirs.push(dir.to_path_buf());
                        }
                    } else {
                        subdirs.push(dir.to_path_buf());
                    }
                }
            }
        }
    } else {
        // Fall back to WalkDir for non-gitignore-aware walking
        for entry in WalkDir::new(root)
            .min_depth(1)
            .max_depth(max_depth)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories and excluded patterns
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && !excluded_dirs.contains(&name.as_ref())
            })
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                let dir = entry.path();
                // Check if this directory has a mise config file
                let has_config = DEFAULT_CONFIG_FILENAMES
                    .iter()
                    .any(|f| dir.join(f).exists());
                let has_task_includes = has_task_includes(dir);
                if has_config || has_task_includes {
                    // Apply context filtering if provided
                    if let Some(ctx) = ctx {
                        let rel_path = dir
                            .strip_prefix(root)
                            .ok()
                            .and_then(|p| p.to_str())
                            .unwrap_or("");
                        if ctx.should_load_subdir(rel_path, root.to_str().unwrap_or("")) {
                            subdirs.push(dir.to_path_buf());
                        }
                    } else {
                        subdirs.push(dir.to_path_buf());
                    }
                }
            }
        }
    }

    Ok(subdirs)
}

async fn load_global_tasks(
    config: &Arc<Config>,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<Vec<Task>> {
    let config_files = config
        .config_files
        .values()
        .filter(|cf| is_global_config(cf.get_path()))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    for cf in config_files {
        tasks.extend(load_config_and_file_tasks(config, cf.clone(), templates).await?);
    }
    Ok(tasks)
}

async fn load_config_and_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<Vec<Task>> {
    let config_root = cf.config_root();
    let tasks = load_config_tasks(config, cf.clone(), &config_root, templates).await?;
    let file_tasks = load_file_tasks(config, cf.clone(), &config_root).await?;
    Ok(tasks.into_iter().chain(file_tasks).collect())
}

async fn load_config_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
    templates: &IndexMap<String, TaskTemplate>,
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
        // Resolve template if the task extends one
        resolve_task_template(&mut t, templates)?;
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
    if root.is_file() && root.extension().map(|e| e == "toml").unwrap_or(false) {
        load_task_file(config, root, config_root).await
    } else if root.is_dir() {
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
                    .task
                    .disable_paths
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
    } else {
        Ok(vec![])
    }
}

async fn resolve_git_url_to_path(git_url: &str) -> Result<PathBuf> {
    let no_cache = Settings::get().task.remote_no_cache.unwrap_or(false);
    let task_file_providers = TaskFileProvidersBuilder::new()
        .with_cache(!no_cache)
        .build();

    match task_file_providers.get_provider(git_url) {
        Some(provider) => provider.get_local_path(git_url).await,
        None => bail!("No provider found for git URL: {}", git_url),
    }
}

/// Check if a pattern contains glob metacharacters
fn is_glob_pattern(pattern: &str) -> bool {
    // Check for unescaped glob metacharacters: *, ?, [, ], {, }
    // Note: This is a simple check that may have false positives with escaped chars,
    // but glob() will handle those correctly
    pattern.contains('*')
        || pattern.contains('?')
        || pattern.contains('[')
        || pattern.contains(']')
        || pattern.contains('{')
        || pattern.contains('}')
}

/// Expand a task include pattern (which may be a glob) to a list of paths
fn expand_task_include(dir: &Path, pattern: &str) -> Vec<PathBuf> {
    if is_glob_pattern(pattern) {
        match glob(dir, pattern) {
            Ok(paths) => paths,
            Err(err) => {
                warn!(
                    "failed to expand glob pattern '{}' in '{}': {}",
                    pattern,
                    display_path(dir),
                    err
                );
                vec![]
            }
        }
    } else {
        // Literal path
        let path = PathBuf::from(pattern);
        let resolved = if path.is_absolute() {
            path
        } else {
            dir.join(path)
        };
        if resolved.exists() {
            vec![resolved]
        } else {
            vec![]
        }
    }
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
        .unwrap_or_else(default_task_includes);

    let mut tasks = vec![];
    let config_root = Arc::new(config_root.to_path_buf());
    let cf_root = cf.config_root();

    for include in includes {
        let paths = if include.starts_with("git::") {
            vec![resolve_git_url_to_path(&include).await?]
        } else {
            expand_task_include(&cf_root, &include)
        };
        for path in paths {
            tasks.extend(load_tasks_includes(config, &path, &config_root).await?);
        }
    }
    Ok(tasks)
}

pub fn task_includes_for_dir(dir: &Path, config_files: &ConfigMap) -> Vec<PathBuf> {
    let configs = configs_at_root(dir, config_files);

    // Find the first config that has explicit task_config.includes
    // and resolve paths relative to that config file's directory
    let (includes, resolve_dir) = configs
        .iter()
        .rev()
        .find_map(|cf| {
            cf.task_config().includes.clone().map(|includes| {
                // Resolve relative paths from the config root, not the config file's directory
                (includes, cf.config_root())
            })
        })
        .unwrap_or_else(|| {
            // Default includes should be resolved relative to the search directory
            (default_task_includes(), dir.to_path_buf())
        });

    includes
        .into_iter()
        .flat_map(|p| {
            // Git URLs are handled by load_file_tasks, not here
            if p.starts_with("git::") {
                return vec![];
            }
            expand_task_include(&resolve_dir, &p)
        })
        .unique()
        .collect::<Vec<_>>()
}

pub async fn load_tasks_in_dir(
    config: &Arc<Config>,
    dir: &Path,
    config_files: &ConfigMap,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<Vec<Task>> {
    let configs = configs_at_root(dir, config_files);

    let git_includes: Vec<String> = configs
        .iter()
        .rev()
        .find_map(|cf| cf.task_config().includes.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.starts_with("git::"))
        .collect();

    let mut config_tasks = vec![];
    for cf in &configs {
        let dir = dir.to_path_buf();
        config_tasks.extend(load_config_tasks(config, (*cf).clone(), &dir, templates).await?);
    }

    let mut file_tasks = vec![];
    for p in task_includes_for_dir(dir, config_files) {
        file_tasks.extend(load_tasks_includes(config, &p, dir).await?);
    }

    for include in git_includes {
        let resolved = resolve_git_url_to_path(&include).await?;
        file_tasks.extend(load_tasks_includes(config, &resolved, dir).await?);
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
