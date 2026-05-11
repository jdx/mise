use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, env_directive::EnvDirective};
use crate::duration;
use crate::env_diff::EnvDiff;
use crate::file::{display_path, is_executable};
use crate::sandbox::SandboxConfig;
use crate::task::TaskKey;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_list::split_task_spec;
use crate::task::task_output::{TaskOutput, trunc};
use crate::task::task_output_handler::OutputHandler;
use crate::task::task_script_parser::subcommand_name_from_parse;
use crate::task::task_source_checker::{save_checksum, sources_are_fresh, task_cwd};
use crate::task::{Deps, FailedTasks, GetMatchingExt, Task};
use crate::toolset::env_cache::CachedEnv;
use crate::ui::{style, time};
use duct::IntoExecutablePath;
use eyre::{Report, Result, ensure, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
#[cfg(unix)]
use nix::errno::Errno;
use std::collections::{BTreeMap, HashSet};
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use xx::file;

/// Global lock for interactive task exclusivity.
/// Interactive tasks acquire a write lock (exclusive), non-interactive tasks acquire a read lock (shared).
static TASK_RUNTIME_LOCK: LazyLock<RwLock<()>> = LazyLock::new(|| RwLock::new(()));

#[allow(dead_code)] // Guards are held for their Drop impl, not read
enum RuntimeLockGuard<'a> {
    Read(tokio::sync::RwLockReadGuard<'a, ()>),
    Write(tokio::sync::RwLockWriteGuard<'a, ()>),
}

async fn acquire_runtime_lock(interactive: bool) -> RuntimeLockGuard<'static> {
    if interactive {
        RuntimeLockGuard::Write(TASK_RUNTIME_LOCK.write().await)
    } else {
        RuntimeLockGuard::Read(TASK_RUNTIME_LOCK.read().await)
    }
}

/// Configuration for TaskExecutor
pub struct TaskExecutorConfig {
    pub force: bool,
    pub cd: Option<PathBuf>,
    pub shell: Option<String>,
    pub tool: Vec<ToolArg>,
    pub timings: bool,
    pub continue_on_error: bool,
    pub dry_run: bool,
    pub skip_deps: bool,
    /// CLI-level sandbox overrides (merged with task-level sandbox config)
    pub sandbox: crate::sandbox::SandboxConfig,
}

/// Executes tasks with proper context, environment, and output handling
pub struct TaskExecutor {
    pub context_builder: TaskContextBuilder,
    pub output_handler: OutputHandler,
    pub failed_tasks: FailedTasks,

    // CLI flags
    pub force: bool,
    pub cd: Option<PathBuf>,
    pub shell: Option<String>,
    pub tool: Vec<ToolArg>,
    pub timings: bool,
    pub continue_on_error: bool,
    pub dry_run: bool,
    pub skip_deps: bool,
    pub sandbox: crate::sandbox::SandboxConfig,
}

impl TaskExecutor {
    pub fn new(
        context_builder: TaskContextBuilder,
        output_handler: OutputHandler,
        config: TaskExecutorConfig,
    ) -> Self {
        Self {
            context_builder,
            output_handler,
            failed_tasks: Arc::new(StdMutex::new(Vec::new())),
            force: config.force,
            cd: config.cd,
            shell: config.shell,
            tool: config.tool,
            timings: config.timings,
            continue_on_error: config.continue_on_error,
            dry_run: config.dry_run,
            skip_deps: config.skip_deps,
            sandbox: config.sandbox,
        }
    }

    pub fn is_stopping(&self) -> bool {
        !self.failed_tasks.lock().unwrap().is_empty()
    }

    pub fn add_failed_task(&self, task: Task, status: Option<i32>) {
        let mut failed = self.failed_tasks.lock().unwrap();
        failed.push((task, status.or(Some(1))));
    }

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        self.output_handler.eprint(task, prefix, line);
    }

    fn output(&self, task: Option<&Task>) -> crate::task::task_output::TaskOutput {
        self.output_handler.output(task)
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.output_handler.quiet(task)
    }

    fn raw(&self, task: Option<&Task>) -> bool {
        self.output_handler.raw(task)
    }

    /// Build a SandboxConfig for a task by merging task-level config with CLI overrides.
    ///
    /// Task-level relative `allow_read`/`allow_write` paths are resolved against the task's
    /// effective working directory (`task.dir(config)`, which itself falls back to `config_root`)
    /// so that `allow_read = ["."]` means "the directory the task runs in", matching how `dir`
    /// resolves. CLI-supplied paths are left as-is and resolved against cwd by `resolve_paths()`.
    async fn build_sandbox_for_task(
        &self,
        task: &Task,
        config: &Arc<Config>,
    ) -> Result<SandboxConfig> {
        let task_base = task.dir(config).await?;
        let resolve_task_path = |p: &PathBuf| -> PathBuf {
            if p.is_absolute() {
                p.clone()
            } else if let Some(base) = &task_base {
                base.join(p)
            } else {
                p.clone()
            }
        };
        let mut sandbox = SandboxConfig {
            deny_read: task.deny_all || task.deny_read || self.sandbox.deny_read,
            deny_write: task.deny_all || task.deny_write || self.sandbox.deny_write,
            deny_net: task.deny_all || task.deny_net || self.sandbox.deny_net,
            deny_env: task.deny_all || task.deny_env || self.sandbox.deny_env,
            allow_read: task
                .allow_read
                .iter()
                .map(&resolve_task_path)
                .chain(self.sandbox.allow_read.iter().cloned())
                .collect(),
            allow_write: task
                .allow_write
                .iter()
                .map(&resolve_task_path)
                .chain(self.sandbox.allow_write.iter().cloned())
                .collect(),
            allow_net: task
                .allow_net
                .iter()
                .chain(self.sandbox.allow_net.iter())
                .cloned()
                .collect(),
            allow_env: task
                .allow_env
                .iter()
                .chain(self.sandbox.allow_env.iter())
                .cloned()
                .collect(),
        };
        sandbox.resolve_paths();
        Ok(sandbox)
    }

    pub fn task_timings(&self) -> bool {
        let output_mode = self.output_handler.output(None);
        self.timings
            || Settings::get().task.timings.unwrap_or(
                output_mode == TaskOutput::Prefix
                    || output_mode == TaskOutput::Timed
                    || output_mode == TaskOutput::KeepOrder,
            )
    }

    /// Run a task, returning true if the task actually executed (not skipped).
    #[allow(clippy::too_many_arguments)]
    pub async fn run_task_sched(
        &self,
        task: &Task,
        config: &Arc<Config>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
        completed_tasks: HashSet<TaskKey>,
        dep_ran: bool,
        semaphore: Arc<Semaphore>,
        permit: &mut Option<OwnedSemaphorePermit>,
    ) -> Result<bool> {
        let prefix = task.estyled_prefix();
        let total_start = std::time::Instant::now();
        if Settings::get().task.skip.contains(&task.name) {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "skipping task");
            }
            return Ok(false);
        }
        // If any dependency actually ran, skip the source freshness check
        // so that downstream tasks are invalidated by upstream changes
        if !self.force && !dep_ran && sources_are_fresh(task, config).await? {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            return Ok(false);
        }

        let mut tools = self.tool.clone();
        for (k, v) in &task.tools {
            tools.push(v.to_tool_spec(k).parse()?);
        }
        let ts_build_start = std::time::Instant::now();

        // Check if we need special handling for monorepo tasks with config file context
        // Remote tasks (from git::/http:/https: URLs) should NOT use config file context
        // because they need tools from the full config hierarchy, not just the local config
        let task_cf = if task.is_remote() {
            None
        } else {
            task.cf(config)
        };

        // Build toolset - either from task's config file or standard way
        let ts = self
            .context_builder
            .build_toolset_for_task(config, task, task_cf, &tools)
            .await?;

        trace!(
            "task {} ToolsetBuilder::build took {}ms",
            task.name,
            ts_build_start.elapsed().as_millis()
        );
        let env_render_start = std::time::Instant::now();

        // Build environment - either from task's config file context or standard way
        // extra_vars contains resolved vars from the task's config hierarchy (for monorepo tasks)
        let (mut env, task_env, extra_vars) = if let Some(task_cf) = task_cf {
            self.context_builder
                .resolve_task_env_with_config(config, task, task_cf, &ts)
                .await?
        } else {
            // Fallback to standard behavior
            let (env, task_env) = task.render_env(config, &ts).await?;
            (env, task_env, None)
        };

        trace!(
            "task {} render_env took {}ms",
            task.name,
            env_render_start.elapsed().as_millis()
        );
        if !self.timings {
            env.insert("MISE_TASK_TIMINGS".to_string(), "0".to_string());
        }
        // Propagate MISE_ENV to child tasks so -E flag works for nested mise invocations
        if !crate::env::MISE_ENV.is_empty() {
            env.insert("MISE_ENV".to_string(), crate::env::MISE_ENV.join(","));
        }
        if let Some(cwd) = &*crate::dirs::CWD {
            env.insert("MISE_ORIGINAL_CWD".into(), cwd.display().to_string());
        }
        // Prefer the task's own config_root so MISE_PROJECT_ROOT is the directory of the
        // mise.toml that defined the task. This keeps the value stable regardless of the
        // cwd from which the task was invoked (important for monorepo subprojects, where
        // config.project_root depends on cwd).
        //
        // Exception: for global tasks (inline in ~/.config/mise/config.toml or scripts in
        // ~/.config/mise/tasks/) and remote tasks (loaded from git/http), task.config_root
        // points at the global/remote location rather than the user's project. Fall back
        // to config.project_root (the local project the user is in) for those, matching
        // the pre-existing behavior.
        let project_root = if task.global || task.is_remote() {
            config.project_root.clone().or(task.config_root.clone())
        } else {
            task.config_root.clone().or(config.project_root.clone())
        };
        if let Some(root) = project_root {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
        }
        if let Some(monorepo_root) = config.monorepo_root() {
            env.insert(
                "MISE_MONOREPO_ROOT".into(),
                monorepo_root.display().to_string(),
            );
        }
        env.insert("MISE_TASK_NAME".into(), task.name.clone());
        let task_file = task
            .file_path(config)
            .await?
            .unwrap_or(task.config_source.clone());
        env.insert("MISE_TASK_FILE".into(), task_file.display().to_string());
        if let Some(dir) = task_file.parent() {
            env.insert("MISE_TASK_DIR".into(), dir.display().to_string());
        }
        if let Some(config_root) = &task.config_root {
            env.insert("MISE_CONFIG_ROOT".into(), config_root.display().to_string());
        }

        // Ensure cache key exists for task subprocesses for nested mise invocations
        // This matches exec.rs behavior - enables caching for subprocesses
        if Settings::get().env_cache {
            let key = CachedEnv::ensure_encryption_key();
            env.insert("__MISE_ENV_CACHE_KEY".into(), key);
        }

        // Embed __MISE_DIFF so a nested `mise` invocation inside this task can
        // recover the pristine env (and pristine PATH) instead of stacking our
        // tool dirs on top of its own. Without this, nested `mise -C <new> exec`
        // would inherit our tool dirs as user-pre-PATH and they would outrank
        // the inner toolset's resolved tool. See discussion #9754.
        if let Ok(serialized) = EnvDiff::from_final_env(&crate::env::PRISTINE_ENV, &env).serialize()
        {
            env.insert("__MISE_DIFF".into(), serialized);
        }

        let timer = std::time::Instant::now();

        if let Some(file) = task.file_path(config).await? {
            let exec_start = std::time::Instant::now();
            self.exec_file(config, &file, task, &env, &prefix, extra_vars)
                .await?;
            trace!(
                "task {} exec_file took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        } else {
            let rendered_run_scripts = task
                .render_run_scripts_with_args(
                    config,
                    self.cd.clone(),
                    &task.args,
                    &env,
                    extra_vars.clone(),
                )
                .await?;

            let get_args = || {
                [String::new()]
                    .iter()
                    .chain(task.args.iter())
                    .cloned()
                    .collect()
            };
            self.parse_usage_spec_and_init_env(config, task, &mut env, get_args, extra_vars)
                .await?;

            // For interactive tasks, acquire the lock before confirmation so the
            // prompt gets exclusive terminal access (consistent with exec_file path).
            let confirm_guard = if task.interactive {
                Some(acquire_runtime_lock(task.interactive).await)
            } else {
                None
            };

            // Check confirmation after usage args are parsed
            self.check_confirmation(config, task, &env).await?;

            let exec_start = std::time::Instant::now();
            self.exec_task_run_entries(
                config,
                task,
                (&env, &task_env),
                &prefix,
                rendered_run_scripts,
                sched_tx,
                confirm_guard,
                &completed_tasks,
                semaphore,
                permit,
            )
            .await?;
            trace!(
                "task {} exec_task_run_entries took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        }

        if self.task_timings()
            && (task.file.as_ref().is_some() || !task.run_script_strings().is_empty())
        {
            self.eprint(
                task,
                &prefix,
                &format!("Finished in {}", time::format_duration(timer.elapsed())),
            );
        }

        save_checksum(task, config).await?;

        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    async fn exec_task_run_entries(
        &self,
        config: &Arc<Config>,
        task: &Task,
        full_env: (&BTreeMap<String, String>, &[(String, String)]),
        prefix: &str,
        rendered_scripts: Vec<(String, Vec<String>)>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
        existing_guard: Option<RuntimeLockGuard<'static>>,
        completed_tasks: &HashSet<TaskKey>,
        semaphore: Arc<Semaphore>,
        permit: &mut Option<OwnedSemaphorePermit>,
    ) -> Result<()> {
        let (env, task_env) = full_env;
        use crate::task::RunEntry;
        let mut script_iter = rendered_scripts.into_iter();

        let needs_tera = task.run().iter().any(RunEntry::has_tera_template);
        let mut tera_state = if needs_tera {
            let usage_values = crate::task::parse_usage_values_from_task(config, task).await?;
            let config_root = task.config_root.clone().unwrap_or_default();
            let tera = crate::tera::get_tera(Some(&config_root));
            let mut tera_ctx = task.tera_ctx(config).await?;
            if !usage_values.is_empty() {
                tera_ctx.insert("usage", &usage_values);
            }
            tera_ctx.insert("env", env);
            Some((tera, tera_ctx))
        } else {
            None
        };

        // Use an existing guard (e.g. from confirmation) or acquire a new one.
        // The lock is held across consecutive script entries for exclusivity
        // and temporarily dropped around inject_and_wait to avoid deadlocking.
        let mut guard = match existing_guard {
            Some(g) => Some(g),
            None => Some(acquire_runtime_lock(task.interactive).await),
        };
        for raw_entry in task.run() {
            let rendered;
            let entry = if let Some((ref mut tera, ref tera_ctx)) = tera_state
                && raw_entry.has_tera_template()
            {
                rendered = raw_entry.render(tera, tera_ctx)?;
                &rendered
            } else {
                raw_entry
            };
            match entry {
                RunEntry::Script(_) => {
                    if let Some((script, args)) = script_iter.next() {
                        if guard.is_none() {
                            guard = Some(acquire_runtime_lock(task.interactive).await);
                        }
                        self.exec_script(&script, &args, task, env, prefix).await?;
                    }
                }
                RunEntry::SingleTask {
                    task: spec,
                    args: entry_args,
                    env: entry_env,
                } => {
                    let resolved_spec = crate::task::resolve_task_pattern(spec, Some(task));
                    let override_args = if entry_args.is_empty() {
                        None
                    } else {
                        Some(entry_args.clone())
                    };
                    let override_env: Vec<(String, String)> = entry_env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let override_env_ref = if override_env.is_empty() {
                        None
                    } else {
                        Some(override_env.as_slice())
                    };
                    guard = None; // drop lock before waiting on sub-tasks
                    // Release the semaphore permit before waiting on sub-tasks to
                    // avoid deadlock when MISE_JOBS=1 (the sub-task needs a permit
                    // but we're holding the only one).
                    let had_permit = permit.is_some();
                    *permit = None;
                    self.inject_and_wait(
                        config,
                        &[resolved_spec],
                        task_env,
                        override_args.as_deref(),
                        override_env_ref,
                        sched_tx.clone(),
                        completed_tasks,
                    )
                    .await?;
                    if had_permit {
                        *permit = Some(semaphore.clone().acquire_owned().await?);
                    }
                }
                RunEntry::TaskGroup { tasks } => {
                    let resolved_tasks: Vec<String> = tasks
                        .iter()
                        .map(|t| crate::task::resolve_task_pattern(t, Some(task)))
                        .collect();
                    guard = None; // drop lock before waiting on sub-tasks
                    let had_permit = permit.is_some();
                    *permit = None;
                    self.inject_and_wait(
                        config,
                        &resolved_tasks,
                        task_env,
                        None,
                        None,
                        sched_tx.clone(),
                        completed_tasks,
                    )
                    .await?;
                    if had_permit {
                        *permit = Some(semaphore.clone().acquire_owned().await?);
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn inject_and_wait(
        &self,
        config: &Arc<Config>,
        specs: &[String],
        task_env: &[(String, String)],
        override_args: Option<&[String]>,
        override_env: Option<&[(String, String)]>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
        completed_tasks: &HashSet<TaskKey>,
    ) -> Result<()> {
        use crate::task::TaskLoadContext;
        trace!("inject start: {}", specs.join(", "));
        // Build tasks list from specs
        // Create a TaskLoadContext from the specs to ensure project tasks are loaded
        let ctx = TaskLoadContext::from_patterns(specs.iter().map(|s| {
            let (name, _) = split_task_spec(s);
            name
        }));
        let tasks = config.tasks_with_context(Some(&ctx)).await?;
        let tasks_map: BTreeMap<String, Task> = tasks
            .values()
            .flat_map(|t| {
                t.aliases
                    .iter()
                    .map(|a| (a.to_string(), t.clone()))
                    .chain(once((t.name.clone(), t.clone())))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut to_run: Vec<Task> = vec![];
        for spec in specs {
            let (name, args) = split_task_spec(spec);
            let matches = tasks_map.get_matching(name)?;
            ensure!(!matches.is_empty(), "task not found: {}", name);
            for t in matches {
                let mut t = (*t).clone();
                t.args = override_args
                    .map(|a| a.to_vec())
                    .unwrap_or_else(|| args.clone());
                // Apply entry-level env via with_dependency_env (high priority,
                // consistent with depends/depends_post) so it overrides the
                // sub-task's own declared env.
                if let Some(env) = override_env {
                    let env_directives: Vec<EnvDirective> = env
                        .iter()
                        .map(|(k, v)| EnvDirective::Val(k.clone(), v.clone(), Default::default()))
                        .collect();
                    t = t.with_dependency_env(&env_directives);
                    if let Some(config_root) = &t.config_root {
                        let env_map: IndexMap<String, String> = env.iter().cloned().collect();
                        t.outputs.re_render_with_env(
                            &t.raw_outputs.clone(),
                            &env_map,
                            config_root,
                        )?;
                    } else {
                        trace!(
                            "re_render_with_env skipped: task {} has no config_root",
                            t.name
                        );
                    }
                }
                if self.skip_deps {
                    t.depends.clear();
                    t.depends_post.clear();
                    t.wait_for.clear();
                }
                to_run.push(t);
            }
        }
        let sub_deps = Deps::new_pruned(config, to_run, completed_tasks).await?;
        let sub_deps = Arc::new(Mutex::new(sub_deps));

        // Pump subgraph into scheduler and signal completion via oneshot when done
        let (done_tx, mut done_rx) = oneshot::channel::<()>();
        let task_env_directives: Vec<EnvDirective> =
            task_env.iter().cloned().map(Into::into).collect();
        {
            let sub_deps_clone = sub_deps.clone();
            let sched_tx = sched_tx.clone();
            // forward initial leaves synchronously
            {
                let mut rx = sub_deps_clone.lock().await.subscribe();
                let mut any = false;
                loop {
                    match rx.try_recv() {
                        Ok(Some(task)) => {
                            any = true;
                            let task = task.derive_env(&task_env_directives);
                            trace!("inject initial leaf: {} {}", task.name, task.args.join(" "));
                            let _ = sched_tx.send((task, sub_deps_clone.clone()));
                        }
                        Ok(None) => {
                            trace!("inject initial done");
                            break;
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                            break;
                        }
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                            break;
                        }
                    }
                }
                if !any {
                    trace!("inject had no initial leaves");
                }
            }
            // then forward remaining leaves asynchronously
            tokio::spawn(async move {
                let mut rx = sub_deps_clone.lock().await.subscribe();
                while let Some(msg) = rx.recv().await {
                    match msg {
                        Some(task) => {
                            trace!(
                                "inject leaf scheduled: {} {}",
                                task.name,
                                task.args.join(" ")
                            );
                            let task = task.derive_env(&task_env_directives);
                            let _ = sched_tx.send((task, sub_deps_clone.clone()));
                        }
                        None => {
                            let _ = done_tx.send(());
                            trace!("inject complete");
                            break;
                        }
                    }
                }
            });
        }

        // Wait for completion with a check for early stopping
        loop {
            // Check if we should stop early due to failure
            if self.is_stopping() && !self.continue_on_error {
                trace!("inject_and_wait: stopping early due to failure");
                // Clean up the dependency graph to ensure completion
                let mut deps = sub_deps.lock().await;
                let tasks_to_remove: Vec<Task> = deps.all().cloned().collect();
                for task in tasks_to_remove {
                    deps.remove(&task);
                }
                drop(deps);
                // Give a short time for the spawned task to finish cleanly
                let _ = tokio::time::timeout(Duration::from_millis(100), done_rx).await;
                return Err(eyre!("task sequence aborted due to failure"));
            }

            // Try to receive the done signal with a short timeout
            match tokio::time::timeout(Duration::from_millis(100), &mut done_rx).await {
                Ok(Ok(())) => {
                    trace!("inject_and_wait: received done signal");
                    break;
                }
                Ok(Err(e)) => {
                    return Err(eyre!(e));
                }
                Err(_) => {
                    // Timeout, check again if we should stop
                    continue;
                }
            }
        }

        // Final check if we failed during the execution
        if self.is_stopping() && !self.continue_on_error {
            return Err(eyre!("task sequence aborted due to failure"));
        }

        Ok(())
    }

    async fn exec_script(
        &self,
        script: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let config = Config::get().await?;
        let script = script.trim_start();
        let cmd = format!("$ {script} {args}", args = args.join(" ")).to_string();
        if !self.quiet(Some(task)) {
            let msg = style::ebold(trunc(prefix, config.redact(&cmd).trim()))
                .bright()
                .to_string();
            self.eprint(task, prefix, &msg)
        }

        if script.starts_with("#!") {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("script");
            tokio::fs::write(&file, script.as_bytes()).await?;
            file::make_executable(&file)?;
            self.exec_with_text_file_busy_retry(&file, args, task, env, prefix)
                .await
        } else {
            let (program, args) = self.get_cmd_program_and_args(script, task, args)?;
            self.exec_program(&program, &args, task, env, prefix).await
        }
    }

    fn get_file_program_and_args(
        &self,
        file: &Path,
        task: &Task,
        args: &[String],
    ) -> Result<(String, Vec<String>)> {
        let display = file.display().to_string();
        if !Settings::get().use_file_shell_for_executable_tasks && can_execute_directly(file) {
            return Ok((display, args.to_vec()));
        }
        let shell = task
            .shell()
            .or_else(|| shell_from_shebang(file))
            .or_else(|| shell_from_extension(file))
            .unwrap_or(Settings::get().default_file_shell()?);
        trace!("using shell: {}", shell.join(" "));
        let mut full_args = shell.clone();
        full_args.push(display);
        if !args.is_empty() {
            full_args.extend(args.iter().cloned());
        }
        Ok((shell[0].clone(), full_args[1..].to_vec()))
    }

    fn get_cmd_program_and_args(
        &self,
        script: &str,
        task: &Task,
        args: &[String],
    ) -> Result<(String, Vec<String>)> {
        let shell = task.shell().unwrap_or(self.clone_default_inline_shell()?);
        trace!("using shell: {}", shell.join(" "));
        let mut full_args = shell.clone();

        #[cfg(windows)]
        {
            full_args.push(script.to_string());
            full_args.extend(args.iter().cloned());
        }

        #[cfg(unix)]
        {
            let mut script = script.to_string();
            if !args.is_empty() {
                script = format!("{script} {}", shell_words::join(args));
            }
            full_args.push(script);
        }
        Ok((full_args[0].clone(), full_args[1..].to_vec()))
    }

    fn clone_default_inline_shell(&self) -> Result<Vec<String>> {
        if let Some(shell) = &self.shell {
            Ok(shell_words::split(shell)?)
        } else {
            Settings::get().default_inline_shell()
        }
    }

    async fn exec_file(
        &self,
        config: &Arc<Config>,
        file: &Path,
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
        extra_vars: Option<IndexMap<String, String>>,
    ) -> Result<()> {
        let mut env = env.clone();
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();
        let get_args = || once(command.clone()).chain(args.clone()).collect_vec();
        self.parse_usage_spec_and_init_env(config, task, &mut env, get_args, extra_vars)
            .await?;

        // For interactive tasks, acquire the lock before confirmation so the
        // prompt gets exclusive terminal access. For non-interactive tasks,
        // acquire after confirmation to avoid blocking the task graph.
        let guard = if task.interactive {
            Some(acquire_runtime_lock(task.interactive).await)
        } else {
            None
        };

        // Check confirmation after usage args are parsed
        self.check_confirmation(config, task, &env).await?;

        if !self.quiet(Some(task)) {
            let cmd = format!("{} {}", display_path(file), args.join(" "))
                .trim()
                .to_string();
            let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
            let cmd = trunc(prefix, config.redact(&cmd).trim());
            self.eprint(task, prefix, &cmd);
        }

        let _guard = if guard.is_some() {
            guard
        } else {
            Some(acquire_runtime_lock(task.interactive).await)
        };
        self.exec(file, &args, task, &env, prefix).await
    }

    async fn exec(
        &self,
        file: &Path,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let (program, args) = self.get_file_program_and_args(file, task, args)?;
        self.exec_program(&program, &args, task, env, prefix).await
    }

    async fn exec_with_text_file_busy_retry(
        &self,
        file: &Path,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        const ETXTBUSY_RETRIES: usize = 3;
        const ETXTBUSY_SLEEP_MS: u64 = 50;

        let mut attempt = 0;
        loop {
            match self.exec(file, args, task, env, prefix).await {
                Ok(()) => break Ok(()),
                Err(err) if Self::is_text_file_busy(&err) && attempt < ETXTBUSY_RETRIES => {
                    attempt += 1;
                    trace!(
                        "retrying execution of {} after ETXTBUSY (attempt {}/{})",
                        display_path(file),
                        attempt,
                        ETXTBUSY_RETRIES
                    );
                    // Exponential backoff: 50ms, 100ms, 200ms
                    let sleep_ms = ETXTBUSY_SLEEP_MS * (1 << (attempt - 1));
                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                }
                Err(err) => break Err(err),
            }
        }
    }

    async fn exec_program(
        &self,
        program: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let config = Config::get().await?;
        let program = program.to_executable();
        let redactions = config.redactions();
        let raw = self.raw(Some(task));
        let sandbox = self.build_sandbox_for_task(task, &config).await?;
        let env = if sandbox.is_active() {
            Settings::get().ensure_experimental("sandbox")?;
            &sandbox.filter_env(env)
        } else {
            env
        };
        // On Windows, when about to spawn a POSIX shell, resolve the program to
        // an absolute path *before* converting PATH for the child. Otherwise the
        // converted Unix-form PATH is also what Win32 CreateProcess uses to find
        // the program, and `bash` cannot be located in `/c/...:/c/...` entries.
        #[cfg(windows)]
        let program = resolve_posix_shell_program_path(&program, env).unwrap_or(program);
        let env = maybe_convert_env_for_msys_shell(Path::new(&program), env);
        let mut cmd = CmdLineRunner::new(program.clone())
            .args(args)
            .envs(env.as_ref())
            .redact(redactions.deref().clone())
            .raw(raw)
            .with_sandbox(sandbox);
        if raw && !redactions.is_empty() {
            if task.interactive && !task.raw && !Settings::get().raw {
                hint!(
                    "interactive_redactions",
                    "interactive tasks bypass redactions—secrets may appear in terminal output",
                    ""
                );
            } else {
                hint!(
                    "raw_redactions",
                    "--raw will prevent mise from being able to use redactions",
                    ""
                );
            }
        }
        let output = self.output(Some(task));
        cmd.with_pass_signals();
        match output {
            TaskOutput::Prefix => {
                if !task.silent.suppresses_stdout() {
                    cmd = cmd.with_on_stdout(|line| {
                        if console::colors_enabled() {
                            prefix_println!(prefix, "{line}\x1b[0m");
                        } else {
                            prefix_println!(prefix, "{line}");
                        }
                    });
                } else {
                    cmd = cmd.stdout(Stdio::null());
                }
                if !task.silent.suppresses_stderr() {
                    cmd = cmd.with_on_stderr(|line| {
                        if console::colors_enabled() {
                            self.eprint(task, prefix, &format!("{line}\x1b[0m"));
                        } else {
                            self.eprint(task, prefix, &line);
                        }
                    });
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::KeepOrder => {
                if !task.silent.suppresses_stdout() {
                    let state = self.output_handler.keep_order_state.clone();
                    let task_clone = task.clone();
                    let prefix_str = prefix.to_string();
                    cmd = cmd.with_on_stdout(move |line| {
                        state
                            .lock()
                            .unwrap()
                            .on_stdout(&task_clone, prefix_str.clone(), line);
                    });
                } else {
                    cmd = cmd.stdout(Stdio::null());
                }
                if !task.silent.suppresses_stderr() {
                    let state = self.output_handler.keep_order_state.clone();
                    let task_clone = task.clone();
                    let prefix_str = prefix.to_string();
                    cmd = cmd.with_on_stderr(move |line| {
                        state
                            .lock()
                            .unwrap()
                            .on_stderr(&task_clone, prefix_str.clone(), line);
                    });
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::Replacing => {
                // Replacing mode shows a progress indicator unless both streams are suppressed
                if task.silent.suppresses_stdout() {
                    cmd = cmd.stdout(Stdio::null());
                }
                if task.silent.suppresses_stderr() {
                    cmd = cmd.stderr(Stdio::null());
                }
                // Show progress indicator except when both streams are fully suppressed
                if !task.silent.suppresses_both() {
                    let pr = self.output_handler.get_or_init_task_pr(task);
                    cmd = cmd.with_pr_arc(pr);
                }
            }
            TaskOutput::Timed => {
                if !task.silent.suppresses_stdout() {
                    let timed_outputs = self.output_handler.timed_outputs.clone();
                    cmd = cmd.with_on_stdout(move |line| {
                        timed_outputs
                            .lock()
                            .unwrap()
                            .insert(prefix.to_string(), (SystemTime::now(), line));
                    });
                } else {
                    cmd = cmd.stdout(Stdio::null());
                }
                if !task.silent.suppresses_stderr() {
                    cmd = cmd.with_on_stderr(|line| {
                        if console::colors_enabled() {
                            self.eprint(task, prefix, &format!("{line}\x1b[0m"));
                        } else {
                            self.eprint(task, prefix, &line);
                        }
                    });
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::Silent => {
                cmd = cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            TaskOutput::Quiet | TaskOutput::Interleave => {
                if raw || redactions.is_empty() {
                    cmd = cmd.stdin(Stdio::inherit());
                    if !task.silent.suppresses_stdout() {
                        cmd = cmd.stdout(Stdio::inherit());
                    } else {
                        cmd = cmd.stdout(Stdio::null());
                    }
                    if !task.silent.suppresses_stderr() {
                        cmd = cmd.stderr(Stdio::inherit());
                    } else {
                        cmd = cmd.stderr(Stdio::null());
                    }
                }
            }
        }
        let dir = task_cwd(task, &config).await?;
        if !dir.exists() {
            self.eprint(
                task,
                prefix,
                &format!(
                    "{} task directory does not exist: {}",
                    style::eyellow("WARN"),
                    display_path(&dir)
                ),
            );
        }
        cmd = cmd.current_dir(dir);
        if self.dry_run {
            return Ok(());
        }
        let effective_timeout =
            task.timeout
                .as_ref()
                .and_then(|s| match duration::parse_duration(s) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        warn!("invalid timeout {:?} for task {}: {e}", s, task.name);
                        None
                    }
                });
        if let Some(timeout) = effective_timeout {
            cmd = cmd.with_timeout(timeout);
        }
        // Apply sandbox async (DNS resolution for macOS) before blocking execute
        cmd.apply_sandbox().await?;
        // cmd.execute() is blocking (calls cp.wait()), so use block_in_place
        // to avoid starving the tokio runtime while holding the TASK_RUNTIME_LOCK guard.
        tokio::task::block_in_place(|| cmd.execute())?;
        trace!("{prefix} exited successfully");
        Ok(())
    }

    #[cfg(unix)]
    fn is_text_file_busy(err: &Report) -> bool {
        err.chain().any(|cause| {
            if let Some(io_err) = cause.downcast_ref::<std::io::Error>()
                && let Some(code) = io_err.raw_os_error()
            {
                // ETXTBUSY (Text file busy) on Unix
                return code == Errno::ETXTBSY as i32;
            }
            false
        })
    }

    #[cfg(not(unix))]
    #[allow(unused_variables)]
    fn is_text_file_busy(err: &Report) -> bool {
        false
    }

    fn parse_confirm_default(default: &str) -> Result<bool> {
        match default.trim().to_ascii_lowercase().as_str() {
            "yes" | "y" | "true" => Ok(true),
            "no" | "n" | "false" => Ok(false),
            _ => Err(eyre!(
                "invalid task confirm default: {default:?}, expected one of yes/no/y/n/true/false"
            )),
        }
    }

    async fn check_confirmation(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &BTreeMap<String, String>,
    ) -> Result<()> {
        if let Some(confirm) = &task.confirm
            && !Settings::get().yes
        {
            let config_root = task.config_root.clone().unwrap_or_default();
            let mut tera = crate::tera::get_tera(Some(&config_root));
            let mut tera_ctx = task.tera_ctx(config).await?;

            // Add usage values from parsed environment
            let mut usage_ctx = std::collections::HashMap::new();
            for (key, value) in env {
                if let Some(usage_key) = key.strip_prefix("usage_") {
                    usage_ctx.insert(usage_key.to_string(), tera::Value::String(value.clone()));
                }
            }
            tera_ctx.insert("usage", &usage_ctx);

            let message = tera.render_str(confirm.message(), &tera_ctx)?;
            let default_yes = match confirm.default_value() {
                Some(default) => Self::parse_confirm_default(default)?,
                None => true, // keep backwards compatible default of yes if not specified
            };
            if !crate::ui::prompt::confirm_with_default(&message, default_yes).unwrap_or(false) {
                return Err(eyre!("aborted by user"));
            }
        }
        Ok(())
    }

    async fn parse_usage_spec_and_init_env(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &mut BTreeMap<String, String>,
        get_args: impl Fn() -> Vec<String>,
        extra_vars: Option<IndexMap<String, String>>,
    ) -> Result<()> {
        let (spec, _) = task
            .parse_usage_spec_with_vars(config, self.cd.clone(), env, extra_vars)
            .await?;
        // raw_args tasks (and `-- --help`/`-- -h` ad-hoc invocations) must
        // skip the usage parser so it can't intercept --help.
        if !task.should_bypass_usage_parser()
            && (!spec.cmd.args.is_empty()
                || !spec.cmd.flags.is_empty()
                || !spec.cmd.subcommands.is_empty())
        {
            let args: Vec<String> = get_args();
            trace!("Parsing usage spec for {:?}", args);
            // Pass env vars to Parser so it can resolve env= defaults in usage specs
            let env_map: std::collections::HashMap<String, String> =
                env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let po = usage::Parser::new(&spec)
                .with_env(env_map)
                .parse(&args)
                .map_err(|err| eyre!(err))?;
            for (k, v) in po.as_env() {
                trace!("Adding key {} value {} in env", k, v);
                env.insert(k, v);
            }
            // always export $usage_cmd when spec has subcommands so
            // shell scripts with `set -u` don't fail when none is chosen
            if !spec.cmd.subcommands.is_empty() {
                env.entry("usage_cmd".to_string()).or_default();
            }
            if let Some(subcmd) = subcommand_name_from_parse(&po.cmds) {
                trace!("Adding key usage_cmd value {} in env", subcmd);
                env.insert("usage_cmd".to_string(), subcmd);
            }
        } else {
            trace!("Usage spec has no args, flags, or subcommands");
        }

        Ok(())
    }
}

/// Check if a file can be executed directly by the OS without a shell wrapper.
/// On Unix, this checks the executable permission bit.
/// On Windows, this checks for a known executable extension (.bat, .ps1, etc.)
/// — shebang-only files need to be run through a shell.
fn can_execute_directly(path: &Path) -> bool {
    #[cfg(windows)]
    {
        // .ps1 files need pwsh -File, they can't be executed directly
        if path.extension().is_some_and(|e| e == "ps1") {
            return false;
        }
        crate::file::has_known_executable_extension(path)
    }
    #[cfg(not(windows))]
    {
        is_executable(path)
    }
}

/// Determine the shell from a file's extension.
/// e.g. `.ps1` → `["pwsh", "-File"]`
fn shell_from_extension(path: &Path) -> Option<Vec<String>> {
    match path.extension()?.to_str()? {
        "ps1" => Some(vec!["pwsh".to_string(), "-File".to_string()]),
        _ => None,
    }
}

/// On Windows, when about to spawn a POSIX shell whose PATH we are about to
/// convert to Unix form, resolve the program to its absolute path using the
/// pre-conversion (Windows-form) PATH from the task env.
///
/// Why: `Command::spawn` on Windows uses the *child* env's PATH (when set via
/// `.envs(...)`) to locate the program. If we hand it the converted
/// `/c/foo:/d/bar` PATH, Win32 cannot find `bash.exe`. Resolving here means
/// the child process gets an absolute path argument and does not need PATH
/// search at the OS level.
///
/// For `bash` specifically, prefer a real POSIX bash (Git Bash / MSYS2) over
/// the WSL launcher at `C:\Windows\System32\bash.exe`. The WSL launcher is on
/// PATH first when mise is invoked from PowerShell, and routing into WSL means
/// the spawned task body runs inside a separate Linux filesystem where
/// mise-managed Windows tools aren't visible. Resolution order:
///   1. `MISE_BASH_PATH` env var (explicit override).
///   2. Common Git Bash and MSYS2 install locations
///      (`C:\Program Files\Git\bin\bash.exe`,
///      `C:\Program Files (x86)\Git\bin\bash.exe`,
///      `%LOCALAPPDATA%\Programs\Git\bin\bash.exe`,
///      `C:\msys64\usr\bin\bash.exe`, `C:\msys32\usr\bin\bash.exe`).
///   3. `which::which_in_all` over the task env's PATH, picking the first
///      entry that isn't the WSL launcher. This rescues setups where a real
///      POSIX bash is on PATH but appears after `C:\Windows\System32`.
///
/// Returns `None` when the program is not a POSIX shell, the env has no PATH,
/// the PATH is already in Unix form (no `;` and no `\`, so no conversion will
/// fire), `which` finds nothing, or every PATH match for `bash` is the WSL
/// launcher — in those cases the caller keeps the original program string and
/// lets the stdlib spawn it (which will then fail loudly rather than silently
/// routing into WSL).
#[cfg(windows)]
fn resolve_posix_shell_program_path(
    program: &std::ffi::OsStr,
    env: &BTreeMap<String, String>,
) -> Option<std::ffi::OsString> {
    if !crate::path::is_posix_shell_program(Path::new(program)) {
        return None;
    }
    let path_val = env.get(&*crate::env::PATH_KEY)?;
    if !path_val.contains(';') && !path_val.contains('\\') {
        return None;
    }

    let is_bash = is_bash_basename(program);

    if is_bash {
        let override_path = env
            .get("MISE_BASH_PATH")
            .cloned()
            .or_else(|| std::env::var("MISE_BASH_PATH").ok())
            .filter(|s| !s.is_empty());
        if let Some(p) = override_path {
            let path = PathBuf::from(&p);
            if path.is_file() {
                return Some(path.into_os_string());
            }
            warn!("MISE_BASH_PATH={p} does not exist; falling back to other candidates");
        }
        for candidate in bash_candidates(env) {
            if candidate.is_file() {
                return Some(candidate.into_os_string());
            }
        }
    }

    let cwd = std::env::current_dir().ok()?;

    if is_bash {
        // For bash, walk every PATH match and pick the first that isn't the
        // WSL launcher. This rescues setups where a real POSIX bash sits later
        // on PATH than `C:\Windows\System32\bash.exe` — common under PowerShell
        // when Git Bash is installed somewhere `bash_candidates` doesn't probe.
        let mut all = which::which_in_all(program, Some(path_val.as_str()), cwd).ok()?;
        if let Some(p) = all.find(|p| !is_wsl_launcher_bash(p)) {
            return Some(p.into_os_string());
        }
        warn!(
            "no real POSIX bash found on PATH (only the WSL launcher) when resolving bash for a task; \
             install Git Bash or MSYS2, or set MISE_BASH_PATH to a real POSIX bash to silence this"
        );
        return None;
    }

    which::which_in(program, Some(path_val.as_str()), cwd)
        .ok()
        .map(|p| p.into_os_string())
}

/// Returns true if `program`'s basename (case-insensitive, `.exe` stripped) is `bash`.
/// More specific than [`crate::path::is_posix_shell_program`], which also accepts
/// sh/zsh/fish/ksh/dash. Used to scope the Windows bash-resolution heuristics so
/// they don't fire for other POSIX shells we might gain support for later.
#[cfg(windows)]
fn is_bash_basename(program: &std::ffi::OsStr) -> bool {
    crate::path::program_stem(Path::new(program)).as_deref() == Some("bash")
}

/// Common real-POSIX-bash install locations on Windows (Git Bash + MSYS2), in
/// preference order. Pure given `env` (no filesystem access), so the caller
/// stats each candidate. `MISE_BASH_PATH` covers anything outside this list,
/// including non-`C:` drive installs.
#[cfg(windows)]
fn bash_candidates(env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
    ];
    let local_appdata = env
        .get("LOCALAPPDATA")
        .cloned()
        .or_else(|| std::env::var("LOCALAPPDATA").ok());
    if let Some(local) = local_appdata.filter(|s| !s.is_empty()) {
        candidates.push(PathBuf::from(local).join(r"Programs\Git\bin\bash.exe"));
    }
    // MSYS2 standalone installs (default `C:\msys64`, 32-bit fallback `C:\msys32`).
    candidates.push(PathBuf::from(r"C:\msys64\usr\bin\bash.exe"));
    candidates.push(PathBuf::from(r"C:\msys32\usr\bin\bash.exe"));
    candidates
}

/// Returns true if `path` looks like the Windows-shipped WSL launcher rather
/// than a real POSIX bash. Matches `C:\Windows\System32\bash.exe` and the
/// `WindowsApps\bash.exe` shim that App Execution Aliases install. Both
/// dispatch into a WSL distribution's Linux userspace, which is the wrong
/// place to run a task that uses mise-managed Windows tools.
#[cfg(windows)]
fn is_wsl_launcher_bash(path: &Path) -> bool {
    let Some(s) = path.to_str() else {
        return false;
    };
    let lower = s.to_ascii_lowercase().replace('/', "\\");
    lower.ends_with(r"\windows\system32\bash.exe")
        || lower.contains(r"\microsoft\windowsapps\bash.exe")
}

/// On Windows, when spawning a POSIX-style shell (bash/sh/zsh/...) for a task, the
/// child needs PATH in MSYS Unix format — `/c/foo:/d/bar` rather than `C:\foo;D:\bar`.
/// PowerShell-launched mise inherits no `MSYSTEM`, so the conversion has to happen
/// here at the spawn boundary (driven by the target program), not in mise's own env.
///
/// The cfg-attribute pattern keeps the call site OS-agnostic and avoids cloning the
/// env on the common path (Windows + non-POSIX-shell, or any non-Windows host).
fn maybe_convert_env_for_msys_shell<'a>(
    program: &Path,
    env: &'a BTreeMap<String, String>,
) -> std::borrow::Cow<'a, BTreeMap<String, String>> {
    #[cfg(windows)]
    {
        if crate::path::is_posix_shell_program(program)
            && let Some(path_val) = env.get(&*crate::env::PATH_KEY)
            // Skip the clone+convert cycle when PATH is already in Unix form (no
            // `;` separator, no `\` to translate). This is the common case when
            // mise itself runs inside Git Bash and spawns another bash subshell.
            && (path_val.contains(';') || path_val.contains('\\'))
        {
            let converted = crate::path::windows_path_list_to_unix(path_val);
            let mut new_env = env.clone();
            new_env.insert((*crate::env::PATH_KEY).to_string(), converted);
            return std::borrow::Cow::Owned(new_env);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = program;
    }
    std::borrow::Cow::Borrowed(env)
}

/// Read the shebang from a file and parse it into a shell command.
/// e.g. `#!/usr/bin/env bash` → `["bash"]`
/// e.g. `#!/bin/bash` → `["/bin/bash"]`
fn shell_from_shebang(path: &Path) -> Option<Vec<String>> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;
    let shebang = first_line.strip_prefix("#!")?;
    let shebang = shebang.strip_prefix("/usr/bin/env -S").unwrap_or(shebang);
    let shebang = shebang.strip_prefix("/usr/bin/env").unwrap_or(shebang);
    let mut parts = shebang.split_whitespace();
    let shell = parts.next()?;
    // On Windows, convert unix paths like /bin/bash to just the binary name
    let shell = if cfg!(windows) {
        shell.rsplit('/').next().unwrap_or(shell)
    } else {
        shell
    };
    let args: Vec<String> = parts.map(|s| s.to_string()).collect();
    Some(once(shell.to_string()).chain(args).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_with_path(path: &str) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert((*crate::env::PATH_KEY).to_string(), path.to_string());
        env.insert("OTHER".to_string(), "unchanged".to_string());
        env
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_converts_for_bash() {
        let env = env_with_path(r"C:\Users\me\.rustup\bin;D:\tools\bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/c/Users/me/.rustup/bin:/d/tools/bin"
        );
        assert_eq!(out.get("OTHER").unwrap(), "unchanged");
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_skips_for_cmd() {
        let env = env_with_path(r"C:\Users\me\.rustup\bin;D:\tools\bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("cmd.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            r"C:\Users\me\.rustup\bin;D:\tools\bin"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_full_path_to_bash() {
        let env = env_with_path(r"C:\foo;D:\bar");
        let out =
            maybe_convert_env_for_msys_shell(Path::new(r"C:\Program Files\Git\bin\bash.exe"), &env);
        assert_eq!(out.get(&*crate::env::PATH_KEY).unwrap(), "/c/foo:/d/bar");
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_borrows_when_path_already_unix() {
        // PATH already in Unix form (no `;` and no `\`) — Cow stays Borrowed,
        // env is not cloned. Common when mise runs from Git Bash itself.
        let env = env_with_path("/c/foo:/d/bar:/usr/bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/c/foo:/d/bar:/usr/bin"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_borrows_when_path_missing() {
        // No PATH at all — also no clone.
        let mut env = BTreeMap::new();
        env.insert("OTHER".to_string(), "unchanged".to_string());
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_maybe_convert_env_for_msys_shell_noop_on_unix() {
        let env = env_with_path("/usr/bin:/bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash"), &env);
        assert_eq!(out.get(&*crate::env::PATH_KEY).unwrap(), "/usr/bin:/bin");
    }

    #[test]
    #[cfg(windows)]
    fn test_is_bash_basename_accepts_bash_variants() {
        use std::ffi::OsStr;
        assert!(is_bash_basename(OsStr::new("bash")));
        assert!(is_bash_basename(OsStr::new("bash.exe")));
        assert!(is_bash_basename(OsStr::new("BASH.EXE")));
        assert!(is_bash_basename(OsStr::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(is_bash_basename(OsStr::new("/usr/bin/bash")));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_bash_basename_rejects_other_shells() {
        use std::ffi::OsStr;
        assert!(!is_bash_basename(OsStr::new("sh")));
        assert!(!is_bash_basename(OsStr::new("zsh.exe")));
        assert!(!is_bash_basename(OsStr::new("fish")));
        assert!(!is_bash_basename(OsStr::new("dash")));
        assert!(!is_bash_basename(OsStr::new("cmd.exe")));
        assert!(!is_bash_basename(OsStr::new("bashfoo")));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_detects_system32() {
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\Windows\System32\bash.exe"
        )));
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\WINDOWS\system32\bash.exe"
        )));
        assert!(is_wsl_launcher_bash(Path::new(
            r"D:\Windows\System32\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_detects_windows_apps() {
        assert!(is_wsl_launcher_bash(Path::new(
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\bash.exe"
        )));
        // Forward slashes still match — `which::which_in` may produce them.
        assert!(is_wsl_launcher_bash(Path::new(
            "C:/Users/me/AppData/Local/Microsoft/WindowsApps/bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_is_wsl_launcher_bash_accepts_real_bash() {
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Program Files\Git\usr\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\msys64\usr\bin\bash.exe"
        )));
        assert!(!is_wsl_launcher_bash(Path::new(
            r"C:\Users\me\scoop\apps\git\current\bin\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_includes_program_files() {
        let env = BTreeMap::new();
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")));
        assert!(candidates.contains(&PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe")));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_includes_msys2() {
        let env = BTreeMap::new();
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(r"C:\msys64\usr\bin\bash.exe")));
        assert!(candidates.contains(&PathBuf::from(r"C:\msys32\usr\bin\bash.exe")));
    }

    #[test]
    #[cfg(windows)]
    fn test_bash_candidates_uses_localappdata_from_env() {
        let mut env = BTreeMap::new();
        env.insert(
            "LOCALAPPDATA".to_string(),
            r"C:\Users\me\AppData\Local".to_string(),
        );
        let candidates = bash_candidates(&env);
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\me\AppData\Local\Programs\Git\bin\bash.exe"
        )));
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_uses_mise_bash_path_override() {
        // SAFETY: tests in this module run sequentially within the cargo test runner;
        // env mutation is scoped via a guard.
        let tmp = tempfile::tempdir().expect("tempdir");
        let bash_path = tmp.path().join("custom-bash.exe");
        std::fs::write(&bash_path, b"").expect("write fake bash");

        let mut env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
        env.insert(
            "MISE_BASH_PATH".to_string(),
            bash_path.to_string_lossy().into_owned(),
        );

        let resolved = resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
            .expect("override should resolve");
        assert_eq!(PathBuf::from(&resolved), bash_path);
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_skips_when_not_posix_shell() {
        let env = env_with_path(r"C:\Windows\System32");
        assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("cmd.exe"), &env).is_none());
        assert!(
            resolve_posix_shell_program_path(std::ffi::OsStr::new("notepad.exe"), &env).is_none()
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_resolve_posix_shell_program_path_skips_when_path_already_unix() {
        let env = env_with_path("/c/foo:/d/bar");
        assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env).is_none());
    }
}
