use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::config::config_file::ConfigFile;
use crate::config::env_directive::EnvDirective;
use crate::env;
use crate::task::Task;
use crate::task::task_helpers::canonicalize_path;
use crate::toolset::{Toolset, ToolsetBuilder};
use eyre::Result;
use indexmap::IndexMap;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

type EnvResolutionResult = (BTreeMap<String, String>, Vec<(String, String)>);

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
        }
    }
}

impl TaskContextBuilder {
    pub fn new() -> Self {
        Self {
            toolset_cache: RwLock::new(IndexMap::new()),
            tool_request_set_cache: RwLock::new(IndexMap::new()),
            env_resolution_cache: RwLock::new(IndexMap::new()),
        }
    }

    /// Build toolset for a task, with caching for monorepo tasks
    pub async fn build_toolset_for_task(
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

            let config_paths = crate::config::load_config_hierarchy_from_dir(task_dir)?;
            trace!(
                "task {} found {} config files in hierarchy",
                task.name,
                config_paths.len()
            );

            let task_config_files =
                crate::config::load_config_files_from_paths(&config_paths).await?;

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

    /// Resolve environment variables for a task using its config file context
    /// This is used for monorepo tasks to load env vars from subdirectory mise.toml files
    pub async fn resolve_task_env_with_config(
        &self,
        config: &Arc<Config>,
        task: &Task,
        task_cf: &Arc<dyn ConfigFile>,
        ts: &Toolset,
    ) -> Result<(BTreeMap<String, String>, Vec<(String, String)>)> {
        // Determine if this is a monorepo task (task config differs from current project root)
        let is_monorepo_task = task_cf.project_root() != config.project_root;

        // Check if task runs in the current working directory
        let task_runs_in_cwd = task
            .dir(config)
            .await?
            .and_then(|dir| config.project_root.as_ref().map(|pr| dir == *pr))
            .unwrap_or(false);

        // Get env entries - load the FULL config hierarchy for monorepo tasks
        let all_config_env_entries: Vec<(crate::config::env_directive::EnvDirective, PathBuf)> =
            if is_monorepo_task && !task_runs_in_cwd {
                // For monorepo tasks that DON'T run in cwd: Load config hierarchy from the task's directory
                // This includes parent configs AND MISE_ENV-specific configs
                let task_dir = task_cf.get_path().parent().unwrap_or(task_cf.get_path());

                trace!(
                    "Loading config hierarchy for monorepo task {} from {}",
                    task.name,
                    task_dir.display()
                );

                // Load all config files in the hierarchy
                let config_paths = crate::config::load_config_hierarchy_from_dir(task_dir)?;
                trace!("Found {} config files in hierarchy", config_paths.len());

                let task_config_files =
                    crate::config::load_config_files_from_paths(&config_paths).await?;

                // Extract env entries from all config files
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
            return task.render_env(config, ts).await;
        }

        let config_path = canonicalize_path(task_cf.get_path());

        // Check cache first if task has no task-specific env directives
        if task.env.0.is_empty() && task.inherited_env.0.is_empty() {
            let cache = self
                .env_resolution_cache
                .read()
                .expect("env_resolution_cache RwLock poisoned");
            if let Some(cached_env) = cache.get(&config_path) {
                trace!(
                    "task {} using cached env resolution from {}",
                    task.name,
                    config_path.display()
                );
                return Ok(cached_env.clone());
            }
        }

        let mut env = ts.full_env(config).await?;
        let tera_ctx = self.build_tera_context(task_cf, ts, config).await?;

        // Resolve config-level env from ALL config files, not just task_cf
        let config_env_results = self
            .resolve_env_directives(config, &tera_ctx, &env, all_config_env_entries)
            .await?;
        Self::apply_env_results(&mut env, &config_env_results);

        let task_env_directives = self.build_task_env_directives(task);
        let task_env_results = self
            .resolve_env_directives(config, &tera_ctx, &env, task_env_directives)
            .await?;

        let task_env = self.extract_task_env(&task_env_results);
        Self::apply_env_results(&mut env, &task_env_results);

        // Cache the result if no task-specific env directives
        if task.env.0.is_empty() && task.inherited_env.0.is_empty() {
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
                (env.clone(), task_env.clone())
            });
        }

