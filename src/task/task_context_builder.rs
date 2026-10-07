use crate::args::ToolArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;
use crate::config::env_directive::{EnvDirective, EnvResolveOptions, EnvResults, ToolsFilter};
use crate::env;
use crate::task::Task;
use crate::task::task_helpers::canonicalize_path;
use crate::toolset::{Toolset, ToolsetBuilder};
use eyre::Result;
use indexmap::IndexMap;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

type EnvResolutionResult = (
    BTreeMap<String, String>,
    Vec<(String, String)>,
    Option<IndexMap<String, String>>,
    BTreeSet<String>,
    TaskEnvKeys,
);

/// What the secrets checks need to know about how a task's env came about.
#[derive(Debug, Default, Clone)]
pub(crate) struct TaskEnvKeys {
    /// every key mise itself sets for the task, whatever its value
    pub(crate) mise: BTreeSet<String>,
    /// keys whose task-env `default` directive rendered (and did not yield to a value)
    pub(crate) rendered_defaults: BTreeSet<String>,
}

/// The config-level `[env]` of a monorepo task's own hierarchy, for a secrets source.
#[derive(Debug, Default, Clone)]
pub(crate) struct SourceConfigEnv {
    pub(crate) values: BTreeMap<String, String>,
    pub(crate) unset: BTreeSet<String>,
    /// `_.path` entries of the hierarchy, in the order the directives gave them
    pub(crate) paths: Vec<PathBuf>,
    /// `_.source`, module or venv directives were left out (`skip_scripts`): keys they could
    /// set are unknown.
    pub(crate) skipped_scripts: bool,
}

/// Builds toolset and environment context for task execution
///
/// Handles:
/// - Toolset caching for monorepo tasks
/// - Environment resolution with config file contexts
/// - Tool request set caching
pub struct TaskContextBuilder {
    toolset_cache: RwLock<IndexMap<PathBuf, Arc<Toolset>>>,
    tool_request_set_cache: RwLock<IndexMap<PathBuf, Arc<crate::toolset::ToolRequestSet>>>,
    env_resolution_cache: RwLock<IndexMap<PathBuf, EnvResolutionResult>>,
    /// The config-level `[env]` results of a monorepo hierarchy, run once per hierarchy: the
    /// task's own env preparation and the secrets source env both consume it, so a
    /// `_.source`/module script (a credential refresher, say) is not executed twice.
    hierarchy_env_cache: RwLock<IndexMap<String, Arc<tokio::sync::OnceCell<EnvResults>>>>,
}

impl Clone for TaskContextBuilder {
    fn clone(&self) -> Self {
        // Clone by creating a new instance with the same cache contents
        Self {
            toolset_cache: RwLock::new(self.toolset_cache.read().unwrap().clone()),
            tool_request_set_cache: RwLock::new(
                self.tool_request_set_cache.read().unwrap().clone(),
            ),
            env_resolution_cache: RwLock::new(self.env_resolution_cache.read().unwrap().clone()),
            hierarchy_env_cache: RwLock::new(self.hierarchy_env_cache.read().unwrap().clone()),
        }
    }
}

impl TaskContextBuilder {
    pub fn new() -> Self {
        Self {
            toolset_cache: RwLock::new(IndexMap::new()),
            tool_request_set_cache: RwLock::new(IndexMap::new()),
            env_resolution_cache: RwLock::new(IndexMap::new()),
            hierarchy_env_cache: RwLock::new(IndexMap::new()),
        }
    }

