use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, env_directive::EnvDirective};
use crate::file::{display_path, is_executable};
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_list::split_task_spec;
use crate::task::task_output::{TaskOutput, trunc};
use crate::task::task_output_handler::OutputHandler;
use crate::task::task_source_checker::{save_checksum, sources_are_fresh, task_cwd};
use crate::task::task_trace::{TaskTraceReport, TaskTraceStage};
use crate::task::{Deps, FailedTasks, GetMatchingExt, Silent, Task};
use crate::toolset::env_cache::CachedEnv;
use crate::ui::{style, time};
use duct::IntoExecutablePath;
use eyre::{Report, Result, ensure, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
#[cfg(unix)]
use nix::errno::Errno;
use std::collections::BTreeMap;
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock as Lazy, Mutex as StdMutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::sync::{RwLock, mpsc, oneshot};
use xx::file;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioDirective {
    Unchanged,
    Inherit,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskStdioPlan {
    stdin: StdioDirective,
    stdout: StdioDirective,
    stderr: StdioDirective,
}

impl Default for TaskStdioPlan {
    fn default() -> Self {
        Self {
            // CmdLineRunner::new defaults:
            // stdin = null, stdout = piped, stderr = piped
            stdin: StdioDirective::Unchanged,
            stdout: StdioDirective::Unchanged,
            stderr: StdioDirective::Unchanged,
        }
    }
}

static INTERACTIVE_EXEC_GATE: Lazy<RwLock<()>> = Lazy::new(|| RwLock::new(()));

#[cfg(unix)]
struct InteractiveTerminalGuard {
    saved: Option<nix::sys::termios::Termios>,
}

#[cfg(unix)]
impl InteractiveTerminalGuard {
    fn new(enabled: bool) -> Self {
        let saved = enabled
            .then(|| nix::sys::termios::tcgetattr(std::io::stdin()).ok())
            .flatten();
        Self { saved }
    }
}

#[cfg(unix)]
impl Drop for InteractiveTerminalGuard {
    fn drop(&mut self) {
        if let Some(termios) = &self.saved {
            let _ = nix::sys::termios::tcsetattr(
                std::io::stdin(),
                nix::sys::termios::SetArg::TCSANOW,
                termios,
            );
        }
        let _ = console::Term::stderr().show_cursor();
    }
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
        }
    }

    pub fn is_stopping(&self) -> bool {
        crate::ui::ctrlc::was_interrupted() || !self.failed_tasks.lock().unwrap().is_empty()
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

    pub fn task_timings(&self) -> bool {
        let output_mode = self.output_handler.output(None);
        self.timings
            || Settings::get().task.timings.unwrap_or(
                output_mode == TaskOutput::Prefix
                    || output_mode == TaskOutput::Timed
                    || output_mode == TaskOutput::KeepOrder,
            )
    }

    pub async fn run_task_sched_with_trace(
        &self,
        task: &Task,
        config: &Arc<Config>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorEntry);
        let prefix = task.estyled_prefix();
        let total_start = std::time::Instant::now();
        if Settings::get().task.skip.contains(&task.name) {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "skipping task");
            }
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorSkipTaskSkip);
            return Ok(());
        }
        let sources_fresh = if self.force {
            false
        } else {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorSourcesCheck);
            sources_are_fresh(task, config).await.map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorSourcesCheck)
            })?
        };
        if sources_fresh {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorSkipSourcesFresh);
            return Ok(());
        }

        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorToolsCollect);
        let mut tools = self.tool.clone();
        for (k, v) in &task.tools {
            tools.push(format!("{k}@{v}").parse().map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorToolsParse)
            })?);
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
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorToolsetBuildStart);
        let ts = self
            .context_builder
            .build_toolset_for_task(config, task, task_cf, &tools)
            .await
            .map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorToolsetBuild)
            })?;
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorToolsetBuildOk);

        trace!(
            "task {} ToolsetBuilder::build took {}ms",
            task.name,
            ts_build_start.elapsed().as_millis()
        );
        let env_render_start = std::time::Instant::now();

        // Build environment - either from task's config file context or standard way
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorEnvRenderStart);
        let (mut env, task_env, extra_vars) = if let Some(task_cf) = task_cf {
            self.context_builder
                .resolve_task_env_with_config(config, task, task_cf, &ts)
                .await
                .map_err(|err| {
                    wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorEnvRender)
                })?
        } else {
            // Fallback to standard behavior
            let (env, task_env) = task.render_env(config, &ts).await.map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorEnvRender)
            })?;
            (env, task_env, None)
        };
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorEnvRenderOk);

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
        if let Some(root) = config.project_root.clone().or(task.config_root.clone()) {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
        }
        env.insert("MISE_TASK_NAME".into(), task.name.clone());
        let task_file = task
            .file_path(config)
            .await
            .map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorTaskFile)
            })?
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

        let timer = std::time::Instant::now();

        if let Some(file) = task.file_path(config).await.map_err(|err| {
            wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorFilePath)
        })? {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecFileStart);
            let exec_start = std::time::Instant::now();
            self.exec_file(
                config,
                &file,
                task,
                &env,
                &prefix,
                extra_vars.clone(),
                task_trace.as_deref_mut(),
            )
            .await
            .map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorExecFile)
            })?;
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecFileOk);
            trace!(
                "task {} exec_file took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        } else {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorRunEntriesPrepare);
            let rendered_run_scripts = task
                .render_run_scripts_with_args(
                    config,
                    self.cd.clone(),
                    &task.args,
                    &env,
                    extra_vars.clone(),
                )
                .await
                .map_err(|err| {
                    wrap_task_trace_error(
                        &mut task_trace,
                        err,
                        TaskTraceStage::ExecutorRunEntriesRender,
                    )
                })?;

            let get_args = || {
                [String::new()]
                    .iter()
                    .chain(task.args.iter())
                    .cloned()
                    .collect()
            };
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorUsageParse);
            self.parse_usage_spec_and_init_env(config, task, &mut env, get_args, extra_vars)
                .await
                .map_err(|err| {
                    wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorUsage)
                })?;

            // Check confirmation after usage args are parsed
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorConfirm);
            self.check_confirmation(config, task, &env)
                .await
                .map_err(|err| {
                    wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorConfirm)
                })?;

            let exec_start = std::time::Instant::now();
            self.exec_task_run_entries(
                config,
                task,
                (&env, &task_env),
                &prefix,
                rendered_run_scripts,
                sched_tx,
                task_trace.as_deref_mut(),
            )
            .await
            .map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorRunEntriesExec)
            })?;
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorRunEntriesOk);
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

        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorChecksumSave);
        save_checksum(task, config).await.map_err(|err| {
            wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorChecksum)
        })?;
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorDone);

        Ok(())
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
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorRunEntriesStart);
        let (env, task_env) = full_env;
        use crate::task::RunEntry;
        let mut script_iter = rendered_scripts.into_iter();
        for (idx, entry) in task.run().iter().enumerate() {
            match entry {
                RunEntry::Script(_) => {
                    mark_task_trace(
                        &mut task_trace,
                        TaskTraceStage::ExecutorRunEntryScript { index: idx },
                    );
                    if let Some((script, args)) = script_iter.next() {
                        self.exec_script(
                            &script,
                            &args,
                            task,
                            env,
                            prefix,
                            task_trace.as_deref_mut(),
                        )
                        .await
                        .map_err(|err| {
                            wrap_task_trace_error(
                                &mut task_trace,
                                err,
                                TaskTraceStage::ExecutorRunEntryScriptExec,
                            )
                        })?;
                    }
                }
                RunEntry::SingleTask { task: spec } => {
                    mark_task_trace(
                        &mut task_trace,
                        TaskTraceStage::ExecutorRunEntrySingle { index: idx },
                    );
                    let resolved_spec = crate::task::resolve_task_pattern(spec, Some(task));
                    self.inject_and_wait(
                        config,
                        task,
                        format!("run entry {{ task = \"{spec}\" }}"),
                        &[resolved_spec],
                        task_env,
                        task.interactive_owner,
                        sched_tx.clone(),
                        task_trace.as_deref_mut(),
                    )
                    .await
                    .map_err(|err| {
                        wrap_task_trace_error(
                            &mut task_trace,
                            err,
                            TaskTraceStage::ExecutorRunEntrySingleExec,
                        )
                    })?;
                }
                RunEntry::TaskGroup { tasks } => {
                    mark_task_trace(
                        &mut task_trace,
                        TaskTraceStage::ExecutorRunEntryGroup { index: idx },
                    );
                    let resolved_tasks: Vec<String> = tasks
                        .iter()
                        .map(|t| crate::task::resolve_task_pattern(t, Some(task)))
                        .collect();
                    self.inject_and_wait(
                        config,
                        task,
                        format!("run entry {{ tasks = [{}] }}", tasks.join(", ")),
                        &resolved_tasks,
                        task_env,
                        task.interactive_owner,
                        sched_tx.clone(),
                        task_trace.as_deref_mut(),
                    )
                    .await
                    .map_err(|err| {
                        wrap_task_trace_error(
                            &mut task_trace,
                            err,
                            TaskTraceStage::ExecutorRunEntryGroupExec,
                        )
                    })?;
                }
            }
        }
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorRunEntriesDone);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn inject_and_wait(
        &self,
        config: &Arc<Config>,
        caller_task: &Task,
        caller_entry: String,
        specs: &[String],
        task_env: &[(String, String)],
        interactive_owner: Option<u64>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(
            &mut task_trace,
            TaskTraceStage::ExecutorInjectStart {
                specs: specs.join(","),
            },
        );
        use crate::task::TaskLoadContext;
        trace!("inject start: {}", specs.join(", "));
        // Build tasks list from specs
        // Create a TaskLoadContext from the specs to ensure project tasks are loaded
        let ctx = TaskLoadContext::from_patterns(specs.iter().map(|s| {
            let (name, _) = split_task_spec(s);
            name
        }));
        let tasks = config.tasks_with_context(Some(&ctx)).await.map_err(|err| {
            wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorInjectLoad)
        })?;
        let tasks_map: BTreeMap<String, Task> = tasks
            .iter()
            .flat_map(|(_, t)| {
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
            let frame_reason = format!("called by `{}` via {}", caller_task.name, caller_entry);
            if let Some(trace) = task_trace.as_deref_mut() {
                // Keep unresolved specs visible in task_path when matching fails.
                trace.add_task_name_frame_with_reason(
                    name.to_string(),
                    None,
                    Some(frame_reason.clone()),
                );
            }
            let matches = tasks_map.get_matching(name).map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorInjectMatch)
            })?;
            ensure!(!matches.is_empty(), "task not found: {}", name);
            for t in matches {
                let mut t = (*t).clone();
                t.args = args.clone();
                t.interactive_owner = interactive_owner;
                if self.skip_deps {
                    t.depends.clear();
                    t.depends_post.clear();
                    t.wait_for.clear();
                }
                if let Some(trace) = task_trace.as_deref_mut() {
                    trace.add_task_frame_with_reason(&t, Some(frame_reason.clone()));
                }
                to_run.push(t);
            }
        }
        let sub_deps = Deps::new(config, to_run).await.map_err(|err| {
            wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorInjectDeps)
        })?;
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
                mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorInjectStopRequested);
                // Clean up the dependency graph to ensure completion
                let mut deps = sub_deps.lock().await;
                let tasks_to_remove: Vec<Task> = deps.all().cloned().collect();
                for task in tasks_to_remove {
                    deps.remove(&task);
                }
                drop(deps);
                // Give a short time for the spawned task to finish cleanly
                let _ = tokio::time::timeout(Duration::from_millis(100), done_rx).await;
                return Err(wrap_task_trace_error(
                    &mut task_trace,
                    eyre!("task sequence aborted due to failure"),
                    TaskTraceStage::ExecutorInjectStopRequested,
                ));
            }

            // Try to receive the done signal with a short timeout
            match tokio::time::timeout(Duration::from_millis(100), &mut done_rx).await {
                Ok(Ok(())) => {
                    trace!("inject_and_wait: received done signal");
                    mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorInjectDoneSignal);
                    break;
                }
                Ok(Err(e)) => {
                    return Err(wrap_task_trace_error(
                        &mut task_trace,
                        eyre!(e),
                        TaskTraceStage::ExecutorInjectDoneChannel,
                    ));
                }
                Err(_) => {
                    // Timeout, check again if we should stop
                    continue;
                }
            }
        }

        // Final check if we failed during the execution
        if self.is_stopping() && !self.continue_on_error {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorInjectFinalStop);
            return Err(wrap_task_trace_error(
                &mut task_trace,
                eyre!("task sequence aborted due to failure"),
                TaskTraceStage::ExecutorInjectFinalStop,
            ));
        }

        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorInjectOk);
        Ok(())
    }

    async fn exec_script(
        &self,
        script: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecScriptStart);
        let config = Config::get().await.map_err(|err| {
            wrap_task_trace_error(
                &mut task_trace,
                err,
                TaskTraceStage::ExecutorExecScriptConfig,
            )
        })?;
        let script = script.trim_start();
        let user_command = format!("{script} {args}", args = args.join(" "))
            .trim()
            .to_string();
        if let Some(trace) = task_trace.as_deref_mut() {
            trace.set_command(user_command.clone());
        }
        if !self.quiet(Some(task)) {
            let cmd = format!("$ {user_command}");
            let msg = style::ebold(trunc(prefix, config.redact(&cmd).trim()))
                .bright()
                .to_string();
            self.eprint(task, prefix, &msg)
        }

        if script.starts_with("#!") {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecScriptShebang);
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("script");
            tokio::fs::write(&file, script.as_bytes()).await?;
            file::make_executable(&file)?;
            self.exec_with_text_file_busy_retry(
                &file,
                args,
                task,
                env,
                prefix,
                task_trace.as_deref_mut(),
            )
            .await
        } else {
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecScriptInline);
            let (program, args) = self.get_cmd_program_and_args(script, task, args)?;
            self.exec_program(&program, &args, task, env, prefix, task_trace)
                .await
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

    #[allow(clippy::too_many_arguments)]
    async fn exec_file(
        &self,
        config: &Arc<Config>,
        file: &Path,
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
        extra_vars: Option<IndexMap<String, String>>,
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecFilePrepare);
        let mut env = env.clone();
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();
        let user_command = format!("{} {}", display_path(file), args.join(" "))
            .trim()
            .to_string();
        if let Some(trace) = task_trace.as_deref_mut() {
            trace.set_command(user_command.clone());
        }
        let get_args = || once(command.clone()).chain(args.clone()).collect_vec();
        self.parse_usage_spec_and_init_env(config, task, &mut env, get_args, extra_vars)
            .await
            .map_err(|err| {
                wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorExecFileUsage)
            })?;

        // Check confirmation after usage args are parsed
        self.check_confirmation(config, task, &env)
            .await
            .map_err(|err| {
                wrap_task_trace_error(
                    &mut task_trace,
                    err,
                    TaskTraceStage::ExecutorExecFileConfirm,
                )
            })?;

        if !self.quiet(Some(task)) {
            let cmd = style::ebold(format!("$ {user_command}"))
                .bright()
                .to_string();
            let cmd = trunc(prefix, config.redact(&cmd).trim());
            self.eprint(task, prefix, &cmd);
        }

        self.exec(file, &args, task, &env, prefix, task_trace).await
    }

    async fn exec(
        &self,
        file: &Path,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
        task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        let (program, args) = self.get_file_program_and_args(file, task, args)?;
        self.exec_program(&program, &args, task, env, prefix, task_trace)
            .await
    }

    async fn exec_with_text_file_busy_retry(
        &self,
        file: &Path,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        const ETXTBUSY_RETRIES: usize = 3;
        const ETXTBUSY_SLEEP_MS: u64 = 50;

        let mut attempt = 0;
        loop {
            match self
                .exec(file, args, task, env, prefix, task_trace.as_deref_mut())
                .await
            {
                Ok(()) => break Ok(()),
                Err(err) if Self::is_text_file_busy(&err) && attempt < ETXTBUSY_RETRIES => {
                    attempt += 1;
                    mark_task_trace(
                        &mut task_trace,
                        TaskTraceStage::ExecutorExecRetryEtxtbusy { attempt },
                    );
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
        mut task_trace: Option<&mut TaskTraceReport>,
    ) -> Result<()> {
        mark_task_trace(
            &mut task_trace,
            TaskTraceStage::ExecutorExecProgramStart {
                program: program.to_string(),
            },
        );
        let config = Config::get().await.map_err(|err| {
            wrap_task_trace_error(
                &mut task_trace,
                err,
                TaskTraceStage::ExecutorExecProgramConfig,
            )
        })?;
        let program = program.to_executable();
        if let Some(trace) = task_trace.as_deref_mut() {
            let user_command = once(display_path(PathBuf::from(&program)))
                .chain(args.iter().cloned())
                .join(" ");
            trace.set_command(user_command);
        }
        let redactions = config.redactions();
        let raw = self.raw(Some(task));
        let requested_output = self.output(Some(task));
        let policy = task_execution_policy(
            requested_output,
            &task.silent,
            raw,
            redactions.is_empty(),
            task.interactive,
        );
        if policy.warn_interactive_redactions {
            hint!(
                "interactive_redactions",
                "interactive tasks stream output directly; live redaction is not applied",
                ""
            );
        }
        let mut cmd = CmdLineRunner::new(program.clone())
            .args(args)
            .envs(env)
            .redact(redactions.deref().clone())
            .raw(raw);
        if policy.warn_raw_redactions {
            hint!(
                "raw_redactions",
                "--raw will prevent mise from being able to use redactions",
                ""
            );
        }
        cmd.with_pass_signals();
        match policy.stdout {
            StreamCallback::None => {}
            StreamCallback::Prefix => {
                let output_handler = self.output_handler.clone();
                let task_clone = task.clone();
                let prefix_str = prefix.to_string();
                cmd = cmd.with_on_stdout(move |line| {
                    output_handler.on_prefix_stdout(&task_clone, prefix_str.clone(), line);
                });
            }
            StreamCallback::KeepOrder => {
                let state = self.output_handler.keep_order_state.clone();
                let task_clone = task.clone();
                let prefix_str = prefix.to_string();
                cmd = cmd.with_on_stdout(move |line| {
                    state
                        .lock()
                        .unwrap()
                        .on_stdout(&task_clone, prefix_str.clone(), line);
                });
            }
            StreamCallback::Timed => {
                let timed_outputs = self.output_handler.timed_outputs.clone();
                cmd = cmd.with_on_stdout(move |line| {
                    timed_outputs
                        .lock()
                        .unwrap()
                        .insert(prefix.to_string(), (SystemTime::now(), line));
                });
            }
        }
        match policy.stderr {
            StreamCallback::None => {}
            StreamCallback::Prefix => {
                let output_handler = self.output_handler.clone();
                let task_clone = task.clone();
                let prefix_str = prefix.to_string();
                cmd = cmd.with_on_stderr(move |line| {
                    output_handler.on_prefix_stderr(&task_clone, prefix_str.clone(), line);
                });
            }
            StreamCallback::KeepOrder => {
                let state = self.output_handler.keep_order_state.clone();
                let task_clone = task.clone();
                let prefix_str = prefix.to_string();
                cmd = cmd.with_on_stderr(move |line| {
                    state
                        .lock()
                        .unwrap()
                        .on_stderr(&task_clone, prefix_str.clone(), line);
                });
            }
            StreamCallback::Timed => {
                cmd = cmd.with_on_stderr(|line| {
                    let line = format_timed_stderr_line(&line, console::colors_enabled());
                    self.eprint(task, prefix, &line);
                });
            }
        }
        if policy.use_progress_report {
            let pr = self.output_handler.replacing_report(task);
            cmd = cmd.with_pr_arc(pr);
        }
        cmd = apply_task_stdio_plan(cmd, policy.stdio);
        let dir = task_cwd(task, &config).await.map_err(|err| {
            wrap_task_trace_error(&mut task_trace, err, TaskTraceStage::ExecutorExecProgramCwd)
        })?;
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
            mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecProgramDryRun);
            return Ok(());
        }
        let effective_timeout = task.timeout.as_ref().and_then(|s| {
            crate::duration::parse_duration(s).map_or_else(
                |e| {
                    warn!("invalid timeout {:?} for task {}: {e}", s, task.name);
                    None
                },
                Some,
            )
        });
        if let Some(timeout) = effective_timeout {
            cmd = cmd.with_timeout(timeout);
        }
        if task.interactive {
            let _execution_guard = INTERACTIVE_EXEC_GATE.write().await;
            #[cfg(unix)]
            let _interactive_terminal_guard = InteractiveTerminalGuard::new(true);
            cmd.execute().map_err(|err| {
                wrap_task_trace_error(
                    &mut task_trace,
                    err,
                    TaskTraceStage::ExecutorExecProgramExecute,
                )
            })?;
        } else {
            let _execution_guard = INTERACTIVE_EXEC_GATE.read().await;
            cmd.execute().map_err(|err| {
                wrap_task_trace_error(
                    &mut task_trace,
                    err,
                    TaskTraceStage::ExecutorExecProgramExecute,
                )
            })?;
        }
        mark_task_trace(&mut task_trace, TaskTraceStage::ExecutorExecProgramOk);
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

    async fn check_confirmation(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &BTreeMap<String, String>,
    ) -> Result<()> {
        if let Some(confirm_template) = &task.confirm
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

            let message = tera.render_str(confirm_template, &tera_ctx)?;
            ensure_task_confirmation(crate::ui::confirm(&message).unwrap_or(false))?;
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
        if !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty() {
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
        } else {
            trace!("Usage spec has no args or flags");
        }

        Ok(())
    }
}

fn apply_task_stdio_plan<'a>(mut cmd: CmdLineRunner<'a>, plan: TaskStdioPlan) -> CmdLineRunner<'a> {
    match plan.stdin {
        StdioDirective::Unchanged => {}
        StdioDirective::Inherit => cmd = cmd.stdin(Stdio::inherit()),
        StdioDirective::Null => cmd = cmd.stdin(Stdio::null()),
    }
    match plan.stdout {
        StdioDirective::Unchanged => {}
        StdioDirective::Inherit => cmd = cmd.stdout(Stdio::inherit()),
        StdioDirective::Null => cmd = cmd.stdout(Stdio::null()),
    }
    match plan.stderr {
        StdioDirective::Unchanged => {}
        StdioDirective::Inherit => cmd = cmd.stderr(Stdio::inherit()),
        StdioDirective::Null => cmd = cmd.stderr(Stdio::null()),
    }
    cmd
}

