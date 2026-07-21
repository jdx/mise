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
use crate::cli::args::{BackendArg, split_bracketed_opts};
use crate::cli::version;
use crate::config::config_file::idiomatic_version::IdiomaticVersionFile;
use crate::config::config_file::min_version::MinVersionSpec;
use crate::config::config_file::mise_toml::{MiseToml, Tasks};
use crate::config::config_file::{ConfigFile, config_trust_root, is_path_trusted, trust_check};
use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
use crate::config::tracking::Tracker;
use crate::env::{MISE_DEFAULT_CONFIG_FILENAME, MISE_DEFAULT_TOOL_VERSIONS_FILENAME};
use crate::file::display_path;
use crate::shorthands::{Shorthands, get_shorthands};
use crate::task::task_file_providers::TaskFileProvidersBuilder;
use crate::task::{Task, TaskTemplate, strip_extension};
use crate::tera::{contains_template_syntax, render_str, take_tera_accessed_files};
use crate::toolset::env_cache::{CachedNonToolEnv, compute_settings_hash, get_file_mtime};
use crate::toolset::{
    ResolvedToolOptions, ToolOptionSource, ToolOptions, ToolRequestSet, ToolRequestSetBuilder,
    ToolVersion, ToolVersionOptions, Toolset, install_state,
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

pub(crate) struct MonorepoUnion {
    pub config_files: ConfigMap,
    pub tool_request_set: ToolRequestSet,
    pub repo_urls: HashMap<String, String>,
}

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
    vars_results: OnceCell<EnvResults>,
}

#[derive(Debug, Clone, Default)]
pub struct Alias {
    pub backend: Option<String>,
    pub versions: IndexMap<String, String>,
}