    /// `resolve_env_directives` for a monorepo hierarchy's config entries, shared only between
    /// callers whose inputs are identical: the key hashes the hierarchy path, the base env
    /// (which carries the toolset's effect, `--tool` included) and the directives. A concurrent miss on one key evaluates once.
    async fn hierarchy_env_results(
        &self,
        config: &Arc<Config>,
        task_cf: &Arc<dyn ConfigFile>,
        tera_ctx: &tera::Context,
        env: &BTreeMap<String, String>,
        entries: Vec<(EnvDirective, PathBuf)>,
    ) -> Result<EnvResults> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(
            canonicalize_path(task_cf.get_path())
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update(format!("{env:?}").as_bytes());
        // The template context as a whole is not hashed (a map with no stable iteration
        // order): its `tools` come from the toolset, whose install paths are in the base env
        // hashed above. Its `vars` are resolved separately, so they are hashed: directives
        // may read `{{ vars.X }}`. A differently ordered rendering only costs a cache miss.
        if let Some(vars) = tera_ctx.get("vars") {
            hasher.update(vars.to_string().as_bytes());
        }
        hasher.update(format!("{entries:?}").as_bytes());
        let key = hasher.finalize().to_hex().to_string();
        let cell = self
            .hierarchy_env_cache
            .write()
            .unwrap()
            .entry(key)
            .or_default()
            .clone();
        cell.get_or_try_init(|| self.resolve_env_directives(config, tera_ctx, env, entries))
            .await
            .cloned()
    }

    /// Build toolset for a task, with caching for monorepo tasks
    pub(crate) async fn build_toolset_for_task(
        &self,
        config: &Arc<Config>,
        task: &Task,
        task_cf: Option<&Arc<dyn ConfigFile>>,
        tools: &[ToolArg],
    ) -> Result<Toolset> {
        // Only use task-specific config file context for monorepo tasks
        // (tasks with self.cf set, not just those with a config_source)
        if let (Some(task_cf), Some(_)) = (task_cf, &task.cf) {
            let config_path = canonicalize_path(task_cf.get_path());

            trace!(
                "task {} using monorepo config file context from {}",
                task.name,
                config_path.display()
            );

            // Check cache first if no task-specific tools or CLI args
            if tools.is_empty() && task.tools.is_empty() {
                let cache = self
                    .toolset_cache
                    .read()
                    .expect("toolset_cache RwLock poisoned");
                if let Some(cached_ts) = cache.get(&config_path) {
                    trace!(
                        "task {} using cached toolset from {}",
                        task.name,
                        config_path.display()
                    );
                    // Clone Arc, not the entire Toolset
                    return Ok(Arc::unwrap_or_clone(Arc::clone(cached_ts)));
                }
            }

            let task_dir = task_cf.get_path().parent().unwrap_or(task_cf.get_path());
            trace!(
                "Loading config hierarchy for monorepo task {} toolset from {}",
                task.name,
                task_dir.display()
            );

            let (config_paths, idiomatic_filenames) =
                crate::config::load_config_hierarchy_from_dir(task_dir).await?;
            trace!(
                "task {} found {} config files in hierarchy",
                task.name,
                config_paths.len()
            );

            let task_config_files =
                crate::config::load_config_files_from_paths(&config_paths, &idiomatic_filenames)
                    .await?;

            let task_ts = ToolsetBuilder::new()
                .with_config_files(task_config_files)
                .with_args(tools)
                .build(config)
                .await?;

            trace!("task {} final toolset: {:?}", task.name, task_ts);

            // Cache the toolset if no task-specific tools or CLI args
            if tools.is_empty() && task.tools.is_empty() {
                let mut cache = self
                    .toolset_cache
                    .write()
                    .expect("toolset_cache RwLock poisoned");
                cache.insert(config_path.clone(), Arc::new(task_ts.clone()));
                trace!(
                    "task {} cached toolset to {}",
                    task.name,
                    config_path.display()
                );
            }

            Ok(task_ts)
        } else {
            trace!("task {} using standard toolset build", task.name);
            // Standard toolset build - includes all config files
            ToolsetBuilder::new().with_args(tools).build(config).await
        }
    }