        Ok((env, task_env))
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
    async fn build_tera_context(
        &self,
        task_cf: &Arc<dyn ConfigFile>,
        ts: &Toolset,
        config: &Arc<Config>,
    ) -> Result<tera::Context> {
        let mut tera_ctx = ts.tera_ctx(config).await?.clone();
        if let Some(root) = task_cf.project_root() {
            tera_ctx.insert("config_root", &root);
        }
        Ok(tera_ctx)
    }

    /// Build env directives from task-specific env (including inherited env)
    fn build_task_env_directives(&self, task: &Task) -> Vec<(EnvDirective, PathBuf)> {
        // Include inherited_env first (so task's own env can override it)
        task.inherited_env
            .0
            .iter()
            .chain(task.env.0.iter())
            .map(|directive| (directive.clone(), task.config_source.clone()))
            .collect()
    }

    /// Resolve env directives using EnvResults
    async fn resolve_env_directives(
        &self,
        config: &Arc<Config>,
        tera_ctx: &tera::Context,
        env: &BTreeMap<String, String>,
        directives: Vec<(EnvDirective, PathBuf)>,
    ) -> Result<crate::config::env_directive::EnvResults> {
        use crate::config::env_directive::{EnvResolveOptions, EnvResults, ToolsFilter};
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
    fn extract_task_env(
        &self,
        task_env_results: &crate::config::env_directive::EnvResults,
    ) -> Vec<(String, String)> {
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
        results: &crate::config::env_directive::EnvResults,
    ) {
        // Apply environment variables
        for (k, (v, _)) in &results.env {
            env.insert(k.clone(), v.clone());
        }

        // Remove explicitly unset variables
        for key in &results.env_remove {
            env.remove(key);
        }

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
    pub fn tool_request_set_cache(
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

        let mut results = crate::config::env_directive::EnvResults::default();
        results.env.insert(
            "NEW_VAR".to_string(),
            ("new_value".to_string(), PathBuf::from("/test")),
        );

        TaskContextBuilder::apply_env_results(&mut env, &results);

        assert_eq!(env.get("EXISTING"), Some(&"value".to_string()));
        assert_eq!(env.get("NEW_VAR"), Some(&"new_value".to_string()));
    }

    #[test]
    fn test_apply_env_results_removes_vars() {
        let mut env = BTreeMap::new();
        env.insert("TO_REMOVE".to_string(), "value".to_string());
        env.insert("TO_KEEP".to_string(), "value".to_string());

        let mut results = crate::config::env_directive::EnvResults::default();
        results.env_remove.insert("TO_REMOVE".to_string());

        TaskContextBuilder::apply_env_results(&mut env, &results);

        assert_eq!(env.get("TO_REMOVE"), None);
        assert_eq!(env.get("TO_KEEP"), Some(&"value".to_string()));
    }

    #[test]
    fn test_apply_env_results_path_handling() {
        let mut env = BTreeMap::new();
        env.insert(env::PATH_KEY.to_string(), "/existing/path".to_string());

        let mut results = crate::config::env_directive::EnvResults::default();
        results
            .env_paths
            .push(PathBuf::from("/new/path").to_path_buf());

        TaskContextBuilder::apply_env_results(&mut env, &results);

        let path = env.get(&*env::PATH_KEY).unwrap();
        assert!(path.contains("/new/path"));
    }

    #[test]
    fn test_extract_task_env() {
        let builder = TaskContextBuilder::new();
        let mut results = crate::config::env_directive::EnvResults::default();
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