fn mark_task_trace(task_trace: &mut Option<&mut TaskTraceReport>, stage: TaskTraceStage) {
    if let Some(trace) = task_trace.as_deref_mut() {
        trace.mark(stage);
    }
}

fn wrap_task_trace_error(
    task_trace: &mut Option<&mut TaskTraceReport>,
    err: Report,
    stage: TaskTraceStage,
) -> Report {
    if let Some(trace) = task_trace.as_deref_mut() {
        trace.set_exit_code(crate::errors::Error::get_exit_status(&err));
        trace.wrap_error(err, stage)
    } else {
        err
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamCallback {
    None,
    Prefix,
    KeepOrder,
    Timed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TaskExecutionPolicy {
    output: TaskOutput,
    stdio: TaskStdioPlan,
    stdout: StreamCallback,
    stderr: StreamCallback,
    use_progress_report: bool,
    warn_interactive_redactions: bool,
    warn_raw_redactions: bool,
}

fn format_timed_stderr_line(line: &str, colors_enabled: bool) -> String {
    if colors_enabled {
        format!("{line}\x1b[0m")
    } else {
        line.to_string()
    }
}

fn ensure_task_confirmation(confirmed: bool) -> Result<()> {
    if confirmed {
        Ok(())
    } else {
        Err(eyre!("aborted by user"))
    }
}

fn task_execution_policy(
    requested_output: TaskOutput,
    silent: &Silent,
    raw: bool,
    redactions_empty: bool,
    interactive: bool,
) -> TaskExecutionPolicy {
    let output = if interactive {
        TaskOutput::Interleave
    } else {
        requested_output
    };

    let mut policy = TaskExecutionPolicy {
        output,
        stdio: task_stdio_plan(output, silent, raw, redactions_empty, interactive),
        stdout: StreamCallback::None,
        stderr: StreamCallback::None,
        use_progress_report: false,
        warn_interactive_redactions: interactive && !redactions_empty,
        warn_raw_redactions: raw && !redactions_empty,
    };

    match output {
        TaskOutput::Prefix => {
            if !silent.suppresses_stdout() {
                policy.stdout = StreamCallback::Prefix;
            }
            if !silent.suppresses_stderr() {
                policy.stderr = StreamCallback::Prefix;
            }
        }
        TaskOutput::KeepOrder => {
            if !silent.suppresses_stdout() {
                policy.stdout = StreamCallback::KeepOrder;
            }
            if !silent.suppresses_stderr() {
                policy.stderr = StreamCallback::KeepOrder;
            }
        }
        TaskOutput::Replacing => {
            policy.use_progress_report = !silent.suppresses_both();
        }
        TaskOutput::Timed => {
            if !silent.suppresses_stdout() {
                policy.stdout = StreamCallback::Timed;
            }
            if !silent.suppresses_stderr() {
                policy.stderr = StreamCallback::Timed;
            }
        }
        TaskOutput::Silent | TaskOutput::Quiet | TaskOutput::Interleave => {}
    }

    policy
}

fn task_stdio_plan(
    output: TaskOutput,
    silent: &Silent,
    raw: bool,
    redactions_empty: bool,
    interactive: bool,
) -> TaskStdioPlan {
    let mut plan = TaskStdioPlan::default();

    if interactive {
        return TaskStdioPlan {
            stdin: StdioDirective::Inherit,
            stdout: StdioDirective::Inherit,
            stderr: StdioDirective::Inherit,
        };
    }

    match output {
        TaskOutput::Prefix | TaskOutput::KeepOrder | TaskOutput::Replacing | TaskOutput::Timed => {
            // Keep stdin attached to the terminal so interactive commands still work
            // even when stdout/stderr are captured for structured output modes.
            plan.stdin = StdioDirective::Inherit;
            if silent.suppresses_stdout() {
                plan.stdout = StdioDirective::Null;
            }
            if silent.suppresses_stderr() {
                plan.stderr = StdioDirective::Null;
            }
        }
        TaskOutput::Silent => {
            plan.stdout = StdioDirective::Null;
            plan.stderr = StdioDirective::Null;
        }
        TaskOutput::Quiet | TaskOutput::Interleave => {
            if raw || redactions_empty {
                plan.stdin = StdioDirective::Inherit;
                plan.stdout = if silent.suppresses_stdout() {
                    StdioDirective::Null
                } else {
                    StdioDirective::Inherit
                };
                plan.stderr = if silent.suppresses_stderr() {
                    StdioDirective::Null
                } else {
                    StdioDirective::Inherit
                };
            }
        }
    }

    plan
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
    use super::{
        StdioDirective, StreamCallback, TaskExecutionPolicy, TaskStdioPlan,
        format_timed_stderr_line, task_execution_policy, task_stdio_plan,
    };
    use crate::task::Silent;
    use crate::task::task_output::TaskOutput;
    use eyre::eyre;
    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn exit_status_with_code(code: i32) -> ExitStatus {
        #[cfg(unix)]
        {
            ExitStatus::from_raw(code << 8)
        }
        #[cfg(windows)]
        {
            ExitStatus::from_raw(code as u32)
        }
    }

    fn assert_plan(
        plan: TaskStdioPlan,
        stdin: StdioDirective,
        stdout: StdioDirective,
        stderr: StdioDirective,
    ) {
        assert_eq!(
            (plan.stdin, plan.stdout, plan.stderr),
            (stdin, stdout, stderr)
        );
    }

    #[test]
    fn test_interleave_without_redactions_inherits_all_streams() {
        let plan = task_stdio_plan(TaskOutput::Interleave, &Silent::Off, false, true, false);
        assert_plan(
            plan,
            StdioDirective::Inherit,
            StdioDirective::Inherit,
            StdioDirective::Inherit,
        );
    }

    #[test]
    fn test_interleave_with_redactions_keeps_default_streams_for_filtering() {
        let plan = task_stdio_plan(TaskOutput::Interleave, &Silent::Off, false, false, false);
        assert_plan(
            plan,
            StdioDirective::Unchanged,
            StdioDirective::Unchanged,
            StdioDirective::Unchanged,
        );
    }

    #[test]
    fn test_quiet_with_stdout_suppressed_keeps_stdin_and_stderr_interactive() {
        let plan = task_stdio_plan(TaskOutput::Quiet, &Silent::Stdout, false, true, false);
        assert_plan(
            plan,
            StdioDirective::Inherit,
            StdioDirective::Null,
            StdioDirective::Inherit,
        );
    }

    #[test]
    fn test_quiet_with_redactions_keeps_default_streams_for_filtering_even_when_silent_flags_set() {
        let plan = task_stdio_plan(TaskOutput::Quiet, &Silent::Stderr, false, false, false);
        assert_plan(
            plan,
            StdioDirective::Unchanged,
            StdioDirective::Unchanged,
            StdioDirective::Unchanged,
        );
    }

    #[test]
    fn test_quiet_with_raw_uses_inherited_stdio_even_with_redactions() {
        let plan = task_stdio_plan(TaskOutput::Quiet, &Silent::Off, true, false, false);
        assert_plan(
            plan,
            StdioDirective::Inherit,
            StdioDirective::Inherit,
            StdioDirective::Inherit,
        );
    }

    #[test]
    fn test_interleave_with_silent_bool_true_nulls_stdout_and_stderr() {
        let plan = task_stdio_plan(
            TaskOutput::Interleave,
            &Silent::Bool(true),
            false,
            true,
            false,
        );
        assert_plan(
            plan,
            StdioDirective::Inherit,
            StdioDirective::Null,
            StdioDirective::Null,
        );
    }

    #[test]
    fn test_silent_output_nulls_both_streams_and_leaves_stdin_default() {
        let plan = task_stdio_plan(TaskOutput::Silent, &Silent::Off, false, true, false);
        assert_plan(
            plan,
            StdioDirective::Unchanged,
            StdioDirective::Null,
            StdioDirective::Null,
        );
    }

    #[test]
    fn test_structured_outputs_keep_stdin_interactive() {
        for output in [
            TaskOutput::Prefix,
            TaskOutput::KeepOrder,
            TaskOutput::Replacing,
            TaskOutput::Timed,
        ] {
            let plan = task_stdio_plan(output, &Silent::Off, false, true, false);
            assert_eq!(plan.stdin, StdioDirective::Inherit, "{output:?}");
        }
    }

    #[test]
    fn test_structured_outputs_apply_silent_stream_suppression() {
        for output in [
            TaskOutput::Prefix,
            TaskOutput::KeepOrder,
            TaskOutput::Replacing,
            TaskOutput::Timed,
        ] {
            let stdout_suppressed = task_stdio_plan(output, &Silent::Stdout, false, true, false);
            assert_plan(
                stdout_suppressed,
                StdioDirective::Inherit,
                StdioDirective::Null,
                StdioDirective::Unchanged,
            );

            let stderr_suppressed = task_stdio_plan(output, &Silent::Stderr, false, true, false);
            assert_plan(
                stderr_suppressed,
                StdioDirective::Inherit,
                StdioDirective::Unchanged,
                StdioDirective::Null,
            );
        }
    }

    #[test]
    fn test_regression_prefix_should_keep_tty_stdin_for_interactive_tasks() {
        let plan = task_stdio_plan(TaskOutput::Prefix, &Silent::Off, false, true, false);
        assert_eq!(plan.stdin, StdioDirective::Inherit);
    }

    #[test]
    fn test_interactive_always_inherits_all_streams() {
        // Matrix: S01/S02/S03/S04/S05/S06/S14 (C2, C6)
        for output in [
            TaskOutput::Prefix,
            TaskOutput::KeepOrder,
            TaskOutput::Replacing,
            TaskOutput::Timed,
            TaskOutput::Interleave,
            TaskOutput::Quiet,
        ] {
            let plan = task_stdio_plan(output, &Silent::Off, false, false, true);
            assert_plan(
                plan,
                StdioDirective::Inherit,
                StdioDirective::Inherit,
                StdioDirective::Inherit,
            );
        }
    }

    fn assert_policy(
        policy: TaskExecutionPolicy,
        output: TaskOutput,
        stdout: StreamCallback,
        stderr: StreamCallback,
    ) {
        assert_eq!(policy.output, output);
        assert_eq!(policy.stdout, stdout);
        assert_eq!(policy.stderr, stderr);
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum IoEvent {
        Stdout(String),
        Stderr(String),
        Prompt(String),
        Eof,
        Signal(String),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum IoAction {
        StdoutCallback(StreamCallback, String),
        StderrCallback(StreamCallback, String),
        StdoutPassthrough(String),
        StderrPassthrough(String),
        Prompt(String),
        Eof,
        Signal(String),
    }

    fn fake_drive_io(
        policy: TaskExecutionPolicy,
        events: &[IoEvent],
        colors_enabled: bool,
    ) -> Vec<IoAction> {
        let mut actions = Vec::new();
        for event in events {
            match event {
                IoEvent::Stdout(line) => match policy.stdout {
                    StreamCallback::None => actions.push(IoAction::StdoutPassthrough(line.clone())),
                    cb => actions.push(IoAction::StdoutCallback(cb, line.clone())),
                },
                IoEvent::Stderr(line) => match policy.stderr {
                    StreamCallback::None => actions.push(IoAction::StderrPassthrough(line.clone())),
                    StreamCallback::Timed => actions.push(IoAction::StderrCallback(
                        StreamCallback::Timed,
                        format_timed_stderr_line(line, colors_enabled),
                    )),
                    cb => actions.push(IoAction::StderrCallback(cb, line.clone())),
                },
                IoEvent::Prompt(p) => actions.push(IoAction::Prompt(p.clone())),
                IoEvent::Eof => actions.push(IoAction::Eof),
                IoEvent::Signal(sig) => actions.push(IoAction::Signal(sig.clone())),
            }
        }
        actions
    }

    #[test]
    fn test_interactive_structured_output_is_forced_to_interleave_with_no_line_callbacks() {
        // Matrix: C01/S11 (C2, C6)
        for requested in [
            TaskOutput::Prefix,
            TaskOutput::KeepOrder,
            TaskOutput::Replacing,
            TaskOutput::Timed,
        ] {
            let policy = task_execution_policy(requested, &Silent::Off, false, true, true);
            assert_policy(
                policy,
                TaskOutput::Interleave,
                StreamCallback::None,
                StreamCallback::None,
            );
            assert!(!policy.use_progress_report);
        }
    }

    #[test]
    fn test_interactive_raw_keeps_inherit_policy() {
        // Matrix: S07/S08 (C2, C9)
        for raw in [false, true] {
            let policy =
                task_execution_policy(TaskOutput::Interleave, &Silent::Off, raw, true, true);
            assert_eq!(policy.output, TaskOutput::Interleave);
            assert_plan(
                policy.stdio,
                StdioDirective::Inherit,
                StdioDirective::Inherit,
                StdioDirective::Inherit,
            );
        }
    }

    #[test]
    fn test_interactive_redactions_emit_only_interactive_warning() {
        // Matrix: S09 (C14)
        let policy = task_execution_policy(TaskOutput::Prefix, &Silent::Off, false, false, true);
        assert!(policy.warn_interactive_redactions);
        assert!(!policy.warn_raw_redactions);
    }

    #[test]
    fn test_non_interactive_redactions_keep_normal_policy_without_interactive_warning() {
        // Matrix: S10 (C14, C16)
        let policy = task_execution_policy(TaskOutput::Prefix, &Silent::Off, false, false, false);
        assert!(!policy.warn_interactive_redactions);
        assert!(!policy.warn_raw_redactions);
        assert_policy(
            policy,
            TaskOutput::Prefix,
            StreamCallback::Prefix,
            StreamCallback::Prefix,
        );
    }

    #[test]
    fn test_timed_stderr_color_formatting_has_no_artifacts() {
        // Matrix: S12 (C6)
        assert_eq!(
            format_timed_stderr_line("hello", true),
            "hello\x1b[0m".to_string()
        );
        assert_eq!(
            format_timed_stderr_line("hello", false),
            "hello".to_string()
        );
    }

    #[test]
    fn test_confirmation_refusal_returns_abort_error() {
        // Matrix: C03 (C12)
        let err = super::ensure_task_confirmation(false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("aborted by user"));
        super::ensure_task_confirmation(true).unwrap();
    }

    #[test]
    fn test_fake_driver_keeps_stdout_stderr_callback_order() {
        // Matrix: C01/O6 (C6)
        let policy = task_execution_policy(TaskOutput::Prefix, &Silent::Off, false, true, false);
        let events = vec![
            IoEvent::Stdout("a".to_string()),
            IoEvent::Stderr("b".to_string()),
            IoEvent::Stdout("c".to_string()),
        ];
        let actions = fake_drive_io(policy, &events, false);
        assert_eq!(
            actions,
            vec![
                IoAction::StdoutCallback(StreamCallback::Prefix, "a".to_string()),
                IoAction::StderrCallback(StreamCallback::Prefix, "b".to_string()),
                IoAction::StdoutCallback(StreamCallback::Prefix, "c".to_string()),
            ]
        );
    }

    #[test]
    fn test_fake_driver_interactive_has_no_line_callbacks() {
        // Matrix: S11 (C2, C6)
        let policy = task_execution_policy(TaskOutput::Prefix, &Silent::Off, false, true, true);
        let events = vec![
            IoEvent::Stdout("out".to_string()),
            IoEvent::Stderr("err".to_string()),
        ];
        let actions = fake_drive_io(policy, &events, false);
        assert_eq!(
            actions,
            vec![
                IoAction::StdoutPassthrough("out".to_string()),
                IoAction::StderrPassthrough("err".to_string()),
            ]
        );
    }

    #[test]
    fn test_fake_driver_prompt_eof_signal_are_preserved_symbolically() {
        // Matrix: P01/P02/F04 (symbolic approximation)
        let policy = task_execution_policy(TaskOutput::Interleave, &Silent::Off, false, true, true);
        let events = vec![
            IoEvent::Prompt(">>> ".to_string()),
            IoEvent::Eof,
            IoEvent::Signal("SIGINT".to_string()),
        ];
        let actions = fake_drive_io(policy, &events, false);
        assert_eq!(
            actions,
            vec![
                IoAction::Prompt(">>> ".to_string()),
                IoAction::Eof,
                IoAction::Signal("SIGINT".to_string()),
            ]
        );
    }

    #[test]
    fn test_trace_error_wrap_includes_stage_and_preserves_inner_error() {
        let task = crate::task::Task {
            name: "trace-task".to_string(),
            ..Default::default()
        };
        let mut report = crate::task::task_trace::TaskTraceReport::new(&task);
        report.mark(crate::task::task_trace::TaskTraceStage::ExecutorEntry);
        let mut opt = Some(&mut report);

        let wrapped = super::wrap_task_trace_error(
            &mut opt,
            eyre!("boom"),
            crate::task::task_trace::TaskTraceStage::ExecutorUsage,
        );
        let rendered = format!("{wrapped:#}");
        assert!(rendered.contains("Task Failure Report:"));
        assert!(rendered.contains("Name: trace-task"));
        assert!(rendered.contains("Reason: failed while parsing arguments for `trace-task`"));
        assert!(rendered.contains("boom"));
    }

    #[test]
    fn test_trace_error_wrap_includes_exit_code_for_script_failures() {
        let task = crate::task::Task {
            name: "trace-task".to_string(),
            ..Default::default()
        };
        let mut report = crate::task::task_trace::TaskTraceReport::new(&task);
        report.set_command("bash -lc false");
        let mut opt = Some(&mut report);
        let err =
            crate::errors::Error::ScriptFailed("bash".to_string(), Some(exit_status_with_code(23)))
                .into();

        let wrapped = super::wrap_task_trace_error(
            &mut opt,
            err,
            crate::task::task_trace::TaskTraceStage::ExecutorExecProgramExecute,
        );
        let rendered = format!("{wrapped:#}");
        assert!(rendered.contains("Command: bash -lc false"));
        assert!(rendered.contains("Exit Code: 23"));
    }

    #[test]
    fn test_trace_mark_noop_without_report() {
        let mut none: Option<&mut crate::task::task_trace::TaskTraceReport> = None;
        super::mark_task_trace(
            &mut none,
            crate::task::task_trace::TaskTraceStage::ExecutorEntry,
        );
        assert!(none.is_none());
    }
}