    /// The config hierarchy of a monorepo task's own directory, or `None` when the task runs
    /// in the current project. This is what task `[env]` is resolved from, and where the
    /// task's secrets source is selected.
    pub(crate) async fn task_config_files(
        &self,
        config: &Arc<Config>,
        task: &Task,
        task_cf: &Arc<dyn ConfigFile>,
    ) -> Result<Option<crate::config::ConfigMap>> {
        let is_monorepo_task = task_cf.project_root() != config.project_root;
        let task_runs_in_cwd = task
            .dir(config)
            .await?
            .and_then(|dir| config.project_root.as_ref().map(|pr| dir == *pr))
            .unwrap_or(false);
        if !is_monorepo_task || task_runs_in_cwd {
            return Ok(None);
        }
        let task_dir = task_cf.get_path().parent().unwrap_or(task_cf.get_path());

        trace!(
            "Loading config hierarchy for monorepo task {} from {}",
            task.name,
            task_dir.display()
        );

        let (config_paths, idiomatic_filenames) =
            crate::config::load_config_hierarchy_from_dir(task_dir).await?;
        trace!("Found {} config files in hierarchy", config_paths.len());

        Ok(Some(
            crate::config::load_config_files_from_paths(&config_paths, &idiomatic_filenames)
                .await?,
        ))
    }

    /// The config-level `[env]` of a monorepo task's own hierarchy, for a secrets source that
    /// runs from that subproject (so fnox sees its `AWS_PROFILE`, `FNOX_PROFILE`, ...). Returns
    /// the resolved values and the keys the hierarchy unsets, or `None` when the task runs in the
    /// current project. It never includes the task's own env, its dependencies' env or secrets.
    pub(crate) async fn config_env_for_source(
        &self,
        config: &Arc<Config>,
        task: &Task,
        ts: &Toolset,
        skip_scripts: bool,
    ) -> Result<Option<SourceConfigEnv>> {
        let Some(task_cf) = task.cf.as_ref() else {
            return Ok(None);
        };
        let Some(files) = self.task_config_files(config, task, task_cf).await? else {
            return Ok(None);
        };
        let is_script = |d: &EnvDirective| {
            matches!(
                d,
                EnvDirective::Source(..)
                    | EnvDirective::Module(..)
                    | EnvDirective::PythonVenv { .. }
            )
        };
        let entries: Vec<(EnvDirective, PathBuf)> = files
            .iter()
            .rev()
            .filter_map(|(source, cf)| {
                cf.env_entries()
                    .ok()
                    .map(|entries| entries.into_iter().map(move |e| (e, source.clone())))
            })
            .flatten()
            // A static preflight must not run `_.source` scripts or modules: the keys they
            // could set are then simply unknown, and the spawn-time check decides.
            .filter(|(d, _)| !skip_scripts || !is_script(d))
            .collect();
        let skipped_scripts = skip_scripts
            && files.values().any(|cf| {
                cf.env_entries()
                    .map(|entries| entries.iter().any(is_script))
                    .unwrap_or(false)
            });
        let (tera_ctx, _) = self
            .build_tera_context(task_cf, ts, config, Some(&files))
            .await?;
        let (mut env, env_remove) = ts.full_env_with_removals(config).await?;
        // as the task env replay does: a `required` directive may still read a value the
        // root config removed
        for key in &env_remove {
            if let Some(value) = env::PRISTINE_ENV.get(key) {
                env.insert(key.clone(), value.clone());
            }
        }
        // the full (script-running) result is shared with the task's own env preparation
        let results = if skip_scripts {
            self.resolve_env_directives(config, &tera_ctx, &env, entries)
                .await?
        } else {
            self.hierarchy_env_results(config, task_cf, &tera_ctx, &env, entries)
                .await?
        };
        let values = results
            .env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect();
        Ok(Some(SourceConfigEnv {
            values,
            unset: results.env_remove.clone(),
            paths: results.env_paths.clone(),
            skipped_scripts,
        }))
    }