static _CONFIG: RwLock<Option<Arc<Config>>> = RwLock::new(None);
static _REDACTOR: Lazy<Mutex<Redactor>> = Lazy::new(Default::default);
const MONOREPO_LOCKFILE_WARN_AT: &str = "2026.12.0";
const MONOREPO_LOCKFILE_DEFAULT_AT: &str = "2027.6.0";

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
                crate::task::reset();
                Ok(())
            },
            Duration::from_secs(5),
        )
        .await?;
        Settings::reload();
        Config::load().await
    }

    pub(crate) fn with_config_files(&self, config_files: ConfigMap) -> Arc<Self> {
        let project_root = get_project_root(&config_files).or_else(|| self.project_root.clone());
        let repo_urls = load_plugins(&config_files).unwrap_or_else(|_| self.repo_urls.clone());
        Arc::new(Self {
            tera_ctx: self.tera_ctx.clone(),
            config_files,
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: self.shorthands.clone(),
            hooks: OnceCell::new(),
            tasks_cache: Arc::new(DashMap::new()),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases: self.all_aliases.clone(),
            aliases: self.aliases.clone(),
            project_root,
            repo_urls,
            shell_aliases: self.shell_aliases.clone(),
            tera_files: self.tera_files.clone(),
            vars: self.vars.clone(),
            vars_results: OnceCell::new(),
        })
    }

    pub(crate) fn with_tool_request_set(&self, tool_request_set: ToolRequestSet) -> Arc<Self> {
        let config = self.with_config_files(self.config_files.clone());
        config.tool_request_set.set(tool_request_set).unwrap();
        config
    }

    #[async_backtrace::framed]
    pub async fn load() -> Result<Arc<Self>> {
        backend::load_tools().await?;
        let idiomatic_files = measure!("config::load idiomatic_files", {
            load_idiomatic_filenames().await
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
            vars_results: OnceCell::new(),
        });
        let vars_results = measure!("config::load vars_results", {
            let results = load_vars(&vars_config).await?;
            config.vars_results.set(results.clone()).ok();
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
                config
                    .redaction_keys()
                    .into_iter()
                    .chain(vars_results.redactions.iter().cloned()),
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

        warn_if_auto_env_files_exist();
        warn_if_monorepo_lockfile_default_changes(&config);

        time!("load done");

        measure!("config::load install_state", {
            for plugin in config.repo_urls.keys() {
                let (plugin_type, plugin) = PluginType::from_plugin_config(plugin);
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
            .get_or_try_init(async || Ok(self.env_results().await?.env.clone()))
            .await
    }
    pub async fn env_results(self: &Arc<Self>) -> Result<&EnvResults> {
        self.env
            .get_or_try_init(|| async { self.load_env().await })
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

    fn has_tool_alias(&self, short: &str) -> bool {
        self.all_aliases
            .get(short)
            .is_some_and(|alias| alias.backend.is_some())
            || self.repo_urls.contains_key(short)
    }

    pub async fn get_tool_opts_with_overrides(
        self: &Arc<Self>,
        backend_arg: &Arc<BackendArg>,
    ) -> Result<ToolOptions> {
        Ok(self
            .resolve_tool_opts_with_overrides(backend_arg)
            .await?
            .into_options())
    }

    pub async fn resolve_tool_opts_with_overrides(
        self: &Arc<Self>,
        backend_arg: &Arc<BackendArg>,
    ) -> Result<ResolvedToolOptions> {
        let trs = self.get_tool_request_set().await?;
        let short_match = trs.iter().find(|tr| tr.0.short == backend_arg.short);
        let tool_request = short_match.or_else(|| {
            if !self.has_tool_alias(&backend_arg.short) {
                return None;
            }

            let resolved_ba = BackendArg::new(backend_arg.full(), None);
            trs.iter().find(|tr| tr.0.short == resolved_ba.short)
        });
        let config_opts = tool_request.and_then(|tr| tr.1.first().map(|req| req.options()));
        let alias_opts = self.get_backend_alias_opts(backend_arg);
        let mut resolved = ResolvedToolOptions::default();
        resolved.apply_overrides(&backend_arg.registry_opts(), ToolOptionSource::Registry);
        if let Some(manifest_opts) = backend_arg.install_manifest_opts() {
            resolved.apply_overrides(manifest_opts, ToolOptionSource::InstallManifest);
        }
        if alias_opts.is_none()
            && let Some(full_opts) = backend_arg.resolved_full_opts()
        {
            resolved.apply_overrides(&full_opts, ToolOptionSource::BackendAlias);
        }
        if let Some(alias_opts) = alias_opts {
            resolved.apply_overrides(&alias_opts, ToolOptionSource::BackendAlias);
        }
        if let Some(config_opts) = config_opts {
            resolved.apply_overrides(&config_opts, ToolOptionSource::Config);
        }
        if let Some(inline_opts) = backend_arg.explicit_opts() {
            resolved.apply_overrides(inline_opts, ToolOptionSource::InlineBackendArg);
        }
        Ok(resolved)
    }

    fn get_backend_alias_opts(&self, backend_arg: &BackendArg) -> Option<ToolVersionOptions> {
        if backend_arg.has_env_backend_override() {
            return None;
        }
        let short = backend::unalias_backend(&backend_arg.short);
        self.all_aliases
            .get(short)
            .and_then(|alias| alias.backend.as_deref())
            .and_then(|backend| split_bracketed_opts(backend).map(|(_, opts)| opts))
            .map(crate::toolset::parse_tool_options)
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
                if registry::url_like(plugin_name) || plugin_name.split('/').count() == 2 {
                    Some(registry::full_to_url(plugin_name))
                } else {
                    None
                }
            })
    }

    pub fn is_monorepo(&self) -> bool {
        find_monorepo_root(&self.config_files).is_some()
    }

    pub fn monorepo_root(&self) -> Option<PathBuf> {
        find_monorepo_root(&self.config_files)
    }

    /// Returns the root lockfile directory when unified monorepo lockfiles are active.
    ///
    /// Lockfile discovery is intentionally lenient: it only requires
    /// `[monorepo].config_roots` to match directories, because legacy lockfiles
    /// can exist in roots whose live config is idiomatic-only or was removed.
    pub fn monorepo_lockfile_root(&self) -> Option<PathBuf> {
        let cf = find_monorepo_config(&self.config_files)?;
        let setting = cf.monorepo().and_then(|m| m.lockfile);
        if !monorepo_lockfile_enabled_for_version(&version::V, setting) {
            return None;
        }
        let monorepo_root = cf.project_root().map(|p| p.to_path_buf())?;
        match self.monorepo_config_root_dirs(None) {
            Ok(config_roots) if !config_roots.is_empty() => Some(monorepo_root),
            Ok(_) => {
                if setting == Some(true) {
                    warn_once!(
                        "[monorepo] lockfile = true is set, but [monorepo].config_roots did not match any directories; using root lockfiles without migration"
                    );
                    Some(monorepo_root)
                } else {
                    None
                }
            }
            Err(err) => {
                if setting == Some(true) {
                    warn_once!(
                        "[monorepo] lockfile = true is set, but [monorepo].config_roots could not be resolved: {err:#}; using root lockfiles without migration"
                    );
                    Some(monorepo_root)
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn monorepo_config_root_dirs_for_lockfiles(&self) -> Result<Vec<PathBuf>> {
        self.monorepo_config_root_dirs(None)
    }

    fn monorepo_config_root_dirs_with_filenames(
        &self,
        filenames: &[String],
    ) -> Result<Vec<PathBuf>> {
        self.monorepo_config_root_dirs(Some(filenames))
    }

    /// Resolve `[monorepo].config_roots`.
    ///
    /// `None` matches any existing directory and is used for lockfile migration
    /// and routing. `Some(filenames)` requires a recognized config or idiomatic
    /// version file and is used for full monorepo union/task loading.
    fn monorepo_config_root_dirs(&self, filenames: Option<&[String]>) -> Result<Vec<PathBuf>> {
        let monorepo_config = find_monorepo_config(&self.config_files)
            .ok_or_else(|| eyre!("no config file in scope sets monorepo_root = true"))?;
        let monorepo_root = monorepo_config
            .project_root()
            .ok_or_else(|| eyre!("monorepo root config has no project root"))?;
        let patterns = &monorepo_config
            .monorepo()
            .ok_or_else(|| eyre!("[monorepo].config_roots is required for monorepo operations"))?
            .config_roots;
        if patterns.is_empty() {
            bail!("[monorepo].config_roots is required for monorepo operations");
        }
        let roots = match filenames {
            Some(filenames) => {
                expand_config_roots_with_filenames(&monorepo_root, patterns, None, filenames)?
            }
            None => expand_config_root_dirs(&monorepo_root, patterns, None)?,
        };
        if roots.is_empty() {
            bail!("[monorepo].config_roots did not match any config roots");
        }
        Ok(roots)
    }

    pub async fn monorepo_union_tool_request_set(self: &Arc<Self>) -> Result<ToolRequestSet> {
        Ok(self.monorepo_union().await?.tool_request_set)
    }

    pub(crate) async fn monorepo_union(self: &Arc<Self>) -> Result<MonorepoUnion> {
        let idiomatic_filenames = load_idiomatic_filenames().await;
        let config_filenames = idiomatic_filenames
            .keys()
            .chain(DEFAULT_CONFIG_FILENAMES.iter())
            .cloned()
            .collect_vec();
        let roots = self.monorepo_config_root_dirs_with_filenames(&config_filenames)?;
        let mut config_files = self.config_files.clone();
        let mut base_config_files = self.config_files.clone();
        base_config_files.retain(|path, _| {
            is_global_config(path) || !roots.iter().any(|root| path.starts_with(root))
        });

        let mut union = ToolRequestSet::new();
        for root in roots {
            let root_paths = config_paths_in_dir_with_filenames(&root, &config_filenames);
            let mut root_config_files =
                load_config_files_from_paths(&root_paths, &idiomatic_filenames).await?;
            for (path, cf) in root_config_files.clone() {
                config_files.entry(path).or_insert(cf);
            }
            for (path, cf) in base_config_files.clone() {
                root_config_files.entry(path).or_insert(cf);
            }

            let root_trs = ToolRequestSetBuilder::new()
                .with_config_files(root_config_files)
                .without_runtime_args()
                .build(self)
                .await?;
            union.unknown_tools.extend(root_trs.unknown_tools.clone());
            for (_ba, requests, source) in root_trs.iter() {
                for request in requests {
                    let already_present = union.tools.get(request.ba()).is_some_and(|existing| {
                        existing.iter().any(|r| {
                            r.version() == request.version() && r.options() == request.options()
                        })
                    });
                    if !already_present {
                        union.add_version(request.clone(), source);
                    }
                }
            }
        }

        union.unknown_tools = union.unknown_tools.into_iter().unique().collect();
        let repo_urls = load_plugins(&config_files)?;
        Ok(MonorepoUnion {
            config_files,
            tool_request_set: union,
            repo_urls,
        })
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
            .values()
            .flat_map(|t| {
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

        let templates = collect_task_templates(&config.config_files);

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
            if !config_file::is_trusted(&trust_root)
                && !config_file::is_trusted(&path)
                // safe mise.toml files load without a trust marker, so a missing
                // marker doesn't mean they should be skipped here
                && !MiseToml::path_is_trust_exempt(&path)
            {
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
        let mut env_results = EnvResults::resolve(
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
        for env_file in Settings::get().env_files() {
            if env_results.env_files.contains(&env_file) {
                continue;
            }
            debug!("env_file: {}", display_path(&env_file));
            match dotenvy::from_path_iter(&env_file) {
                Ok(iter) => {
                    env_results.env_files.push(env_file.clone());
                    for item in iter {
                        match item {
                            Ok((k, v)) => {
                                env_results.env.insert(k, (v, env_file.clone()));
                            }
                            Err(err) => warn!("env_file: {err}"),
                        }
                    }
                }
                Err(err) => trace!("env_file: {err}"),
            }
        }
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
                    let lockfile = parent.join("mise.lock");

                    // Only watch lockfiles that currently exist to prevent missing optional
                    // mise.lock files from keeping hook-env from stabilizing. If one is created
                    // later, should_exit_early_fast() will notice the parent directory mtime
                    // change, force a slow-path run, and this watch set will then include the new
                    // lockfile on that recomputation.
                    if lockfile.exists() {
                        watch_files.push(lockfile.into());
                    }
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
            .chain(env_results.watch_files.iter().map(|p| p.as_path().into()))
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
    // Highest precedence config files are returned first.
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

/// Find the config file that has monorepo_root = true
fn find_monorepo_config(config_files: &ConfigMap) -> Option<&Arc<dyn ConfigFile>> {
    config_files
        .values()
        .find(|cf| cf.monorepo_root() == Some(true))
}

async fn load_idiomatic_filenames() -> BTreeMap<String, Vec<String>> {
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
/// Config filename patterns for a single MISE_ENV environment, in precedence order
/// (later wins, matching LOCAL_CONFIG_FILENAMES ordering)
fn env_config_patterns(env: &str) -> Vec<String> {
    vec![
        format!(".config/mise/config.{env}.toml"),
        format!(".config/mise.{env}.toml"),
        format!("mise/config.{env}.toml"),
        format!("mise.{env}.toml"),
        format!(".mise/config.{env}.toml"),
        format!(".mise.{env}.toml"),
        format!(".config/mise/config.{env}.local.toml"),
        format!(".config/mise.{env}.local.toml"),
        format!("mise/config.{env}.local.toml"),
        format!("mise.{env}.local.toml"),
        format!(".mise/config.{env}.local.toml"),
        format!(".mise.{env}.local.toml"),
    ]
}

pub static DEFAULT_CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    let mut filenames = LOCAL_CONFIG_FILENAMES
        .iter()
        .map(|f| f.to_string())
        .collect_vec();
    for env in &*env::MISE_ENV_WITH_AUTO {
        filenames.extend(env_config_patterns(env));
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
static TOML_CONFIG_MATCHERS: Lazy<Vec<globset::GlobMatcher>> = Lazy::new(|| {
    TOML_CONFIG_FILENAMES
        .iter()
        .filter_map(|pattern| {
            globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
                .map_err(|e| warn!("failed to compile config glob pattern {pattern}: {e}"))
                .ok()
                .map(|glob| glob.compile_matcher())
        })
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
        .filter(|p| {
            let ctr = config_trust_root(p);
            // The `ignored_config_paths` setting is a hard block; the persisted
            // ignore list is overridden by `trusted_config_paths`, matching
            // is_trusted so a settings-trusted config is not reported as ignored.
            if config_file::is_ignored_via_setting(&ctr) || config_file::is_ignored_via_setting(p) {
                return true;
            }
            (config_file::is_persisted_ignored(&ctr) || config_file::is_persisted_ignored(p))
                && !config_file::is_trusted_via_config_paths(p)
        })
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

pub(crate) fn config_paths_in_dir(dir: &Path) -> Vec<PathBuf> {
    config_paths_in_dir_with_filenames(dir, &DEFAULT_CONFIG_FILENAMES)
}

fn config_paths_in_dir_with_filenames(dir: &Path, filenames: &[String]) -> Vec<PathBuf> {
    let config_paths: Vec<PathBuf> = filenames
        .iter()
        .rev()
        .flat_map(|f| {
            if f.contains('*') {
                glob(dir, f).unwrap_or_default().into_iter().rev().collect()
            } else {
                let path = dir.join(f);
                if path.exists() { vec![path] } else { vec![] }
            }
        })
        .collect();

    let mut seen = std::collections::HashSet::new();
    config_paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
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
pub(crate) fn is_tool_versions_file(p: &Path) -> bool {
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
            if config_dir_is_ignored(dir, include_ignored) {
                vec![]
            } else {
                config_paths_in_dir_with_filenames(dir, config_filenames)
            }
        })
        .collect::<Vec<_>>();

    config_files.extend(global_config_files());
    config_files.extend(system_config_files());

    config_files
        .into_iter()
        .unique_by(|p| file::desymlink_path(p))
        .filter(|p| !config_path_is_ignored(p, include_ignored))
        .collect()
}

/// Whether to emit the phase-2 auto_env rollout warning. Pure for unit testing.
/// Warns only when the user hasn't decided (setting unset), auto envs aren't
/// already active, and the mise version is in the warning window that precedes
/// the 2027.6.0 default flip.
fn should_warn_auto_env(
    version: &versions::Versioning,
    setting: Option<bool>,
    auto_envs_active: bool,
) -> bool {
    setting.is_none()
        && !auto_envs_active
        && *version >= versions::Versioning::new("2026.12.0").unwrap()
        && !env::auto_env_default_for_version(version)
}

/// Default for monorepo lockfile routing when `[monorepo].lockfile` is unset.
/// Keep legacy colocated lockfiles until the scheduled default flip.
fn monorepo_lockfile_default_for_version(version: &versions::Versioning) -> bool {
    *version >= versions::Versioning::new(MONOREPO_LOCKFILE_DEFAULT_AT).unwrap()
}

fn monorepo_lockfile_enabled_for_version(
    version: &versions::Versioning,
    setting: Option<bool>,
) -> bool {
    setting.unwrap_or_else(|| monorepo_lockfile_default_for_version(version))
}

/// Whether to emit the phase-2 monorepo lockfile rollout warning. Pure for unit testing.
/// Warns only when the user has not explicitly chosen `lockfile = true` or `false`
/// and the mise version is in the warning window before the default flip.
fn should_warn_monorepo_lockfile_default(
    version: &versions::Versioning,
    setting: Option<bool>,
    lockfile_enabled: bool,
    monorepo_lockfiles_exist: bool,
) -> bool {
    setting.is_none()
        && lockfile_enabled
        && monorepo_lockfiles_exist
        && *version >= versions::Versioning::new(MONOREPO_LOCKFILE_WARN_AT).unwrap()
        && !monorepo_lockfile_default_for_version(version)
}

fn warn_if_monorepo_lockfile_default_changes(config: &Config) {
    // Dead code once the default flips on: unset configs use the new behavior,
    // so this warning path should be removed when the rollout completes.
    debug_assert!(
        !monorepo_lockfile_default_for_version(&version::V),
        "monorepo lockfiles are now default-on; remove warn_if_monorepo_lockfile_default_changes() and should_warn_monorepo_lockfile_default()"
    );
    let Some(cf) = find_monorepo_config(&config.config_files) else {
        return;
    };
    let setting = cf.monorepo().and_then(|m| m.lockfile);
    if !should_warn_monorepo_lockfile_default(
        &version::V,
        setting,
        Settings::get().lockfile_enabled(),
        monorepo_lockfiles_exist(config, cf),
    ) {
        return;
    }

    warn_once!(
        "Monorepo lockfiles will default to a single root lockfile starting in mise {MONOREPO_LOCKFILE_DEFAULT_AT}. \
        Set `[monorepo] lockfile = true` in {} to opt in now, or `lockfile = false` to keep per-subproject lockfiles and silence this warning.",
        display_path(cf.get_path())
    );
}

fn monorepo_lockfiles_exist(config: &Config, monorepo_config: &Arc<dyn ConfigFile>) -> bool {
    let Some(monorepo_root) = monorepo_config.project_root() else {
        return false;
    };
    let mut lockfile_paths = IndexSet::new();

    for (config_path, cf) in &config.config_files {
        if !config_path.starts_with(&monorepo_root) || !cf.source().is_mise_toml() {
            continue;
        }
        lockfile_paths.insert(lockfile::lockfile_path_for_config(config_path, None).0);
        lockfile_paths.insert(
            lockfile::lockfile_path_for_config(config_path, Some(monorepo_root.as_path())).0,
        );
    }

    if let Some(monorepo) = monorepo_config.monorepo()
        && let Ok(config_roots) =
            expand_config_root_dirs(&monorepo_root, &monorepo.config_roots, None)
    {
        for config_root in config_roots {
            for lockfile_path in lockfile::lockfile_variant_paths_in_dir(&config_root) {
                lockfile_paths.insert(lockfile_path);
            }
            for config_path in config_paths_in_dir(&config_root) {
                lockfile_paths.insert(lockfile::lockfile_path_for_config(&config_path, None).0);
                lockfile_paths.insert(
                    lockfile::lockfile_path_for_config(&config_path, Some(monorepo_root.as_path()))
                        .0,
                );
            }
        }
    }

    lockfile_paths.iter().any(|path| path.exists())
}

/// Phase-2 rollout warning for auto_env: starting with 2026.12.0, tell users about
/// platform-specific config files (e.g. mise.windows.toml) that mise will begin
/// loading automatically when auto_env defaults to true in 2027.6.0.
fn warn_if_auto_env_files_exist() {
    // dead code once the default flips on: the files load, so there is nothing to warn about
    debug_assert!(
        !env::auto_env_default_for_version(&version::V),
        "auto_env is now default-on; remove warn_if_auto_env_files_exist() and should_warn_auto_env()"
    );
    if !should_warn_auto_env(
        &version::V,
        env::auto_env_setting(),
        !env::AUTO_ENV_NAMES.is_empty(),
    ) || *env::IS_RUNNING_AS_SHIM
    {
        return;
    }
    // Skip hook-env to keep the every-prompt path free of extra filesystem checks.
    // Match the subcommand anywhere before `--` rather than guessing its positional
    // slot (flags with separate values like `--cd /path` would shift it); the warning
    // is advisory, so erring toward skipping it is the safe direction.
    if env::ARGS
        .read()
        .unwrap()
        .iter()
        .skip(1)
        .take_while(|a| *a != "--")
        .any(|a| a == "hook-env")
    {
        return;
    }
    let found = detect_auto_env_candidate_files();
    if !found.is_empty() {
        warn_once!(
            "Found platform-specific config file(s) that mise will load automatically starting in 2027.6.0: {}. \
            Set MISE_AUTO_ENV=true (or `auto_env = true` in .miserc.toml) to enable this now, \
            or `auto_env = false` to keep the current behavior and silence this warning. \
            See https://mise.jdx.dev/configuration/environments.html#platform-environments",
            found.iter().map(display_path).join(", ")
        );
    }
}

/// Config files that exist on disk and would be loaded if auto_env were enabled,
/// but are not loaded today because their platform env is not in MISE_ENV.
fn detect_auto_env_candidate_files() -> Vec<PathBuf> {
    let candidate_envs = env::platform_env_names()
        .into_iter()
        .filter(|name| !env::MISE_ENV.contains(name))
        .collect_vec();
    // IndexSet: the all_dirs() walk and the global/system dir checks can find the
    // same file (e.g. ~/.config/mise/config.{env}.toml when cwd is under $HOME)
    let mut found = IndexSet::new();
    for dir in all_dirs().unwrap_or_default() {
        if env::MISE_IGNORED_CONFIG_PATHS
            .iter()
            .any(|p| dir.starts_with(p))
        {
            continue;
        }
        for env_name in &candidate_envs {
            for pattern in env_config_patterns(env_name) {
                found.extend(glob(&dir, &pattern).unwrap_or_default());
            }
        }
    }
    for dir in [*dirs::CONFIG, *dirs::SYSTEM_CONFIG] {
        for env_name in &candidate_envs {
            for filename in [
                format!("config.{env_name}.toml"),
                format!("mise.{env_name}.toml"),
                format!("config.{env_name}.local.toml"),
                format!("mise.{env_name}.local.toml"),
            ] {
                let p = dir.join(filename);
                if p.is_file() {
                    found.insert(p);
                }
            }
        }
    }
    // apply the same filters as load_config_paths so we don't warn about files
    // that wouldn't be loaded even with auto_env enabled
    found
        .into_iter()
        .filter(|p| !config_path_is_ignored(p, false))
        .collect()
}

/// Load config hierarchy from a specific directory (for monorepo tasks)
/// This loads all config files from start_dir up through parent directories,
/// including MISE_ENV-specific configs and idiomatic version files.
/// Returns (paths, idiomatic_filenames) so callers can pass the map to
/// load_config_files_from_paths without a redundant second computation.
pub async fn load_config_hierarchy_from_dir(
    start_dir: &Path,
) -> Result<(Vec<PathBuf>, BTreeMap<String, Vec<String>>)> {
    if Settings::no_config() {
        return Ok((vec![], BTreeMap::new()));
    }

    let idiomatic_files = load_idiomatic_filenames().await;
    let config_filenames: Vec<String> = idiomatic_files
        .keys()
        .cloned()
        .chain(DEFAULT_CONFIG_FILENAMES.iter().cloned())
        .collect();

    // Get all directories from start_dir up to root/ceiling
    let dirs = all_dirs_from(start_dir)?;

    let mut config_files = dirs
        .iter()
        .flat_map(|dir| {
            if config_dir_is_ignored(dir, false) {
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
        .filter(|p| !config_path_is_ignored(p, false))
        .collect();

    Ok((paths, idiomatic_files))
}

pub fn is_global_config(path: &Path) -> bool {
    config_set_contains(&global_config_files(), path) || is_system_config(path)
}

pub fn is_system_config(path: &Path) -> bool {
    config_set_contains(&system_config_files(), path)
}

/// Membership test that tolerates symlinked path prefixes (e.g. Fedora Atomic's
/// `/home` -> `/var/home`, where the shell PWD and `$HOME` disagree on the
/// prefix). The fast path is raw equality (no filesystem hit); only on a miss do
/// we canonicalize via [`file::desymlink_path`], matching how `load_config_paths`
/// already dedupes config paths. Without this, a global config discovered via the
/// PWD prefix is not byte-equal to its `$HOME`-derived entry and is wrongly
/// treated as a local config (stripping global-only settings).
fn config_set_contains(set: &IndexSet<PathBuf>, path: &Path) -> bool {
    if set.contains(path) {
        return true;
    }
    let target = file::desymlink_path(path);
    set.iter().any(|p| file::desymlink_path(p) == target)
}

/// Returns true if the path should be filtered out due to MISE_CONFIG_DIR override.
/// When MISE_CONFIG_DIR is set to a non-default location, this filters out configs
/// found under the default location (~/.config/mise) during traversal.
/// See: https://github.com/jdx/mise/discussions/7015
fn is_default_config_dir_override_filtered(path: &Path) -> bool {
    *env::MISE_CONFIG_DIR_OVERRIDDEN
        && !config_set_contains(&global_config_files(), path)
        && path.starts_with(&*env::MISE_DEFAULT_CONFIG_DIR)
}

fn config_dir_is_ignored(dir: &Path, include_ignored: bool) -> bool {
    !include_ignored
        && env::MISE_IGNORED_CONFIG_PATHS
            .iter()
            .any(|p| dir.starts_with(p))
}

fn config_path_is_ignored(path: &Path, include_ignored: bool) -> bool {
    if is_default_config_dir_override_filtered(path) {
        return true;
    }
    if include_ignored {
        return false;
    }
    let ctr = config_trust_root(path);
    // The `ignored_config_paths` setting is a hard filter.
    if config_file::is_ignored_via_setting(&ctr) || config_file::is_ignored_via_setting(path) {
        return true;
    }
    // The persisted ignore list (dismissed prompt / `mise trust --ignore`) is
    // overridden by `trusted_config_paths`, matching is_trusted's precedence so
    // a settings-trusted config is still discovered and loaded.
    if config_file::is_persisted_ignored(&ctr) || config_file::is_persisted_ignored(path) {
        return !config_file::is_trusted_via_config_paths(path);
    }
    false
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
    let config_files = config_files_from_dir(&dirs::SYSTEM_CONFIG);
    *s = Some(config_files.clone());
    config_files
}

static CONFIG_FILENAMES: Lazy<Vec<String>> = Lazy::new(|| {
    let mut filenames = vec!["config.toml".to_string(), "mise.toml".to_string()];
    for env in &*env::MISE_ENV_WITH_AUTO {
        filenames.push(format!("config.{env}.toml"));
        filenames.push(format!("mise.{env}.toml"));
    }
    filenames.push("config.local.toml".to_string());
    filenames.push("mise.local.toml".to_string());
    for env in &*env::MISE_ENV_WITH_AUTO {
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

/// The lowest-precedence TOML config in the nearest local config directory, or
/// the path where it should be written.
pub fn local_toml_config_path() -> PathBuf {
    static CWD: Lazy<PathBuf> = Lazy::new(|| PathBuf::from("."));
    local_toml_config_path_from_dir(dirs::CWD.as_ref().unwrap_or(&CWD))
}

/// The lowest-precedence TOML config in the nearest directory that has one.
///
/// This matches the write-target rule from the docs: choose the highest-precedence
/// directory first, then the lowest-precedence file inside that directory.
pub fn local_toml_config_path_from_dir(cwd: &Path) -> PathBuf {
    if !Settings::no_config() {
        for dir in all_dirs_from(cwd).unwrap_or_default() {
            if config_dir_is_ignored(&dir, false) {
                continue;
            }
            let files = TOML_CONFIG_FILENAMES
                .iter()
                .flat_map(|f| glob(&dir, f).unwrap_or_default())
                .unique_by(|p| file::desymlink_path(p))
                .filter(|p| !config_path_is_ignored(p, false))
                .collect();
            if let Some(cf) = first_config_file(&files)
                && !is_global_config(cf)
            {
                return cf.clone();
            }
        }
    }
    cwd.join(&*env::MISE_DEFAULT_CONFIG_FILENAME)
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
        // For TOML-only commands, choose the nearest config directory and write
        // to its lowest-precedence TOML file.
        Ok(local_toml_config_path_from_dir(&cwd))
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
        if cf.monorepo_root() == Some(true)
            && let Err(err) = config_file::mark_as_monorepo_root(f)
        {
            warn!("failed to mark monorepo root: {err:#}");
        }

        config_map.insert(f.clone(), cf);
    }
    Ok(config_map)
}

/// Load config files from a list of paths (for monorepo task config contexts)
/// Accepts a pre-computed idiomatic filenames map to avoid redundant computation
/// when called after load_config_hierarchy_from_dir.
pub async fn load_config_files_from_paths(
    config_paths: &[PathBuf],
    idiomatic_filenames: &BTreeMap<String, Vec<String>>,
) -> Result<ConfigMap> {
    backend::load_tools().await?;
    let mut config_map = ConfigMap::default();

    for f in config_paths.iter().unique() {
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

pub(crate) async fn resolve_vars_from_config_files(
    config: &Arc<Config>,
    config_files: &ConfigMap,
) -> Result<EnvResults> {
    let entries = config_files
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

    EnvResults::resolve(
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
    .await
}

async fn load_vars(config: &Arc<Config>) -> Result<EnvResults> {
    time!("load_vars start");
    let vars_results = resolve_vars_from_config_files(config, &config.config_files).await?;
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
/// Returns an error if the template is not found.
fn resolve_task_template(
    task: &mut Task,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<()> {
    if let Some(template_name) = &task.extends {
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

fn is_global_task_include_path(path: &Path) -> bool {
    path.starts_with(dirs::CONFIG.join("tasks"))
        || path.starts_with(dirs::SYSTEM_CONFIG.join("tasks"))
}

#[async_backtrace::framed]
pub async fn rebuild_shims_and_runtime_symlinks(
    config: &Arc<Config>,
    ts: &Toolset,
    new_versions: &[ToolVersion],
    lockfile_update_mode: lockfile::LockfileUpdateMode,
) -> Result<()> {
    measure!("rebuilding runtime symlinks", {
        runtime_symlinks::rebuild_for_toolset(config, ts)
            .await
            .wrap_err("failed to rebuild runtime symlinks")?;
    });
    measure!("rebuilding shims", {
        shims::reshim(config, ts, false)
            .await
            .wrap_err("failed to rebuild shims")?;
    });
    lockfile::migrate_monorepo_lockfiles(config)?;
    // Snapshot the lockfiles' platform keys BEFORE update_lockfiles writes
    // current-platform entries — auto-lock uses this to tell a curated lockfile
    // (existing entries are authoritative) from a fresh one (expand to common).
    let pre_install_platforms = if new_versions.is_empty() {
        Default::default()
    } else {
        lockfile::snapshot_pre_install_platforms(config, ts, new_versions)
    };
    measure!("updating lockfiles", {
        lockfile::update_lockfiles(config, ts, new_versions, lockfile_update_mode)
            .wrap_err("failed to update lockfiles")?;
    });
    if !new_versions.is_empty() {
        measure!("auto-locking platforms", {
            lockfile::auto_lock_new_versions(
                config,
                ts,
                new_versions,
                &pre_install_platforms,
                lockfile_update_mode,
            )
            .await
            .wrap_err("failed to auto-lock platforms for new versions")?;
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

                    let config_paths = config_paths_in_dir(&subdir);

                    let found_config = !config_paths.is_empty();
                    for config_path in config_paths {
                        match config_file::parse(&config_path).await {
                            Ok(cf) => {
                                // Pass the owning config file so tasks get `task.cf` set
                                // before templates render — Task::tera_ctx needs it to
                                // resolve vars/env from the subproject's own config
                                // hierarchy rather than the caller's.
                                let mut subdir_tasks = load_config_and_file_tasks(
                                    &config,
                                    cf.clone(),
                                    &templates,
                                    Some(&cf),
                                )
                                .await?;

                                prefix_monorepo_task_names(&mut subdir_tasks, &subdir, &monorepo_root);

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
                        let includes = task_includes_for_dir(&subdir, &config.config_files)?;
                        for include in includes {
                            let mut subdir_tasks = load_tasks_includes(
                                &config, &include, &subdir, &None, &templates, None, true,
                            )
                            .await?;
                            if is_global_task_include_path(&include) {
                                mark_tasks_as_global(&mut subdir_tasks);
                            }
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
    expand_config_roots_with_filenames(root, patterns, ctx, &DEFAULT_CONFIG_FILENAMES)
}

fn expand_config_root_dirs(
    root: &Path,
    patterns: &[String],
    ctx: Option<&crate::task::TaskLoadContext>,
) -> Result<Vec<PathBuf>> {
    expand_config_roots_inner(root, patterns, ctx, None)
}

fn expand_config_roots_with_filenames(
    root: &Path,
    patterns: &[String],
    ctx: Option<&crate::task::TaskLoadContext>,
    filenames: &[String],
) -> Result<Vec<PathBuf>> {
    expand_config_roots_inner(root, patterns, ctx, Some(filenames))
}

fn expand_config_roots_inner(
    root: &Path,
    patterns: &[String],
    ctx: Option<&crate::task::TaskLoadContext>,
    filenames: Option<&[String]>,
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
                                if path.is_dir()
                                    && filenames.is_none_or(|filenames| {
                                        has_mise_config_with_filenames(&path, filenames)
                                    })
                                {
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
                if filenames
                    .is_none_or(|filenames| has_mise_config_with_filenames(&path, filenames))
                {
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

fn has_mise_config_with_filenames(dir: &Path, filenames: &[String]) -> bool {
    filenames.iter().any(|f| {
        if f.contains('*') {
            !glob(dir, f).unwrap_or_default().is_empty()
        } else {
            dir.join(f).exists()
        }
    }) || dir.join(".mise/tasks").is_dir()
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
         See https://mise.en.dev/tasks/monorepo.html#explicit-config-roots"
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
        tasks.extend(load_config_and_file_tasks(config, cf.clone(), templates, None).await?);
    }
    Ok(tasks)
}

/// `monorepo_cf` is the owning config file when loading a monorepo subdirectory
/// outside the current config hierarchy. It is stored on each task as `task.cf`
/// *before* rendering so templates resolve vars/env from the subproject's own
/// config hierarchy, and it makes render errors non-fatal so one broken
/// subproject doesn't break task loading for the whole monorepo.
async fn load_config_and_file_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    templates: &IndexMap<String, TaskTemplate>,
    monorepo_cf: Option<&Arc<dyn ConfigFile>>,
) -> Result<Vec<Task>> {
    let config_root = cf.config_root();
    let config_tasks =
        load_config_tasks(config, cf.clone(), &config_root, templates, monorepo_cf).await?;
    let file_tasks =
        load_file_tasks(config, cf.clone(), &config_root, templates, monorepo_cf).await?;
    Ok(merge_file_and_config_tasks(file_tasks, config_tasks))
}

/// Combine file tasks (auto-discovered executable scripts and included TOML
/// files) with inline `[tasks.*]` blocks.
///
/// `config_tasks` are collected in config-file precedence order (highest first;
/// see [`configs_at_root`]). When a name appears in both a script file task
/// (`file.is_some()`) and an inline block, the script stays as the base and the
/// TOML block is overlaid via [`Task::merge_toml_overlay`]. When the same name
/// appears in multiple inline blocks (e.g. `.config/mise.toml` and
/// `.mise/config.toml`), the first entry wins and later ones are skipped.
/// When the same name appears in more than one file task (e.g. a local
/// `.mise/tasks` script and a same-named task from a `git::` include), the last
/// one wins. Callers load `file_tasks` in declared `task_config.includes`
/// order, so the later include in the list takes precedence — see
/// `load_tasks_in_dir`.
fn merge_file_and_config_tasks(file_tasks: Vec<Task>, config_tasks: Vec<Task>) -> Vec<Task> {
    let mut by_name: IndexMap<String, Task> = IndexMap::new();
    for t in prefer_windows_file_task_siblings(file_tasks) {
        by_name.insert(t.name.clone(), t);
    }
    for t in config_tasks {
        if let Some(existing) = by_name.get_mut(&t.name) {
            if existing.file.is_some() {
                existing.merge_toml_overlay(t);
            }
        } else {
            by_name.insert(t.name.clone(), t);
        }
    }
    by_name.into_values().collect()
}

fn prefer_windows_file_task_siblings(file_tasks: Vec<Task>) -> Vec<Task> {
    if !cfg!(windows) {
        return file_tasks;
    }
    prefer_windows_file_task_siblings_inner(file_tasks)
}

fn prefer_windows_file_task_siblings_inner(file_tasks: Vec<Task>) -> Vec<Task> {
    let windows_exts = Settings::get()
        .windows_executable_extensions
        .iter()
        .map(|ext| ext.to_lowercase())
        .collect::<IndexSet<_>>();
    let extensionless_task_keys = file_tasks
        .iter()
        .filter(|task| task.config_source.extension().is_none())
        .map(|task| (task.config_source.clone(), task.name.clone()))
        .collect::<IndexSet<_>>();
    let mut windows_native_task_key_counts = IndexMap::new();
    for task in &file_tasks {
        let Some(ext) = task
            .config_source
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_lowercase)
        else {
            continue;
        };
        if windows_exts.contains(&ext) {
            *windows_native_task_key_counts
                .entry((
                    task.config_source.with_extension(""),
                    strip_task_extension(&task.name).to_string(),
                ))
                .or_insert(0) += 1;
        }
    }
    let windows_takeover_keys = extensionless_task_keys
        .iter()
        .filter(|key| windows_native_task_key_counts.get(*key) == Some(&1))
        .cloned()
        .collect::<IndexSet<_>>();

    file_tasks
        .into_iter()
        .filter_map(|mut task| {
            if task.config_source.extension().is_none()
                && windows_takeover_keys.contains(&(task.config_source.clone(), task.name.clone()))
            {
                return None;
            }
            if task
                .config_source
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| windows_exts.contains(&ext.to_lowercase()))
            {
                let stem = strip_task_extension(&task.name);
                if windows_takeover_keys
                    .contains(&(task.config_source.with_extension(""), stem.to_string()))
                {
                    task.name = stem.to_string();
                }
            }
            Some(task)
        })
        .collect()
}

fn strip_task_extension(name: &str) -> &str {
    if let Some((prefix, task_part)) = name.rsplit_once(':') {
        let task_without_ext = strip_extension(task_part);
        if task_without_ext == task_part {
            name
        } else {
            &name[..prefix.len() + 1 + task_without_ext.len()]
        }
    } else {
        strip_extension(name)
    }
}

async fn load_config_tasks(
    config: &Arc<Config>,
    cf: Arc<dyn ConfigFile>,
    config_root: &Path,
    templates: &IndexMap<String, TaskTemplate>,
    monorepo_cf: Option<&Arc<dyn ConfigFile>>,
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
        if t.config_root.is_none() {
            t.config_root = Some(config_root.to_path_buf());
        }
        if let Some(monorepo_cf) = monorepo_cf {
            t.cf = Some(monorepo_cf.clone());
        }
        // Resolve template if the task extends one
        resolve_task_template(&mut t, templates)?;
        match t.render(&config, &config_root).await {
            Ok(()) => {
                tasks.push(t);
            }
            Err(e) => {
                if monorepo_cf.is_some() {
                    warn!(
                        "Failed to render task {} in {}: {e:#}. Task will not be available.",
                        t.name,
                        display_path(cf.get_path())
                    );
                } else {
                    return Err(e);
                }
            }
        }
    }
    Ok(tasks)
}

async fn load_tasks_includes(
    config: &Arc<Config>,
    root: &Path,
    config_root: &Path,
    task_config_dir: &Option<String>,
    templates: &IndexMap<String, TaskTemplate>,
    monorepo_cf: Option<&Arc<dyn ConfigFile>>,
    require_trust: bool,
) -> Result<Vec<Task>> {
    if root.is_file() && root.extension().map(|e| e == "toml").unwrap_or(false) {
        trust_check_task_include(root, require_trust)?;
        load_task_file(
            config,
            root,
            config_root,
            task_config_dir,
            templates,
            monorepo_cf,
        )
        .await
    } else if root.is_dir() {
        let all_files = WalkDir::new(root)
            .follow_links(true)
            .into_iter()
            // skip hidden directories (if the root is hidden that's ok)
            .filter_entry(|e| e.path() == root || !e.file_name().to_string_lossy().starts_with('.'))
            .filter_ok(|e| e.file_type().is_file())
            .map_ok(|e| e.path().to_path_buf())
            .try_collect::<_, Vec<PathBuf>, _>()?
            .into_iter()
            .filter(|p| {
                !Settings::get()
                    .task
                    .disable_paths
                    .iter()
                    .any(|d| p.starts_with(d))
            })
            .collect::<Vec<_>>();
        let is_toml = |p: &Path| p.extension().map(|e| e == "toml").unwrap_or(false);
        let (toml_files, exec_files): (Vec<_>, Vec<_>) = all_files
            .into_iter()
            .filter(|p| match is_toml(p) {
                true => !is_mise_config_file_in_task_include(root, p),
                false => file::is_executable(p),
            })
            .partition(|p| is_toml(p));
        let mut tasks = vec![];
        for path in toml_files {
            trust_check_task_include(&path, require_trust)?;
            tasks.extend(
                load_task_file(
                    config,
                    &path,
                    config_root,
                    task_config_dir,
                    templates,
                    monorepo_cf,
                )
                .await?,
            );
        }
        let root = Arc::new(root.to_path_buf());
        let config_root = Arc::new(config_root.to_path_buf());
        for path in exec_files {
            let root = root.clone();
            let config_root = config_root.clone();
            let config = config.clone();
            trust_check_task_include(&path, require_trust)?;
            let mut task = Task::from_path_unrendered_with_cf(
                &path,
                &root,
                &config_root,
                monorepo_cf.cloned(),
            )?;
            if let Err(err) = task.render(&config, &config_root).await {
                if monorepo_cf.is_some() {
                    warn!(
                        "Failed to render task {} in {}: {err:#}. Task will not be available.",
                        task.name,
                        display_path(&path)
                    );
                    continue;
                } else {
                    return Err(err);
                }
            }
            if task.dir.is_none()
                && let Some(ref dir) = *task_config_dir
            {
                task.dir = Some(if contains_template_syntax(dir) {
                    let mut tera = crate::tera::get_tera(Some(config_root.as_ref()));
                    let tera_ctx = task.tera_ctx(&config).await?;
                    render_str(&mut tera, dir, &tera_ctx)?
                } else {
                    dir.clone()
                });
            }
            tasks.push(task);
        }
        Ok(tasks)
    } else {
        Ok(vec![])
    }
}

fn is_mise_config_file_in_task_include(root: &Path, path: &Path) -> bool {
    let Ok(relative_path) = path.strip_prefix(root) else {
        return false;
    };
    let relative_path = relative_path.to_string_lossy().replace('\\', "/");
    let file_name = path
        .file_name()
        .map(|file_name| file_name.to_string_lossy().replace('\\', "/"));
    TOML_CONFIG_MATCHERS.iter().any(|matcher| {
        matcher.is_match(&relative_path) || file_name.as_ref().is_some_and(|f| matcher.is_match(f))
    })
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
    let pattern = file::replace_path(pattern);
    let pattern = pattern.to_string_lossy();
    if is_glob_pattern(&pattern) {
        match glob(dir, &pattern) {
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
        let path = PathBuf::from(&*pattern);
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
    templates: &IndexMap<String, TaskTemplate>,
    monorepo_cf: Option<&Arc<dyn ConfigFile>>,
) -> Result<Vec<Task>> {
    let is_global = is_global_config(cf.get_path());
    let includes = cf
        .task_config_includes()?
        .unwrap_or_else(default_task_includes);

    let mut tasks = vec![];
    let config_root = Arc::new(config_root.to_path_buf());
    let cf_root = cf.config_root();
    let task_config_dir = cf.task_config().dir.clone();
    // a config can only vouch for task include files when it was actually
    // trusted — safe configs load without trust and cannot vouch for anything
    let require_task_include_trust = !is_path_trusted(cf.get_path());

    for include in includes {
        let paths = if include.starts_with("git::") {
            vec![resolve_git_url_to_path(&include).await?]
        } else {
            expand_task_include(&cf_root, &include)
        };
        for path in paths {
            let mut loaded = load_tasks_includes(
                config,
                &path,
                &config_root,
                &task_config_dir,
                templates,
                monorepo_cf,
                require_task_include_trust,
            )
            .await?;
            if is_global || is_global_task_include_path(&path) {
                mark_tasks_as_global(&mut loaded);
            }
            tasks.extend(loaded);
        }
    }
    Ok(tasks)
}

fn task_include_patterns_for_dir(
    dir: &Path,
    config_files: &ConfigMap,
) -> Result<(Vec<String>, PathBuf, bool)> {
    let configs = configs_at_root(dir, config_files);

    // Find the highest-precedence config that has explicit task_config.includes
    // and resolve paths relative to that config file's directory
    Ok(configs
        .iter()
        .find_map(|cf| match cf.task_config_includes() {
            Ok(Some(includes)) => Some(Ok({
                // Resolve relative paths from the config root, not the config file's directory
                (includes, cf.config_root(), false)
            })),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        })
        .transpose()?
        .unwrap_or_else(|| {
            // Default includes should be resolved relative to the search directory
            (default_task_includes(), dir.to_path_buf(), true)
        }))
}

pub fn task_includes_for_dir(dir: &Path, config_files: &ConfigMap) -> Result<Vec<PathBuf>> {
    let (includes, resolve_dir, _) = task_include_patterns_for_dir(dir, config_files)?;

    Ok(includes
        .into_iter()
        .flat_map(|p| {
            // Git URLs are handled by load_file_tasks, not here
            if p.starts_with("git::") {
                return vec![];
            }
            expand_task_include(&resolve_dir, &p)
        })
        .unique()
        .collect::<Vec<_>>())
}

/// Returns the directory where a new file task should be created.
///
/// Existing directories are selected in effective `task_config.includes` order. When the
/// built-in defaults are in use and none exist yet, the first default path is returned so the
/// caller can create it. Explicit includes must contain an existing directory because a missing
/// path cannot be distinguished from a task file.
pub fn task_creation_dir_for_dir(dir: &Path, config_files: &ConfigMap) -> Result<PathBuf> {
    let (includes, resolve_dir, uses_defaults) = task_include_patterns_for_dir(dir, config_files)?;
    let default_create_dir = if uses_defaults {
        includes
            .first()
            .map(|include| resolve_dir.join(file::replace_path(include)))
    } else {
        None
    };
    if let Some(path) = includes
        .iter()
        .filter(|include| !include.starts_with("git::"))
        .flat_map(|include| expand_task_include(&resolve_dir, include))
        .find(|path| path.is_dir())
    {
        return Ok(path);
    }
    if let Some(dir) = default_create_dir {
        return Ok(dir);
    }
    bail!("task includes do not contain an existing directory where a file task can be created")
}

pub async fn load_tasks_in_dir(
    config: &Arc<Config>,
    dir: &Path,
    config_files: &ConfigMap,
    templates: &IndexMap<String, TaskTemplate>,
) -> Result<Vec<Task>> {
    let configs = configs_at_root(dir, config_files);
    // a config can only vouch for task include files when it was actually
    // trusted — safe configs load without trust and cannot vouch for anything
    let require_task_include_trust = !configs.iter().any(|cf| is_path_trusted(cf.get_path()));

    let (includes, resolve_dir) = configs
        .iter()
        .find_map(|cf| match cf.task_config_includes() {
            Ok(Some(includes)) => Some(Ok((includes, cf.config_root()))),
            Ok(None) => None,
            Err(err) => Some(Err(err)),
        })
        .transpose()?
        .unwrap_or_else(|| (default_task_includes(), dir.to_path_buf()));

    let mut config_tasks = vec![];
    for cf in &configs {
        let dir = dir.to_path_buf();
        config_tasks.extend(load_config_tasks(config, (*cf).clone(), &dir, templates, None).await?);
    }

    // Find task_config.dir from the highest-precedence config that defines it
    let task_config_dir = configs.iter().find_map(|cf| cf.task_config().dir.clone());

    let mut file_tasks = vec![];
    for include in &includes {
        let paths = if include.starts_with("git::") {
            vec![resolve_git_url_to_path(include).await?]
        } else {
            expand_task_include(&resolve_dir, include)
        };
        for p in paths {
            let mut loaded = load_tasks_includes(
                config,
                &p,
                dir,
                &task_config_dir,
                templates,
                None,
                require_task_include_trust,
            )
            .await?;
            if is_global_task_include_path(&p) {
                mark_tasks_as_global(&mut loaded);
            }
            file_tasks.extend(loaded);
        }
    }

    let mut tasks = merge_file_and_config_tasks(file_tasks, config_tasks)
        .into_iter()
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

fn trust_check_task_include(path: &Path, require_trust: bool) -> Result<()> {
    if require_trust && !is_global_task_include_path(path) && task_include_requires_trust(path) {
        trust_check(path)?;
    }
    Ok(())
}

/// Template-free task files are inert at load time: TOML and `#MISE` headers
/// are only parsed, and the scripts themselves only run on an explicit
/// `mise run`. Templates are what can execute code while tasks load (via
/// exec() etc. when task fields render), so only files containing template
/// syntax need trust. Paranoid mode keeps requiring trust for everything.
fn task_include_requires_trust(path: &Path) -> bool {
    if Settings::try_get().is_ok_and(|settings| settings.paranoid) {
        return true;
    }
    let Ok(body) = file::read_to_string(path) else {
        // can't read it — fall back to requiring trust
        return true;
    };
    // literal delimiters, plus escaped ones (e.g. `{{`) that decode to
    // templates after TOML parsing and would render at load time
    contains_template_syntax(&body) || crate::task::file_has_decoded_template(path, &body)
}

async fn load_task_file(
    config: &Arc<Config>,
    path: &Path,
    config_root: &Path,
    task_config_dir: &Option<String>,
    templates: &IndexMap<String, TaskTemplate>,
    monorepo_cf: Option<&Arc<dyn ConfigFile>>,
) -> Result<Vec<Task>> {
    let raw = file::read_to_string_async(path).await?;
    let mut tasks = toml::from_str::<Tasks>(&raw)
        .wrap_err_with(|| format!("Error parsing task file: {}", display_path(path)))?
        .0;
    for (name, task) in &mut tasks {
        task.name = name.clone();
        task.config_source = path.to_path_buf();
        task.config_root = Some(config_root.to_path_buf());
        if task.dir.is_none() {
            task.dir = task_config_dir.clone();
        }
        if let Some(monorepo_cf) = monorepo_cf {
            task.cf = Some(monorepo_cf.clone());
        }
    }
    let mut out = vec![];
    for (_, mut task) in tasks {
        let config_root = config_root.to_path_buf();
        resolve_task_template(&mut task, templates)?;
        match task.render(config, &config_root).await {
            Ok(()) => {
                out.push(task);
            }
            Err(err) => {
                if monorepo_cf.is_some() {
                    warn!(
                        "Failed to render task {} in {}: {err:#}. Task will not be available.",
                        task.name,
                        display_path(path)
                    );
                } else {
                    warn!("rendering task: {err:?}");
                    out.push(task);
                }
            }
        }
    }
    Ok(out)
}

fn mark_tasks_as_global(tasks: &mut [Task]) {
    tasks.iter_mut().for_each(|task| task.global = true);
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
    async fn test_reset_reloads_settings() {
        Settings::reset(None);
        let before = Settings::get();

        Config::reset().await.unwrap();
        let after = Settings::get();

        assert!(!Arc::ptr_eq(&before, &after));
        Settings::reset(None);
    }

    #[test]
    fn test_config_set_contains_matches_symlinked_prefix() {
        // Regression for https://github.com/jdx/mise/discussions/10483:
        // a global config discovered via a symlinked path prefix (e.g. Fedora
        // Atomic's `/home` -> `/var/home`) must still be recognized as global.
        let tmp = TempDir::new().unwrap();
        let real_dir = tmp.path().join("real");
        fs::create_dir_all(&real_dir).unwrap();
        let real_file = real_dir.join("config.toml");
        fs::write(&real_file, "").unwrap();
        // Symlinked alias of the dir, mimicking `/home` -> `/var/home`.
        let link_dir = tmp.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();
        let aliased_file = link_dir.join("config.toml");

        let mut set = IndexSet::new();
        set.insert(real_file.clone());

        // exact match (fast path)
        assert!(config_set_contains(&set, &real_file));
        // same file reached via a symlinked prefix — false before the fix
        assert!(config_set_contains(&set, &aliased_file));
        // an unrelated path is not a member
        assert!(!config_set_contains(&set, &real_dir.join("other.toml")));
    }

    #[test]
    fn test_has_mise_config_with_glob_filenames() -> Result<()> {
        let tmp = TempDir::new()?;
        let confd = tmp.path().join(".config/mise/conf.d");
        fs::create_dir_all(&confd)?;
        fs::write(confd.join("tools.toml"), "[tools]\n")?;

        assert!(has_mise_config_with_filenames(
            tmp.path(),
            &[".config/mise/conf.d/*.toml".to_string()]
        ));

        Ok(())
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_keeps_windows_native_script() {
        let file_tasks = vec![
            Task {
                name: "pkl:gen".to_string(),
                config_source: PathBuf::from("mise-tasks/pkl/gen"),
                ..Default::default()
            },
            Task {
                name: "pkl:gen.ps1".to_string(),
                config_source: PathBuf::from("mise-tasks/pkl/gen.ps1"),
                ..Default::default()
            },
        ];

        let names = prefer_windows_file_task_siblings_inner(file_tasks)
            .into_iter()
            .map(|task| task.name)
            .collect_vec();

        assert_eq!(names, vec!["pkl:gen"]);
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_ignores_non_windows_extension() {
        let file_tasks = vec![
            Task {
                name: "hello".to_string(),
                config_source: PathBuf::from("mise-tasks/hello"),
                ..Default::default()
            },
            Task {
                name: "hello.sh".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.sh"),
                ..Default::default()
            },
        ];

        let names = prefer_windows_file_task_siblings_inner(file_tasks)
            .into_iter()
            .map(|task| task.name)
            .collect_vec();

        assert_eq!(names, vec!["hello", "hello.sh"]);
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_preserves_toml_overlay_stem() {
        let file_tasks = vec![
            Task {
                name: "hello".to_string(),
                config_source: PathBuf::from("mise-tasks/hello"),
                file: Some(PathBuf::from("mise-tasks/hello")),
                ..Default::default()
            },
            Task {
                name: "hello.ps1".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.ps1"),
                file: Some(PathBuf::from("mise-tasks/hello.ps1")),
                ..Default::default()
            },
        ];
        let config_tasks = vec![Task {
            name: "hello".to_string(),
            description: "windows task metadata".to_string(),
            ..Default::default()
        }];

        let tasks = merge_file_and_config_tasks(
            prefer_windows_file_task_siblings_inner(file_tasks),
            config_tasks,
        );

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "hello");
        assert_eq!(
            tasks[0].config_source,
            PathBuf::from("mise-tasks/hello.ps1")
        );
        assert_eq!(tasks[0].description, "windows task metadata");
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_keeps_exact_stem_for_matching() {
        use crate::task::GetMatchingExt;

        let file_tasks = vec![
            Task {
                name: "hello".to_string(),
                config_source: PathBuf::from("mise-tasks/hello"),
                ..Default::default()
            },
            Task {
                name: "hello.ps1".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.ps1"),
                ..Default::default()
            },
            Task {
                name: "hello.sh".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.sh"),
                ..Default::default()
            },
        ];
        let tasks = prefer_windows_file_task_siblings_inner(file_tasks)
            .into_iter()
            .map(|task| (task.name.clone(), task))
            .collect::<BTreeMap<_, _>>();

        let matches = tasks.get_matching("hello").unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].config_source,
            PathBuf::from("mise-tasks/hello.ps1")
        );
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_keeps_ambiguous_windows_siblings() {
        let file_tasks = vec![
            Task {
                name: "hello".to_string(),
                config_source: PathBuf::from("mise-tasks/hello"),
                ..Default::default()
            },
            Task {
                name: "hello.ps1".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.ps1"),
                ..Default::default()
            },
            Task {
                name: "hello.cmd".to_string(),
                config_source: PathBuf::from("mise-tasks/hello.cmd"),
                ..Default::default()
            },
        ];

        let tasks = prefer_windows_file_task_siblings_inner(file_tasks);
        let task_names = tasks.iter().map(|task| task.name.as_str()).collect_vec();
        let task_sources = tasks
            .iter()
            .map(|task| task.config_source.as_path())
            .collect_vec();

        assert_eq!(task_names, vec!["hello", "hello.ps1", "hello.cmd"]);
        assert_eq!(
            task_sources,
            vec![
                Path::new("mise-tasks/hello"),
                Path::new("mise-tasks/hello.ps1"),
                Path::new("mise-tasks/hello.cmd"),
            ]
        );
    }

    #[test]
    fn test_prefer_windows_file_task_siblings_scopes_to_source_family() {
        let file_tasks = vec![
            Task {
                name: "build".to_string(),
                config_source: PathBuf::from("included-tasks/build"),
                ..Default::default()
            },
            Task {
                name: "build.ps1".to_string(),
                config_source: PathBuf::from("mise-tasks/build.ps1"),
                ..Default::default()
            },
        ];

        let tasks = prefer_windows_file_task_siblings_inner(file_tasks);
        let task_names = tasks.iter().map(|task| task.name.as_str()).collect_vec();
        let task_sources = tasks
            .iter()
            .map(|task| task.config_source.as_path())
            .collect_vec();

        assert_eq!(task_names, vec!["build", "build.ps1"]);
        assert_eq!(
            task_sources,
            vec![
                Path::new("included-tasks/build"),
                Path::new("mise-tasks/build.ps1"),
            ]
        );
    }

    #[test]
    fn test_env_config_patterns() {
        assert_eq!(
            env_config_patterns("linux"),
            vec![
                ".config/mise/config.linux.toml",
                ".config/mise.linux.toml",
                "mise/config.linux.toml",
                "mise.linux.toml",
                ".mise/config.linux.toml",
                ".mise.linux.toml",
                ".config/mise/config.linux.local.toml",
                ".config/mise.linux.local.toml",
                "mise/config.linux.local.toml",
                "mise.linux.local.toml",
                ".mise/config.linux.local.toml",
                ".mise.linux.local.toml",
            ]
        );
    }

    #[test]
    fn test_should_warn_auto_env() {
        let v = |s: &str| versions::Versioning::new(s).unwrap();
        // before the warning window: never warn
        assert!(!should_warn_auto_env(&v("2026.6.2"), None, false));
        // inside the warning window: warn only when the user hasn't decided
        assert!(should_warn_auto_env(&v("2026.12.0"), None, false));
        assert!(should_warn_auto_env(&v("2027.5.9"), None, false));
        assert!(!should_warn_auto_env(&v("2026.12.0"), Some(false), false));
        assert!(!should_warn_auto_env(&v("2026.12.0"), Some(true), true));
        // auto envs already active (e.g. opted in): nothing to warn about
        assert!(!should_warn_auto_env(&v("2026.12.0"), None, true));
        // default flipped on: warning is obsolete
        assert!(!should_warn_auto_env(&v("2027.6.0"), None, true));
        assert!(!should_warn_auto_env(&v("2027.6.0"), Some(false), false));
    }

    #[test]
    fn test_monorepo_lockfile_rollout() {
        let v = |s: &str| versions::Versioning::new(s).unwrap();

        assert!(!monorepo_lockfile_enabled_for_version(
            &v("2026.6.15"),
            None
        ));
        assert!(monorepo_lockfile_enabled_for_version(
            &v("2026.6.15"),
            Some(true)
        ));
        assert!(!monorepo_lockfile_enabled_for_version(
            &v("2027.6.0"),
            Some(false)
        ));
        assert!(monorepo_lockfile_enabled_for_version(&v("2027.6.0"), None));

        assert!(!should_warn_monorepo_lockfile_default(
            &v("2026.11.9"),
            None,
            true,
            true
        ));
        assert!(should_warn_monorepo_lockfile_default(
            &v("2026.12.0"),
            None,
            true,
            true
        ));
        assert!(should_warn_monorepo_lockfile_default(
            &v("2027.5.9"),
            None,
            true,
            true
        ));
        assert!(!should_warn_monorepo_lockfile_default(
            &v("2026.12.0"),
            None,
            false,
            true
        ));
        assert!(!should_warn_monorepo_lockfile_default(
            &v("2026.12.0"),
            None,
            true,
            false
        ));
        assert!(!should_warn_monorepo_lockfile_default(
            &v("2026.12.0"),
            Some(true),
            true,
            true
        ));
        assert!(!should_warn_monorepo_lockfile_default(
            &v("2026.12.0"),
            Some(false),
            true,
            true
        ));
        assert!(!should_warn_monorepo_lockfile_default(
            &v("2027.6.0"),
            None,
            true,
            true
        ));
    }

    #[tokio::test]
    async fn test_get_tool_opts_with_overrides_keeps_inline_opts_with_config_entry() -> Result<()> {
        crate::toolset::install_state::init().await?;

        let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
        let resolved_ba = Arc::new(BackendArg::from("github:jdx/mise-test-fixtures"));
        let config_opts =
            crate::toolset::parse_tool_options("api_url=https://config.example/api/v3,foo=config");
        let mut trs = ToolRequestSet::new();
        trs.add_version(
            crate::toolset::ToolRequest::new_opts(
                resolved_ba,
                "1.0.0",
                config_opts,
                source.clone(),
            )?,
            &source,
        );

        let mut repo_urls = HashMap::new();
        repo_urls.insert(
            "tiny".to_string(),
            "github:jdx/mise-test-fixtures".to_string(),
        );
        let config = Config {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files: Default::default(),
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
            repo_urls,
            shell_aliases: Default::default(),
            tera_files: Default::default(),
            vars: Default::default(),
            vars_results: OnceCell::new(),
        };
        config.tool_request_set.set(trs).ok();
        let config = Arc::new(config);
        let ba = Arc::new(BackendArg::new_raw(
            "tiny".to_string(),
            Some("github:jdx/mise-test-fixtures".to_string()),
            "jdx/mise-test-fixtures".to_string(),
            Some(crate::toolset::parse_tool_options(
                "api_url=https://inline.example/api/v3",
            )),
            crate::cli::args::BackendResolution::new(true),
        ));

        let opts = config.get_tool_opts_with_overrides(&ba).await?;

        assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
        assert_eq!(opts.get("foo"), Some("config"));
        Ok(())
    }

    #[tokio::test]
    async fn test_get_tool_opts_with_overrides_keeps_inline_opts_without_config_entry() -> Result<()>
    {
        let config = Config::reset().await?;
        let ba = Arc::new(BackendArg::from(
            "tiny[api_url=https://inline.example/api/v3]",
        ));

        let opts = config.get_tool_opts_with_overrides(&ba).await?;

        assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_tool_opts_tracks_alias_config_and_inline_sources() -> Result<()> {
        crate::toolset::install_state::init().await?;

        let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
        let config_ba = Arc::new(BackendArg::from("tiny"));
        let config_opts =
            crate::toolset::parse_tool_options("asset_pattern=config-pattern,bar=config");
        let mut trs = ToolRequestSet::new();
        trs.add_version(
            crate::toolset::ToolRequest::new_opts(config_ba, "1.0.0", config_opts, source.clone())?,
            &source,
        );

        let mut all_aliases = AliasMap::default();
        all_aliases.insert(
            "tiny".to_string(),
            Alias {
                backend: Some(
                    "github:jdx/mise-test-fixtures[api_url=https://alias.example/api/v3,asset_pattern=alias-pattern,foo=alias]"
                        .to_string(),
                ),
                versions: Default::default(),
            },
        );
        let config = Config {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files: Default::default(),
            env: OnceCell::new(),
            env_with_sources: OnceCell::new(),
            shorthands: get_shorthands(&Settings::get()),
            hooks: OnceCell::new(),
            tasks_cache: Arc::new(DashMap::new()),
            tool_request_set: OnceCell::new(),
            toolset: OnceCell::new(),
            all_aliases,
            aliases: Default::default(),
            project_root: Default::default(),
            repo_urls: Default::default(),
            shell_aliases: Default::default(),
            tera_files: Default::default(),
            vars: Default::default(),
            vars_results: OnceCell::new(),
        };
        config.tool_request_set.set(trs).ok();
        let config = Arc::new(config);
        let ba = Arc::new(BackendArg::from(
            "tiny[api_url=https://inline.example/api/v3]",
        ));

        let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
        let opts = resolved.options();

        assert_eq!(opts.get("api_url"), Some("https://inline.example/api/v3"));
        assert_eq!(opts.get("asset_pattern"), Some("config-pattern"));
        assert_eq!(opts.get("foo"), Some("alias"));
        assert_eq!(opts.get("bar"), Some("config"));
        assert_eq!(
            resolved.source_for_key("api_url"),
            Some(crate::toolset::ToolOptionSource::InlineBackendArg)
        );
        assert_eq!(
            resolved.source_for_key("asset_pattern"),
            Some(crate::toolset::ToolOptionSource::Config)
        );
        assert_eq!(
            resolved.source_for_key("foo"),
            Some(crate::toolset::ToolOptionSource::BackendAlias)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_tool_opts_prefers_config_over_install_manifest_opts() -> Result<()> {
        crate::toolset::install_state::init().await?;

        let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
        let config_ba = Arc::new(BackendArg::from("http:manifest-opts"));
        let config_opts =
            crate::toolset::parse_tool_options("version_json_path=.current,config_only=true");
        let mut trs = ToolRequestSet::new();
        trs.add_version(
            crate::toolset::ToolRequest::new_opts(config_ba, "1.0.0", config_opts, source.clone())?,
            &source,
        );

        let mut manifest_opts = BTreeMap::new();
        manifest_opts.insert(
            "version_json_path".to_string(),
            toml::Value::String(".manifest".to_string()),
        );
        manifest_opts.insert(
            "manifest_only".to_string(),
            toml::Value::String("true".to_string()),
        );
        let ba = Arc::new(BackendArg::from(
            crate::toolset::install_state::InstallStateTool {
                short: "http:manifest-opts".to_string(),
                full: Some("http:manifest-opts".to_string()),
                versions: vec!["1.0.0".to_string()],
                explicit_backend: true,
                opts: manifest_opts,
                installs_path: None,
            },
        ));

        let config = Config {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files: Default::default(),
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
            vars_results: OnceCell::new(),
        };
        config.tool_request_set.set(trs).ok();
        let config = Arc::new(config);

        let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
        let opts = resolved.options();

        assert_eq!(opts.get("version_json_path"), Some(".current"));
        assert_eq!(
            resolved.source_for_key("version_json_path"),
            Some(crate::toolset::ToolOptionSource::Config)
        );
        assert_eq!(opts.get("manifest_only"), Some("true"));
        assert_eq!(
            resolved.source_for_key("manifest_only"),
            Some(crate::toolset::ToolOptionSource::InstallManifest)
        );
        assert_eq!(opts.get("config_only"), Some("true"));
        assert_eq!(
            resolved.source_for_key("config_only"),
            Some(crate::toolset::ToolOptionSource::Config)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_tool_opts_prefers_s3_listing_config_over_install_manifest_opts()
    -> Result<()> {
        crate::toolset::install_state::init().await?;

        let source = crate::toolset::ToolSource::MiseToml(PathBuf::from("mise.toml"));
        let config_ba = Arc::new(BackendArg::from("s3:manifest-opts"));
        let config_opts = crate::toolset::parse_tool_options(
            "version_prefix=current/,version_regex=current-(.*)",
        );
        let mut trs = ToolRequestSet::new();
        trs.add_version(
            crate::toolset::ToolRequest::new_opts(config_ba, "1.0.0", config_opts, source.clone())?,
            &source,
        );

        let mut manifest_opts = BTreeMap::new();
        manifest_opts.insert(
            "version_prefix".to_string(),
            toml::Value::String("manifest/".to_string()),
        );
        manifest_opts.insert(
            "version_regex".to_string(),
            toml::Value::String("manifest-(.*)".to_string()),
        );
        manifest_opts.insert(
            "endpoint".to_string(),
            toml::Value::String("https://manifest.example".to_string()),
        );
        let ba = Arc::new(BackendArg::from(
            crate::toolset::install_state::InstallStateTool {
                short: "s3:manifest-opts".to_string(),
                full: Some("s3:manifest-opts".to_string()),
                versions: vec!["1.0.0".to_string()],
                explicit_backend: true,
                opts: manifest_opts,
                installs_path: None,
            },
        ));

        let config = Config {
            tera_ctx: BASE_CONTEXT.clone(),
            config_files: Default::default(),
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
            vars_results: OnceCell::new(),
        };
        config.tool_request_set.set(trs).ok();
        let config = Arc::new(config);

        let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
        let opts = resolved.options();

        assert_eq!(opts.get("version_prefix"), Some("current/"));
        assert_eq!(
            resolved.source_for_key("version_prefix"),
            Some(crate::toolset::ToolOptionSource::Config)
        );
        assert_eq!(opts.get("version_regex"), Some("current-(.*)"));
        assert_eq!(
            resolved.source_for_key("version_regex"),
            Some(crate::toolset::ToolOptionSource::Config)
        );
        assert_eq!(opts.get("endpoint"), Some("https://manifest.example"));
        assert_eq!(
            resolved.source_for_key("endpoint"),
            Some(crate::toolset::ToolOptionSource::InstallManifest)
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_resolve_tool_opts_prefers_env_backend_override_over_alias_opts() -> Result<()> {
        unsafe {
            std::env::set_var("MISE_BACKENDS_ENV_OPTS_TEST", "github:env/repo[foo=env]");
        }

        let result = async {
            let mut all_aliases = AliasMap::default();
            all_aliases.insert(
                "env-opts-test".to_string(),
                Alias {
                    backend: Some("github:alias/repo[foo=alias,bar=alias]".to_string()),
                    versions: Default::default(),
                },
            );
            let config = Config {
                tera_ctx: BASE_CONTEXT.clone(),
                config_files: Default::default(),
                env: OnceCell::new(),
                env_with_sources: OnceCell::new(),
                shorthands: get_shorthands(&Settings::get()),
                hooks: OnceCell::new(),
                tasks_cache: Arc::new(DashMap::new()),
                tool_request_set: OnceCell::new(),
                toolset: OnceCell::new(),
                all_aliases,
                aliases: Default::default(),
                project_root: Default::default(),
                repo_urls: Default::default(),
                shell_aliases: Default::default(),
                tera_files: Default::default(),
                vars: Default::default(),
                vars_results: OnceCell::new(),
            };
            config.tool_request_set.set(ToolRequestSet::new()).ok();
            let config = Arc::new(config);
            let ba = Arc::new(BackendArg::from("env-opts-test"));

            let resolved = config.resolve_tool_opts_with_overrides(&ba).await?;
            let opts = resolved.options();

            assert_eq!(ba.full(), "github:env/repo[foo=env]");
            assert_eq!(opts.get("foo"), Some("env"));
            assert_eq!(opts.get("bar"), None);
            assert_eq!(
                resolved.source_for_key("foo"),
                Some(crate::toolset::ToolOptionSource::BackendAlias)
            );
            Ok(())
        }
        .await;

        unsafe {
            std::env::remove_var("MISE_BACKENDS_ENV_OPTS_TEST");
        }

        result
    }

    #[tokio::test]
    async fn test_monorepo_union_tool_request_set_preserves_matching_tools() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let root = temp_dir.path();
        let api = root.join("apps/api");
        let web = root.join("apps/web");
        fs::create_dir_all(&api)?;
        fs::create_dir_all(&web)?;

        let root_config = root.join(".test.mise.toml");
        fs::write(
            &root_config,
            r#"
monorepo_root = true

[monorepo]
config_roots = ["apps/api", "apps/web"]
"#,
        )?;
        fs::write(
            api.join(".test.mise.toml"),
            r#"
[tools]
"github:jdx/mise-test-fixtures" = "1"
"#,
        )?;
        fs::write(
            web.join(".test.mise.toml"),
            r#"
[tools]
"github:jdx/mise-test-fixtures" = "2"
"#,
        )?;

        let mut config_files: ConfigMap = Default::default();
        config_files.insert(
            root_config.clone(),
            Arc::new(config_file::mise_toml::MiseToml::from_file(&root_config)?),
        );
        let config = Config {
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
            vars_results: OnceCell::new(),
        };
        let config = Arc::new(config);

        assert_eq!(
            config.monorepo_config_root_dirs_with_filenames(&DEFAULT_CONFIG_FILENAMES)?,
            vec![api, web]
        );
        let trs = config.monorepo_union_tool_request_set().await?;
        let fixture_versions = trs
            .iter()
            .find(|(ba, _, _)| ba.short.contains("mise-test-fixtures"))
            .map(|(_, requests, _)| {
                requests
                    .iter()
                    .map(|request| request.version().to_string())
                    .collect_vec()
            })
            .unwrap_or_default();

        assert_eq!(fixture_versions, vec!["1", "2"]);
        Ok(())
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

    #[tokio::test]
    async fn test_get_repo_url_ssh() -> Result<()> {
        let config = Config::reset().await?;
        let urls = [
            "ssh://git@gitlab.dev/mobile/asdf-gitique.git",
            "git@github.com:user/repo.git",
            "git://example.com/repo.git",
            "http://example.com/repo.git",
            "https://example.com/repo.git",
        ];

        for url in urls {
            assert!(
                config.get_repo_url(url).is_some(),
                "URL should be considered valid: {url}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_load_task_file_supports_per_task_vars() -> Result<()> {
        let config = Config::reset().await?;
        let temp_dir = TempDir::new()?;
        let tasks_toml = temp_dir.path().join("tasks.toml");
        fs::write(
            &tasks_toml,
            r#"
[build]
description = "{{vars.target}}"
run = "echo build"
vars = { target = "linux" }
"#,
        )?;

        let tasks = load_task_file(
            &config,
            &tasks_toml,
            temp_dir.path(),
            &None,
            &IndexMap::new(),
            None,
        )
        .await?;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].name, "build");
        assert_eq!(tasks[0].description, "linux");
        Ok(())
    }
}