    /// Resolve environment variables for a task using its config file context
    /// This is used for monorepo tasks to load env vars from subdirectory mise.toml files
    /// Returns (env, task_env, resolved_vars) where resolved_vars contains vars from the
    /// task's config hierarchy (for injecting into tera context during script rendering)
    pub(crate) async fn resolve_task_env_with_config(
        &self,
        config: &Arc<Config>,
        task: &Task,
        task_cf: &Arc<dyn ConfigFile>,
        ts: &Toolset,
    ) -> Result<(
        BTreeMap<String, String>,
        Vec<(String, String)>,
        Option<IndexMap<String, String>>,
        BTreeSet<String>,
        TaskEnvKeys,
    )> {
        // Determine if this is a monorepo task (task config differs from current project root)
        let is_monorepo_task = task_cf.project_root() != config.project_root;

        // Check if task runs in the current working directory
        let task_runs_in_cwd = task
            .dir(config)
            .await?
            .and_then(|dir| config.project_root.as_ref().map(|pr| dir == *pr))
            .unwrap_or(false);

        // Load task config files for monorepo tasks (reused for both vars and env resolution)
        let task_config_files = if is_monorepo_task && !task_runs_in_cwd {
            self.task_config_files(config, task, task_cf).await?
        } else {
            None
        };

        // Get env entries - load the FULL config hierarchy for monorepo tasks
        let all_config_env_entries: Vec<(EnvDirective, PathBuf)> =
            if let Some(ref task_config_files) = task_config_files {
                // Extract env entries from all config files in the task's hierarchy
                task_config_files
                    .iter()
                    .rev()
                    .filter_map(|(source, cf)| {
                        cf.env_entries()
                            .ok()
                            .map(|entries| entries.into_iter().map(move |e| (e, source.clone())))
                    })
                    .flatten()
                    .collect()
            } else {
                // For regular tasks OR monorepo tasks that run in cwd:
                // Use ALL config files from the current project (including MISE_ENV-specific ones)
                // This fixes env inheritance for tasks with dir="{{cwd}}"
                config
                    .config_files
                    .iter()
                    .rev()
                    .filter_map(|(source, cf)| {
                        cf.env_entries()
                            .ok()
                            .map(|entries| entries.into_iter().map(move |e| (e, source.clone())))
                    })
                    .flatten()
                    .collect()
            };

        // Early return if no special context needed
        // Check using task_cf entries for compatibility with existing logic
        let task_cf_env_entries = task_cf.env_entries()?;
        if self.should_use_standard_env_resolution(task, task_cf, config, &task_cf_env_entries) {
            let (env, task_env, env_remove, keys) = task.render_env(config, ts).await?;
            return Ok((env, task_env, None, env_remove, keys));
        }

        let config_path = canonicalize_path(task_cf.get_path());

        // Check cache first if task has no task-specific env directives or tools
        if task.env.0.is_empty()
            && task.inherited_env.0.is_empty()
            && task.overlay_env.is_empty()
            && task.tools.is_empty()
        {
            let cache = self
                .env_resolution_cache
                .read()
                .expect("env_resolution_cache RwLock poisoned");
            if let Some(cached) = cache.get(&config_path) {
                trace!(
                    "task {} using cached env resolution from {}",
                    task.name,
                    config_path.display()
                );
                return Ok(cached.clone());
            }
        }

        let (mut env, mut env_remove, mut mise_keys) =
            ts.full_env_with_removals_and_keys(config).await?;
        let (tera_ctx, resolved_vars) = self
            .build_tera_context(task_cf, ts, config, task_config_files.as_ref())
            .await?;

        // Resolve config-level env from ALL config files, not just task_cf
        //
        // This path replays config directives to resolve them relative to the task's
        // config hierarchy. Start that replay with caller values that the already
        // resolved config removed: a `required` directive must still be able to
        // validate the original caller value before a later directive removes it.
        // The replay's EnvResults and env_remove set keep those values out of the
        // final task environment.
        let mut config_resolution_env = env.clone();
        for key in &env_remove {
            if let Some(value) = env::PRISTINE_ENV.get(key) {
                config_resolution_env.insert(key.clone(), value.clone());
            }
        }
        let config_env_results = if task_config_files.is_some() {
            self.hierarchy_env_results(
                config,
                task_cf,
                &tera_ctx,
                &config_resolution_env,
                all_config_env_entries,
            )
            .await?
        } else {
            self.resolve_env_directives(
                config,
                &tera_ctx,
                &config_resolution_env,
                all_config_env_entries,
            )
            .await?
        };
        Self::apply_env_results(&mut env, &mut env_remove, &config_env_results);
        mise_keys.extend(config_env_results.env.keys().cloned());

        // Register config-level redactions resolved through the task context
        if !config_env_results.redactions.is_empty() {
            config.add_redactions_excluding(
                config_env_results.redactions.iter().cloned(),
                &env,
                &config_env_results.redaction_exclusions,
            );
        }

        let task_env_directives = self.build_task_env_directives(task);
        let task_env_results = self
            .resolve_env_directives(config, &tera_ctx, &env, task_env_directives)
            .await?;

        let task_env = self.extract_task_env(&task_env_results);
        mise_keys.extend(task_env.iter().map(|(k, _)| k.clone()));
        let rendered_defaults = task_env_results.rendered_defaults.clone();
        Self::apply_env_results(&mut env, &mut env_remove, &task_env_results);

        // Register task-specific redactions with the global redactor
        // Include both task-level redact=true keys and config-level redaction patterns
        // so that config-level `redactions = ["PATTERN"]` also covers task-specific env vars
        let task_redact_keys = config
            .redaction_keys()
            .into_iter()
            .chain(task_env_results.redactions.iter().cloned());
        let mut redaction_exclusions = config_env_results.redaction_exclusions.clone();
        for key in task_env_results.env.keys() {
            redaction_exclusions.remove(key);
        }
        redaction_exclusions.extend(task_env_results.redaction_exclusions.iter().cloned());
        config.add_redactions_excluding(task_redact_keys, &env, &redaction_exclusions);

        // Cache the result if no task-specific env directives or tools
        if task.env.0.is_empty()
            && task.inherited_env.0.is_empty()
            && task.overlay_env.is_empty()
            && task.tools.is_empty()
        {
            let mut cache = self
                .env_resolution_cache
                .write()
                .expect("env_resolution_cache RwLock poisoned");
            // Double-check: another thread may have populated while we were resolving
            cache.entry(config_path.clone()).or_insert_with(|| {
                trace!(
                    "task {} cached env resolution to {}",
                    task.name,
                    config_path.display()
                );
                (
                    env.clone(),
                    task_env.clone(),
                    resolved_vars.clone(),
                    env_remove.clone(),
                    TaskEnvKeys {
                        mise: mise_keys.clone(),
                        rendered_defaults: rendered_defaults.clone(),
                    },
                )
            });
        }

        Ok((
            env,
            task_env,
            resolved_vars,
            env_remove,
            TaskEnvKeys {
                mise: mise_keys,
                rendered_defaults,
            },
        ))
    }

    /// Check if standard env resolution should be used instead of special context
    fn should_use_standard_env_resolution(
        &self,
        task: &Task,
        task_cf: &Arc<dyn ConfigFile>,
        config: &Arc<Config>,
        config_env_entries: &[EnvDirective],
    ) -> bool {
        if let (Some(task_config_root), Some(current_config_root)) =
            (task_cf.project_root(), config.project_root.as_ref())
            && task_config_root == *current_config_root
            && config_env_entries.is_empty()
        {
            trace!(
                "task {} config root matches current and no config env, using standard env resolution",
                task.name
            );
            return true;
        }
        false
    }

    /// Build tera context with config_root for monorepo tasks
    /// If task_config_files is provided, resolves vars from the task's config hierarchy
    /// and merges them into the tera context so env directives can reference {{ vars.X }}
    /// Returns (tera_context, resolved_vars) where resolved_vars is Some if task-specific
    /// vars were resolved (for passing to script rendering)
    async fn build_tera_context(
        &self,
        task_cf: &Arc<dyn ConfigFile>,
        ts: &Toolset,
        config: &Arc<Config>,
        task_config_files: Option<&IndexMap<PathBuf, Arc<dyn ConfigFile>>>,
    ) -> Result<(tera::Context, Option<IndexMap<String, String>>)> {
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        if let Some(root) = task_cf.project_root() {
            tera_ctx.insert("config_root", &root);
        }
        let mut resolved_vars = None;
        // If we have task-specific config files, resolve vars from them
        if let Some(task_config_files) = task_config_files {
            let vars_entries: Vec<(EnvDirective, PathBuf)> = task_config_files
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

            if !vars_entries.is_empty() {
                let vars_results = EnvResults::resolve(
                    config,
                    tera_ctx.clone(),
                    &env::PRISTINE_ENV,
                    vars_entries,
                    EnvResolveOptions {
                        vars: true,
                        tools: ToolsFilter::NonToolsOnly,
                        warn_on_missing_required: false,
                    },
                )
                .await?;
                // Merge task vars with existing global vars
                let mut vars: IndexMap<String, String> = config.vars.clone();
                for (k, (v, _)) in &vars_results.vars {
                    vars.insert(k.clone(), v.clone());
                }
                config.add_redactions_excluding(
                    vars_results.redactions.iter().cloned(),
                    &vars.clone().into_iter().collect(),
                    &vars_results.redaction_exclusions,
                );
                tera_ctx.insert("vars", &vars);
                resolved_vars = Some(vars);
            }
        }
        Ok((tera_ctx, resolved_vars))
    }

    /// Build env directives from task-specific env (including inherited env)
    ///
    /// Inherited env comes first (so the task's own env can override it), and overlay entries
    /// come last so a TOML `[tasks.<name>]` block's env overrides the file task's on key
    /// collision, using the overlay's own config path for path-based directives. Values
    /// that use `{{ secrets.X }}` are left out: the executor renders them just before spawn.
    fn build_task_env_directives(&self, task: &Task) -> Vec<(EnvDirective, PathBuf)> {
        task.render_env_directives()
    }

    /// Resolve env directives using EnvResults
    async fn resolve_env_directives(
        &self,
        config: &Arc<Config>,
        tera_ctx: &tera::Context,
        env: &BTreeMap<String, String>,
        directives: Vec<(EnvDirective, PathBuf)>,
    ) -> Result<EnvResults> {
        EnvResults::resolve(
            config,
            tera_ctx.clone(),
            env,
            directives,
            EnvResolveOptions {
                vars: false,
                tools: ToolsFilter::Both,
                warn_on_missing_required: false,
            },
        )
        .await
    }

    /// Extract task env from EnvResults (only task-specific directives)
    fn extract_task_env(&self, task_env_results: &EnvResults) -> Vec<(String, String)> {
        task_env_results
            .env
            .iter()
            .map(|(k, (v, _))| (k.clone(), v.clone()))
            .collect()
    }

    /// Apply EnvResults to an environment map
    /// Handles env vars, env_remove, and env_paths (PATH modifications)
    fn apply_env_results(
        env: &mut BTreeMap<String, String>,
        env_remove: &mut BTreeSet<String>,
        results: &EnvResults,
    ) {
        // Apply environment variables
        for (k, (v, _)) in &results.env {
            env.insert(k.clone(), v.clone());
            env_remove.remove(k);
        }

        // Remove explicitly unset variables
        for key in &results.env_remove {
            env.remove(key);
        }
        env_remove.extend(results.env_remove.iter().cloned());

        // Apply path additions
        if !results.env_paths.is_empty() {
            use crate::path_env::PathEnv;
            let mut path_env = PathEnv::from_iter(env::split_paths(
                &env.get(&*env::PATH_KEY).cloned().unwrap_or_default(),
            ));
            for path in &results.env_paths {
                path_env.add(path.clone());
            }
            env.insert(env::PATH_KEY.to_string(), path_env.to_string());
        }
    }

    /// Get access to the tool request set cache for collecting tools
    pub(crate) fn tool_request_set_cache(
        &self,
    ) -> &RwLock<IndexMap<PathBuf, Arc<crate::toolset::ToolRequestSet>>> {
        &self.tool_request_set_cache
    }
}

impl Default for TaskContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[tokio::test]
async fn hierarchy_env_cache_distinguishes_vars() {
    use crate::config::config_file::mise_toml::MiseToml;
    let config = Config::get().await.unwrap();
    let cf: Arc<dyn ConfigFile> =
        Arc::new(MiseToml::init(std::path::Path::new("/tmp/hier/mise.toml")));
    let builder = TaskContextBuilder::new();
    let entries = || {
        vec![(
            EnvDirective::Val("OUT".into(), "{{ vars.v }}".into(), Default::default()),
            PathBuf::from("/tmp/hier/mise.toml"),
        )]
    };
    let ctx_with = |v: &str| {
        let mut ctx = tera::Context::new();
        ctx.insert("vars", &BTreeMap::from([("v".to_string(), v.to_string())]));
        ctx
    };
    let env = BTreeMap::new();
    let mut seen = vec![];
    for v in ["one", "two"] {
        let res = builder
            .hierarchy_env_results(&config, &cf, &ctx_with(v), &env, entries())
            .await
            .unwrap();
        seen.push(res.env.get("OUT").map(|(v, _)| v.clone()));
    }
    assert_eq!(seen, [Some("one".to_string()), Some("two".to_string())]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_context_builder_new() {
        let builder = TaskContextBuilder::new();
        assert!(builder.toolset_cache.read().unwrap().is_empty());
        assert!(builder.tool_request_set_cache.read().unwrap().is_empty());
        assert!(builder.env_resolution_cache.read().unwrap().is_empty());
    }

    #[test]
    fn test_apply_env_results_basic() {
        let mut env = BTreeMap::new();
        env.insert("EXISTING".to_string(), "value".to_string());

        let mut results = EnvResults::default();
        results.env.insert(
            "NEW_VAR".to_string(),
            ("new_value".to_string(), PathBuf::from("/test")),
        );

        TaskContextBuilder::apply_env_results(&mut env, &mut BTreeSet::new(), &results);

        assert_eq!(env.get("EXISTING"), Some(&"value".to_string()));
        assert_eq!(env.get("NEW_VAR"), Some(&"new_value".to_string()));
    }

    #[test]
    fn test_apply_env_results_removes_vars() {
        let mut env = BTreeMap::new();
        env.insert("TO_REMOVE".to_string(), "value".to_string());
        env.insert("TO_KEEP".to_string(), "value".to_string());

        let mut results = EnvResults::default();
        results.env_remove.insert("TO_REMOVE".to_string());

        let mut env_remove = BTreeSet::new();
        TaskContextBuilder::apply_env_results(&mut env, &mut env_remove, &results);

        assert_eq!(env.get("TO_REMOVE"), None);
        assert_eq!(env.get("TO_KEEP"), Some(&"value".to_string()));
        assert!(env_remove.contains("TO_REMOVE"));
    }

    #[test]
    fn test_apply_env_results_path_handling() {
        let mut env = BTreeMap::new();
        env.insert(env::PATH_KEY.to_string(), "/existing/path".to_string());

        let mut results = EnvResults::default();
        results
            .env_paths
            .push(PathBuf::from("/new/path").to_path_buf());

        TaskContextBuilder::apply_env_results(&mut env, &mut BTreeSet::new(), &results);

        let path = env.get(&*env::PATH_KEY).unwrap();
        assert!(path.contains("/new/path"));
    }

    #[test]
    fn test_extract_task_env() {
        let builder = TaskContextBuilder::new();
        let mut results = EnvResults::default();
        results.env.insert(
            "VAR1".to_string(),
            ("value1".to_string(), PathBuf::from("/test")),
        );
        results.env.insert(
            "VAR2".to_string(),
            ("value2".to_string(), PathBuf::from("/test")),
        );

        let task_env = builder.extract_task_env(&results);

        assert_eq!(task_env.len(), 2);
        assert!(task_env.contains(&("VAR1".to_string(), "value1".to_string())));
        assert!(task_env.contains(&("VAR2".to_string(), "value2".to_string())));
    }
}
