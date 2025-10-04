use crate::{config, errors::Error, hash};
use std::collections::BTreeMap;
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::args::ToolArg;
use crate::cli::Cli;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, env_directive::EnvDirective};
use crate::env_diff::EnvMap;
use crate::file::display_path;
use crate::task::task_file_providers::TaskFileProvidersBuilder;
use crate::task::{Deps, GetMatchingExt, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::{ctrlc, prompt, style, time};
use crate::{dirs, duration, env, exit, file, ui};
use clap::{CommandFactory, ValueHint};
use console::Term;
use demand::{DemandOption, Select};
use duct::IntoExecutablePath;
use eyre::{Result, bail, ensure, eyre};
use glob::glob;
use indexmap::IndexMap;
use itertools::Itertools;
#[cfg(unix)]
use nix::sys::signal::SIGTERM;
use tokio::{
    sync::{Mutex, Semaphore, mpsc, oneshot},
    task::JoinSet,
};
use xx::regex;

/// Run task(s)
///
/// This command will run a tasks, or multiple tasks in parallel.
/// Tasks may have dependencies on other tasks or on source files.
/// If source is configured on a tasks, it will only run if the source
/// files have changed.
///
/// Tasks can be defined in mise.toml or as standalone scripts.
/// In mise.toml, tasks take this form:
///
///     [tasks.build]
///     run = "npm run build"
///     sources = ["src/**/*.ts"]
///     outputs = ["dist/**/*.js"]
///
/// Alternatively, tasks can be defined as standalone scripts.
/// These must be located in `mise-tasks`, `.mise-tasks`, `.mise/tasks`, `mise/tasks` or
/// `.config/mise/tasks`.
/// The name of the script will be the name of the tasks.
///
///     $ cat .mise/tasks/build<<EOF
///     #!/usr/bin/env bash
///     npm run build
///     EOF
///     $ mise run build
#[derive(clap::Args)]
#[clap(visible_alias = "r", verbatim_doc_comment, disable_help_flag = true, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    /// Tasks to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2
    #[clap(
        allow_hyphen_values = true,
        verbatim_doc_comment,
        default_value = "default"
    )]
    pub task: String,

    /// Arguments to pass to the tasks. Use ":::" to separate tasks.
    #[clap(allow_hyphen_values = true)]
    pub args: Vec<String>,

    /// Arguments to pass to the tasks. Use ":::" to separate tasks.
    #[clap(allow_hyphen_values = true, hide = true, last = true)]
    pub args_last: Vec<String>,

    /// Do not use cache on remote tasks
    #[clap(long, verbatim_doc_comment, env = "MISE_TASK_REMOTE_NO_CACHE")]
    pub no_cache: bool,

    /// Change to this directory before executing the command
    #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    pub cd: Option<PathBuf>,

    /// Continue running tasks even if one fails
    #[clap(long, short = 'c', verbatim_doc_comment)]
    pub continue_on_error: bool,

    /// Don't actually run the tasks(s), just print them in order of execution
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Force the tasks to run even if outputs are up to date
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Print stdout/stderr by line, prefixed with the task's label
    /// Defaults to true if --jobs > 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(
        long,
        short,
        verbatim_doc_comment,
        hide = true,
        overrides_with = "interleave"
    )]
    pub prefix: bool,

    /// Print directly to stdout/stderr instead of by line
    /// Defaults to true if --jobs == 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(
        long,
        short,
        verbatim_doc_comment,
        hide = true,
        overrides_with = "prefix"
    )]
    pub interleave: bool,

    /// Shell to use to run toml tasks
    ///
    /// Defaults to `sh -c -o errexit -o pipefail` on unix, and `cmd /c` on Windows
    /// Can also be set with the setting `MISE_UNIX_DEFAULT_INLINE_SHELL_ARGS` or `MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS`
    /// Or it can be overridden with the `shell` property on a task.
    #[clap(long, short, verbatim_doc_comment)]
    pub shell: Option<String>,

    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[clap(short, long, value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,

    /// Number of tasks to run in parallel
    /// [default: 4]
    /// Configure with `jobs` config or `MISE_JOBS` env var
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Read/write directly to stdin/stdout/stderr instead of by line
    /// Redactions are not applied with this option
    /// Configure with `raw` config or `MISE_RAW` env var
    #[clap(long, short, verbatim_doc_comment)]
    pub raw: bool,

    /// Don't show any output except for errors
    #[clap(long, short = 'S', verbatim_doc_comment, env = "MISE_SILENT")]
    pub silent: bool,

    /// Timeout for the task to complete
    /// e.g.: 30s, 5m
    #[clap(long, verbatim_doc_comment)]
    pub timeout: Option<String>,

    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,

    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", verbatim_doc_comment)]
    pub no_timings: bool,

    /// Don't show extra output
    #[clap(long, short, verbatim_doc_comment, env = "MISE_QUIET")]
    pub quiet: bool,

    #[clap(skip)]
    pub is_linear: bool,

    #[clap(skip)]
    pub failed_tasks: std::sync::Mutex<Vec<(Task, i32)>>,

    /// Change how tasks information is output when running tasks
    ///
    /// - `prefix` - Print stdout/stderr by line, prefixed with the task's label
    /// - `interleave` - Print directly to stdout/stderr instead of by line
    /// - `replacing` - Stdout is replaced each time, stderr is printed as is
    /// - `timed` - Only show stdout lines if they are displayed for more than 1 second
    /// - `keep-order` - Print stdout/stderr by line, prefixed with the task's label, but keep the order of the output
    /// - `quiet` - Don't show extra output
    /// - `silent` - Don't show any output including stdout and stderr from the task except for errors
    #[clap(short, long, verbatim_doc_comment, env = "MISE_TASK_OUTPUT")]
    pub output: Option<TaskOutput>,

    #[clap(skip)]
    pub tmpdir: PathBuf,

    #[clap(skip)]
    pub keep_order_output: std::sync::Mutex<IndexMap<Task, KeepOrderOutputs>>,

    #[clap(skip)]
    pub task_prs: IndexMap<Task, Arc<Box<dyn SingleReport>>>,

    #[clap(skip)]
    pub timed_outputs: Arc<std::sync::Mutex<IndexMap<String, (SystemTime, String)>>>,
}

type KeepOrderOutputs = (Vec<(String, String)>, Vec<(String, String)>);

struct SpawnCtx {
    semaphore: Arc<Semaphore>,
    config: Arc<Config>,
    sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
    jset: Arc<Mutex<JoinSet<Result<()>>>>,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
}

impl Run {
    pub async fn run(mut self) -> Result<()> {
        let config = Config::get().await?;
        if self.task == "-h" {
            self.get_clap_command().print_help()?;
            return Ok(());
        }
        if self.task == "--help" {
            self.get_clap_command().print_long_help()?;
            return Ok(());
        }
        time!("run init");
        let tmpdir = tempfile::tempdir()?;
        self.tmpdir = tmpdir.path().to_path_buf();
        let args = once(self.task.clone())
            .chain(self.args.clone())
            .chain(self.args_last.clone())
            .collect_vec();
        let task_list = get_task_lists(&config, &args, true).await?;
        time!("run get_task_lists");

        // Apply global timeout for entire run if configured
        let timeout = if let Some(timeout_str) = &self.timeout {
            Some(duration::parse_duration(timeout_str)?)
        } else {
            Settings::get().task_timeout_duration()
        };

        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, self.parallelize_tasks(config, task_list))
                .await
                .map_err(|_| eyre!("mise run timed out after {:?}", timeout))??
        } else {
            self.parallelize_tasks(config, task_list).await?
        }

        time!("run done");
        Ok(())
    }

    fn get_clap_command(&self) -> clap::Command {
        Cli::command()
            .get_subcommands()
            .find(|s| s.get_name() == "run")
            .unwrap()
            .clone()
    }

    async fn parallelize_tasks(mut self, mut config: Arc<Config>, tasks: Vec<Task>) -> Result<()> {
        time!("parallelize_tasks start");

        ctrlc::exit_on_ctrl_c(false);

        if self.output(None) == TaskOutput::Timed {
            let timed_outputs = self.timed_outputs.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(100));
                loop {
                    {
                        let mut outputs = timed_outputs.lock().unwrap();
                        for (prefix, out) in outputs.clone() {
                            let (time, line) = out;
                            if time.elapsed().unwrap().as_secs() >= 1 {
                                if console::colors_enabled() {
                                    prefix_println!(prefix, "{line}\x1b[0m");
                                } else {
                                    prefix_println!(prefix, "{line}");
                                }
                                outputs.shift_remove(&prefix);
                            }
                        }
                    }
                    interval.tick().await;
                }
            });
        }

        let mut tasks = resolve_depends(&config, tasks).await?;
        self.fetch_tasks(&mut tasks).await?;

        let tasks = Deps::new(&config, tasks).await?;
        for task in tasks.all() {
            self.validate_task(task)?;
            match self.output(Some(task)) {
                TaskOutput::KeepOrder => {
                    self.keep_order_output
                        .lock()
                        .unwrap()
                        .insert(task.clone(), Default::default());
                }
                TaskOutput::Replacing => {
                    let pr = MultiProgressReport::get().add(&task.estyled_prefix());
                    self.task_prs.insert(task.clone(), Arc::new(pr));
                }
                _ => {}
            }
        }

        let num_tasks = tasks.all().count();
        self.is_linear = tasks.is_linear();
        self.output = Some(self.output(None));
        let this = Arc::new(self);

        let mut all_tools = this.tool.clone();
        for t in tasks.all() {
            for (k, v) in &t.tools {
                all_tools.push(format!("{k}@{v}").parse()?);
            }
        }
        let mut ts = ToolsetBuilder::new()
            .with_args(&all_tools)
            .build(&config)
            .await?;

        ts.install_missing_versions(
            &mut config,
            &InstallOptions {
                missing_args_only: !Settings::get().task_run_auto_install,
                ..Default::default()
            },
        )
        .await?;

        let timer = std::time::Instant::now();
        let jset = Arc::new(Mutex::new(JoinSet::new()));
        let config = config.clone();

        type SchedMsg = (Task, Arc<Mutex<Deps>>);
        let (sched_tx, mut sched_rx) = mpsc::unbounded_channel::<SchedMsg>();
        let sched_tx = Arc::new(sched_tx);
        let in_flight = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (main_done_tx, main_done_rx) = tokio::sync::watch::channel(false);

        // Pump initial deps leaves into scheduler
        let main_deps = Arc::new(Mutex::new(tasks));
        {
            let sched_tx = sched_tx.clone();
            let main_deps_clone = main_deps.clone();
            // forward initial leaves synchronously
            {
                let mut rx = main_deps_clone.lock().await.subscribe();
                loop {
                    match rx.try_recv() {
                        Ok(Some(task)) => {
                            trace!(
                                "main deps initial leaf: {} {}",
                                task.name,
                                task.args.join(" ")
                            );
                            let _ = sched_tx.send((task, main_deps_clone.clone()));
                        }
                        Ok(None) => {
                            trace!("main deps initial done");
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
            }
            // then forward remaining leaves asynchronously
            tokio::spawn(async move {
                let mut rx = main_deps_clone.lock().await.subscribe();
                while let Some(msg) = rx.recv().await {
                    match msg {
                        Some(task) => {
                            trace!(
                                "main deps leaf scheduled: {} {}",
                                task.name,
                                task.args.join(" ")
                            );
                            let _ = sched_tx.send((task, main_deps_clone.clone()));
                        }
                        None => {
                            trace!("main deps completed");
                            let _ = main_done_tx.send(true);
                            break;
                        }
                    }
                }
            });
        }

        // Inline scheduler loop; drains ready tasks and exits when main deps done and in-flight is zero
        let semaphore = Arc::new(Semaphore::new(this.jobs()));
        let mut main_done_rx = main_done_rx.clone();
        loop {
            // Drain ready tasks without awaiting
            let mut drained_any = false;
            loop {
                match sched_rx.try_recv() {
                    Ok((task, deps_for_remove)) => {
                        drained_any = true;
                        trace!("scheduler received: {} {}", task.name, task.args.join(" "));
                        if this.is_stopping() && !this.continue_on_error {
                            break;
                        }
                        Self::spawn_sched_job(
                            this.clone(),
                            task,
                            deps_for_remove,
                            SpawnCtx {
                                semaphore: semaphore.clone(),
                                config: config.clone(),
                                sched_tx: sched_tx.clone(),
                                jset: jset.clone(),
                                in_flight: in_flight.clone(),
                            },
                        )
                        .await?;
                    }
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }
            }

            // Check if we should stop early due to failure
            if this.is_stopping() && !this.continue_on_error {
                trace!("scheduler: stopping early due to failure, cleaning up main deps");
                // Clean up the dependency graph to ensure the main_done signal is sent
                let mut deps = main_deps.lock().await;
                let tasks_to_remove: Vec<Task> = deps.all().cloned().collect();
                for task in tasks_to_remove {
                    deps.remove(&task);
                }
                drop(deps);
                break;
            }

            // Exit if main deps finished and nothing is running/queued
            if *main_done_rx.borrow()
                && in_flight.load(std::sync::atomic::Ordering::SeqCst) == 0
                && !drained_any
            {
                trace!("scheduler drain complete; exiting loop");
                break;
            }

            // Await either new work or main_done change
            tokio::select! {
                m = sched_rx.recv() => {
                    if let Some((task, deps_for_remove)) = m {
                        trace!("scheduler received: {} {}", task.name, task.args.join(" "));
                        if this.is_stopping() && !this.continue_on_error { break; }
                        Self::spawn_sched_job(
                            this.clone(),
                            task,
                            deps_for_remove,
                            SpawnCtx {
                                semaphore: semaphore.clone(),
                                config: config.clone(),
                                sched_tx: sched_tx.clone(),
                                jset: jset.clone(),
                                in_flight: in_flight.clone(),
                            },
                        )
                        .await?;
                    } else {
                        // channel closed; rely on main_done/in_flight to exit soon
                    }
                }
                _ = main_done_rx.changed() => {
                    trace!("main_done changed: {}", *main_done_rx.borrow());
                }
            }
        }

        while let Some(result) = jset.lock().await.join_next().await {
            if result.is_ok() || this.continue_on_error {
                continue;
            }
            #[cfg(unix)]
            CmdLineRunner::kill_all(SIGTERM);
            #[cfg(windows)]
            CmdLineRunner::kill_all();
            break;
        }
        // scheduler loop done

        if this.output(None) == TaskOutput::KeepOrder {
            // TODO: display these as tasks complete in order somehow rather than waiting until everything is done
            let output = this.keep_order_output.lock().unwrap();
            for (out, err) in output.values() {
                for (prefix, line) in out {
                    if console::colors_enabled() {
                        prefix_println!(prefix, "{line}\x1b[0m");
                    } else {
                        prefix_println!(prefix, "{line}");
                    }
                }
                for (prefix, line) in err {
                    if console::colors_enabled_stderr() {
                        prefix_eprintln!(prefix, "{line}\x1b[0m");
                    } else {
                        prefix_eprintln!(prefix, "{line}");
                    }
                }
            }
        }
        if this.timings() && num_tasks > 1 {
            let msg = format!("Finished in {}", time::format_duration(timer.elapsed()));
            eprintln!("{}", style::edim(msg));
        };
        // If there were failures and --continue-on-error was used, print a brief summary
        this.maybe_print_failure_summary();
        if let Some((task, status)) = this.failed_tasks.lock().unwrap().first() {
            let prefix = task.estyled_prefix();
            this.eprint(
                task,
                &prefix,
                &format!("{} task failed", style::ered("ERROR")),
            );
            exit(*status);
        }
        time!("parallelize_tasks done");

        Ok(())
    }

    async fn spawn_sched_job(
        this: Arc<Self>,
        task: Task,
        deps_for_remove: Arc<Mutex<Deps>>,
        ctx: SpawnCtx,
    ) -> Result<()> {
        // If we're already stopping due to a previous failure and not in
        // continue-on-error mode, do not launch this task. Ensure we remove
        // it from the dependency graph so the scheduler can make progress.
        if this.is_stopping() && !this.continue_on_error {
            trace!(
                "aborting spawn before start (not continue-on-error): {} {}",
                task.name,
                task.args.join(" ")
            );
            deps_for_remove.lock().await.remove(&task);
            return Ok(());
        }
        let needs_permit = Self::task_needs_permit(&task);
        let permit_opt = if needs_permit {
            let wait_start = std::time::Instant::now();
            let p = Some(ctx.semaphore.clone().acquire_owned().await?);
            trace!(
                "semaphore acquired for {} after {}ms",
                task.name,
                wait_start.elapsed().as_millis()
            );
            // If a failure occurred while we were waiting for a permit and we're not
            // in continue-on-error mode, skip launching this task. This prevents
            // subsequently queued tasks (e.g., from CLI ":::" groups) from running
            // after the first failure when --jobs=1 and ensures immediate stop.
            if this.is_stopping() && !this.continue_on_error {
                trace!(
                    "aborting spawn after failure (not continue-on-error): {} {}",
                    task.name,
                    task.args.join(" ")
                );
                // Remove from deps so the scheduler can drain and not hang
                deps_for_remove.lock().await.remove(&task);
                return Ok(());
            }
            p
        } else {
            trace!("no semaphore needed for orchestrator task: {}", task.name);
            None
        };

        ctx.in_flight
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let in_flight_c = ctx.in_flight.clone();
        trace!("running task: {task}");
        ctx.jset.lock().await.spawn(async move {
            let _permit = permit_opt;
            let result = this
                .run_task_sched(&task, &ctx.config, ctx.sched_tx.clone())
                .await;
            if let Err(err) = &result {
                let status = Error::get_exit_status(err);
                if !this.is_stopping() && status.is_none() {
                    let prefix = task.estyled_prefix();
                    if Settings::get().verbose {
                        this.eprint(&task, &prefix, &format!("{} {err:?}", style::ered("ERROR")));
                    } else {
                        this.eprint(&task, &prefix, &format!("{} {err}", style::ered("ERROR")));
                        let mut current_err = err.source();
                        while let Some(e) = current_err {
                            this.eprint(&task, &prefix, &format!("{} {e}", style::ered("ERROR")));
                            current_err = e.source();
                        }
                    };
                }
                this.add_failed_task(task.clone(), status);
            }
            deps_for_remove.lock().await.remove(&task);
            trace!("deps removed: {} {}", task.name, task.args.join(" "));
            in_flight_c.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            result
        });

        Ok(())
    }

    fn task_needs_permit(task: &Task) -> bool {
        // Only shell/script tasks execute external commands and need a concurrency slot.
        // Orchestrator-only tasks (pure groups of sub-tasks) do not.
        task.file.is_some() || !task.run_script_strings().is_empty()
    }

    fn maybe_print_failure_summary(&self) {
        if !self.continue_on_error {
            return;
        }
        let failed = self.failed_tasks.lock().unwrap().clone();
        if failed.is_empty() {
            return;
        }
        let count = failed.len();
        eprintln!("{} {} task(s) failed:", style::ered("ERROR"), count);
        for (task, status) in &failed {
            let prefix = task.estyled_prefix();
            self.eprint(task, &prefix, &format!("exited with status {}", status));
        }
    }

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        match self.output(Some(task)) {
            TaskOutput::Replacing => {
                let pr = self.task_prs.get(task).unwrap().clone();
                pr.set_message(format!("{prefix} {line}"));
            }
            _ => {
                prefix_eprintln!(prefix, "{line}");
            }
        }
    }

    async fn run_task_sched(
        &self,
        task: &Task,
        config: &Arc<Config>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
    ) -> Result<()> {
        let prefix = task.estyled_prefix();
        let total_start = std::time::Instant::now();
        if Settings::get().task_skip.contains(&task.name) {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "skipping task");
            }
            return Ok(());
        }
        if !self.force && self.sources_are_fresh(task, config).await? {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            return Ok(());
        }
        if let Some(message) = &task.confirm {
            if !Settings::get().yes && !ui::confirm(message).unwrap_or(false) {
                return Err(eyre!("aborted by user"));
            }
        }

        let mut tools = self.tool.clone();
        for (k, v) in &task.tools {
            tools.push(format!("{k}@{v}").parse()?);
        }
        let ts_build_start = std::time::Instant::now();
        let ts = ToolsetBuilder::new()
            .with_args(&tools)
            .build(config)
            .await?;
        trace!(
            "task {} ToolsetBuilder::build took {}ms",
            task.name,
            ts_build_start.elapsed().as_millis()
        );
        let env_render_start = std::time::Instant::now();
        let (mut env, task_env) = task.render_env(config, &ts).await?;
        trace!(
            "task {} render_env took {}ms",
            task.name,
            env_render_start.elapsed().as_millis()
        );
        let output = self.output(Some(task));
        env.insert("MISE_TASK_OUTPUT".into(), output.to_string());
        if !self.timings {
            env.insert("MISE_TASK_TIMINGS".to_string(), "0".to_string());
        }
        if let Some(cwd) = &*dirs::CWD {
            env.insert("MISE_ORIGINAL_CWD".into(), cwd.display().to_string());
        }
        if let Some(root) = config.project_root.clone().or(task.config_root.clone()) {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
        }
        env.insert("MISE_TASK_NAME".into(), task.name.clone());
        let task_file = task.file.as_ref().unwrap_or(&task.config_source);
        env.insert("MISE_TASK_FILE".into(), task_file.display().to_string());
        if let Some(dir) = task_file.parent() {
            env.insert("MISE_TASK_DIR".into(), dir.display().to_string());
        }
        if let Some(config_root) = &task.config_root {
            env.insert("MISE_CONFIG_ROOT".into(), config_root.display().to_string());
        }
        let timer = std::time::Instant::now();

        if let Some(file) = &task.file {
            let exec_start = std::time::Instant::now();
            self.exec_file(config, file, task, &env, &prefix).await?;
            trace!(
                "task {} exec_file took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        } else {
            let rendered_run_scripts = task
                .render_run_scripts_with_args(config, self.cd.clone(), &task.args, &env)
                .await?;

            let get_args = || {
                [String::new()]
                    .iter()
                    .chain(task.args.iter())
                    .cloned()
                    .collect()
            };
            self.parse_usage_spec_and_init_env(config, task, &mut env, get_args)
                .await?;

            let exec_start = std::time::Instant::now();
            self.exec_task_run_entries(
                config,
                task,
                (&env, &task_env),
                &prefix,
                rendered_run_scripts,
                sched_tx,
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

        self.save_checksum(task)?;

        Ok(())
    }

    async fn exec_task_run_entries(
        &self,
        config: &Arc<Config>,
        task: &Task,
        full_env: (&BTreeMap<String, String>, &[(String, String)]),
        prefix: &str,
        rendered_scripts: Vec<(String, Vec<String>)>,
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
    ) -> Result<()> {
        let (env, task_env) = full_env;
        use crate::task::RunEntry;
        let mut script_iter = rendered_scripts.into_iter();
        for entry in task.run() {
            match entry {
                RunEntry::Script(_) => {
                    if let Some((script, args)) = script_iter.next() {
                        self.exec_script(&script, &args, task, env, prefix).await?;
                    }
                }
                RunEntry::SingleTask { task: spec } => {
                    self.inject_and_wait(config, &[spec.to_string()], task_env, sched_tx.clone())
                        .await?;
                }
                RunEntry::TaskGroup { tasks } => {
                    self.inject_and_wait(config, tasks, task_env, sched_tx.clone())
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn inject_and_wait(
        &self,
        config: &Arc<Config>,
        specs: &[String],
        task_env: &[(String, String)],
        sched_tx: Arc<mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
    ) -> Result<()> {
        trace!("inject start: {}", specs.join(", "));
        // Build tasks list from specs
        let tasks_map = config.tasks_with_aliases().await?;
        let mut to_run: Vec<Task> = vec![];
        for spec in specs {
            let (name, args) = split_task_spec(spec);
            let matches = tasks_map.get_matching(name)?;
            ensure!(!matches.is_empty(), "task not found: {}", name);
            for t in matches {
                let mut t = (*t).clone();
                t.args = args.clone();
                to_run.push(t);
            }
        }
        let sub_deps = Deps::new(config, to_run).await?;
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
            let msg = style::ebold(trunc(prefix, config.redact(cmd).trim()))
                .bright()
                .to_string();
            self.eprint(task, prefix, &msg)
        }

        if script.starts_with("#!") {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("script");
            let mut tmp = std::fs::File::create(&file)?;
            tmp.write_all(script.as_bytes())?;
            tmp.flush()?;
            drop(tmp);
            file::make_executable(&file)?;
            self.exec(&file, args, task, env, prefix).await
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
        if file::is_executable(file) && !Settings::get().use_file_shell_for_executable_tasks {
            if cfg!(windows) && file.extension().is_some_and(|e| e == "ps1") {
                let args = vec!["-File".to_string(), display]
                    .into_iter()
                    .chain(args.iter().cloned())
                    .collect_vec();
                return Ok(("pwsh".to_string(), args));
            }
            return Ok((display, args.to_vec()));
        }
        let shell = task
            .shell()
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
        let mut script = script.to_string();
        if !args.is_empty() {
            #[cfg(windows)]
            {
                script = format!("{script} {}", args.join(" "));
            }
            #[cfg(unix)]
            {
                script = format!("{script} {}", shell_words::join(args));
            }
        }
        full_args.push(script);
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
        env: &EnvMap,
        prefix: &str,
    ) -> Result<()> {
        let mut env = env.clone();
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();
        let get_args = || once(command.clone()).chain(args.clone()).collect_vec();
        self.parse_usage_spec_and_init_env(config, task, &mut env, get_args)
            .await?;

        if !self.quiet(Some(task)) {
            let cmd = format!("{} {}", display_path(file), args.join(" "))
                .trim()
                .to_string();
            let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
            let cmd = trunc(prefix, config.redact(cmd).trim());
            self.eprint(task, prefix, &cmd);
        }

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
        let mut cmd = CmdLineRunner::new(program.clone())
            .args(args)
            .envs(env)
            .redact(redactions.deref().clone())
            .raw(raw);
        if raw && !redactions.is_empty() {
            hint!(
                "raw_redactions",
                "--raw will prevent mise from being able to use redactions",
                ""
            );
        }
        let output = self.output(Some(task));
        cmd.with_pass_signals();
        match output {
            TaskOutput::Prefix => {
                cmd = cmd.with_on_stdout(|line| {
                    if console::colors_enabled() {
                        prefix_println!(prefix, "{line}\x1b[0m");
                    } else {
                        prefix_println!(prefix, "{line}");
                    }
                });
                cmd = cmd.with_on_stderr(|line| {
                    if console::colors_enabled() {
                        self.eprint(task, prefix, &format!("{line}\x1b[0m"));
                    } else {
                        self.eprint(task, prefix, &line);
                    }
                });
            }
            TaskOutput::KeepOrder => {
                cmd = cmd.with_on_stdout(|line| {
                    let mut map = self.keep_order_output.lock().unwrap();
                    if !map.contains_key(task) {
                        map.insert(task.clone(), Default::default());
                    }
                    if let Some(entry) = map.get_mut(task) {
                        entry.0.push((prefix.to_string(), line));
                    }
                });
                cmd = cmd.with_on_stderr(|line| {
                    let mut map = self.keep_order_output.lock().unwrap();
                    if !map.contains_key(task) {
                        map.insert(task.clone(), Default::default());
                    }
                    if let Some(entry) = map.get_mut(task) {
                        entry.1.push((prefix.to_string(), line));
                    }
                });
            }
            TaskOutput::Replacing => {
                let pr = self.task_prs.get(task).unwrap().clone();
                cmd = cmd.with_pr_arc(pr);
            }
            TaskOutput::Timed => {
                let timed_outputs = self.timed_outputs.clone();
                cmd = cmd.with_on_stdout(move |line| {
                    timed_outputs
                        .lock()
                        .unwrap()
                        .insert(prefix.to_string(), (SystemTime::now(), line));
                });
                cmd = cmd.with_on_stderr(|line| {
                    if console::colors_enabled() {
                        self.eprint(task, prefix, &format!("{line}\x1b[0m"));
                    } else {
                        self.eprint(task, prefix, &line);
                    }
                });
            }
            TaskOutput::Silent => {
                cmd = cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
            TaskOutput::Quiet | TaskOutput::Interleave => {
                if raw || redactions.is_empty() {
                    cmd = cmd
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                }
            }
        }
        let dir = self.cwd(task, &config).await?;
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
        cmd.execute()?;
        trace!("{prefix} exited successfully");
        Ok(())
    }

    fn output(&self, task: Option<&Task>) -> TaskOutput {
        if let Some(o) = self.output {
            o
        } else if self.silent(task) {
            TaskOutput::Silent
        } else if self.quiet(task) {
            TaskOutput::Quiet
        } else if self.prefix {
            TaskOutput::Prefix
        } else if self.interleave {
            TaskOutput::Interleave
        } else if let Some(output) = Settings::get().task_output {
            output
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
        }
    }

    fn silent(&self, task: Option<&Task>) -> bool {
        self.silent
            || Settings::get().silent
            || self.output.is_some_and(|o| o.is_silent())
            || task.is_some_and(|t| t.silent)
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.quiet
            || Settings::get().quiet
            || self.output.is_some_and(|o| o.is_quiet())
            || task.is_some_and(|t| t.quiet)
            || self.silent(task)
    }

    fn raw(&self, task: Option<&Task>) -> bool {
        self.raw || Settings::get().raw || task.is_some_and(|t| t.raw)
    }

    fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            self.jobs.unwrap_or(Settings::get().jobs)
        }
    }

    fn validate_task(&self, task: &Task) -> Result<()> {
        if let Some(path) = &task.file {
            if path.exists() && !file::is_executable(path) {
                let dp = display_path(path);
                let msg = format!("Script `{dp}` is not executable. Make it executable?");
                if ui::confirm(msg)? {
                    file::make_executable(path)?;
                } else {
                    bail!("`{dp}` is not executable")
                }
            }
        }
        Ok(())
    }

    async fn parse_usage_spec_and_init_env(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &mut EnvMap,
        get_args: impl Fn() -> Vec<String>,
    ) -> Result<()> {
        let (spec, _) = task.parse_usage_spec(config, self.cd.clone(), env).await?;
        if !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty() {
            let args: Vec<String> = get_args();
            trace!("Parsing usage spec for {:?}", args);
            let po = usage::parse(&spec, &args).map_err(|err| eyre!(err))?;
            for (k, v) in po.as_env() {
                trace!("Adding key {} value {} in env", k, v);
                env.insert(k, v);
            }
        } else {
            trace!("Usage spec has no args or flags");
        }

        Ok(())
    }

    async fn sources_are_fresh(&self, task: &Task, config: &Arc<Config>) -> Result<bool> {
        if task.sources.is_empty() {
            return Ok(false);
        }
        // TODO: We should benchmark this and find out if it might be possible to do some caching around this or something
        // perhaps using some manifest in a state directory or something, maybe leveraging atime?
        let run = async || -> Result<bool> {
            let root = self.cwd(task, config).await?;
            let mut sources = task.sources.clone();
            sources.push(task.config_source.to_string_lossy().to_string());
            let source_metadatas = self.get_file_metadatas(&root, &sources)?;
            let source_metadata_hash = self.file_metadatas_to_hash(&source_metadatas);
            let source_metadata_hash_path = self.sources_hash_path(task);
            if let Some(dir) = source_metadata_hash_path.parent() {
                file::create_dir_all(dir)?;
            }
            if self
                .source_metadata_existing_hash(task)
                .is_some_and(|h| h != source_metadata_hash)
            {
                debug!(
                    "source metadata hash mismatch in {}",
                    source_metadata_hash_path.display()
                );
                file::write(&source_metadata_hash_path, &source_metadata_hash)?;
                return Ok(false);
            }
            let sources = self.get_last_modified_from_metadatas(&source_metadatas);
            let outputs = self.get_last_modified(&root, &task.outputs.paths(task))?;
            file::write(&source_metadata_hash_path, &source_metadata_hash)?;
            trace!("sources: {sources:?}, outputs: {outputs:?}");
            match (sources, outputs) {
                (Some(sources), Some(outputs)) => Ok(sources < outputs),
                _ => Ok(false),
            }
        };
        Ok(run().await.unwrap_or_else(|err| {
            warn!("sources_are_fresh: {err:?}");
            false
        }))
    }

    fn sources_hash_path(&self, task: &Task) -> PathBuf {
        let mut hasher = DefaultHasher::new();
        task.hash(&mut hasher);
        task.config_source.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());
        dirs::STATE.join("task-sources").join(&hash)
    }

    fn source_metadata_existing_hash(&self, task: &Task) -> Option<String> {
        let path = self.sources_hash_path(task);
        if path.exists() {
            Some(file::read_to_string(&path).unwrap_or_default())
        } else {
            None
        }
    }

    fn add_failed_task(&self, task: Task, status: Option<i32>) {
        self.failed_tasks
            .lock()
            .unwrap()
            .push((task, status.unwrap_or(1)));
    }

    fn is_stopping(&self) -> bool {
        !self.failed_tasks.lock().unwrap().is_empty()
    }

    fn get_file_metadatas(
        &self,
        root: &Path,
        patterns_or_paths: &[String],
    ) -> Result<Vec<(PathBuf, fs::Metadata)>> {
        if patterns_or_paths.is_empty() {
            return Ok(vec![]);
        }
        let (patterns, paths): (Vec<&String>, Vec<&String>) =
            patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

        let mut metadatas = BTreeMap::new();
        for pattern in patterns {
            let files = glob(root.join(pattern).to_str().unwrap())?;
            for file in files.flatten() {
                if let Ok(metadata) = file.metadata() {
                    metadatas.insert(file, metadata);
                }
            }
        }

        for path in paths {
            let file = root.join(path);
            if let Ok(metadata) = file.metadata() {
                metadatas.insert(file, metadata);
            }
        }

        let metadatas = metadatas
            .into_iter()
            .filter(|(_, m)| m.is_file())
            .collect_vec();

        Ok(metadatas)
    }

    fn file_metadatas_to_hash(&self, metadatas: &[(PathBuf, fs::Metadata)]) -> String {
        let paths: Vec<_> = metadatas.iter().map(|(p, _)| p).collect();
        hash::hash_to_str(&paths)
    }

    fn get_last_modified_from_metadatas(
        &self,
        metadatas: &[(PathBuf, fs::Metadata)],
    ) -> Option<SystemTime> {
        metadatas.iter().flat_map(|(_, m)| m.modified()).max()
    }

    fn get_last_modified(
        &self,
        root: &Path,
        patterns_or_paths: &[String],
    ) -> Result<Option<SystemTime>> {
        if patterns_or_paths.is_empty() {
            return Ok(None);
        }
        let (patterns, paths): (Vec<&String>, Vec<&String>) =
            patterns_or_paths.iter().partition(|p| is_glob_pattern(p));

        let last_mod = std::cmp::max(
            last_modified_glob_match(root, &patterns)?,
            last_modified_path(root, &paths)?,
        );

        trace!(
            "last_modified of {}: {last_mod:?}",
            patterns_or_paths.iter().join(" ")
        );
        Ok(last_mod)
    }

    async fn cwd(&self, task: &Task, config: &Arc<Config>) -> Result<PathBuf> {
        if let Some(d) = task.dir(config).await? {
            Ok(d)
        } else {
            Ok(config
                .project_root
                .clone()
                .or_else(|| dirs::CWD.clone())
                .unwrap_or_default())
        }
    }

    fn save_checksum(&self, task: &Task) -> Result<()> {
        if task.sources.is_empty() {
            return Ok(());
        }
        if task.outputs.is_auto() {
            for p in task.outputs.paths(task) {
                debug!("touching auto output file: {p}");
                file::touch_file(&PathBuf::from(&p))?;
            }
        }
        Ok(())
    }

    fn timings(&self) -> bool {
        !self.quiet(None) && !self.no_timings
    }

    fn task_timings(&self) -> bool {
        self.timings()
            && Settings::get().task_timings.unwrap_or(
                self.output == Some(TaskOutput::Prefix)
                    || self.output == Some(TaskOutput::Timed)
                    || self.output == Some(TaskOutput::KeepOrder),
            )
    }

    async fn fetch_tasks(&self, tasks: &mut Vec<Task>) -> Result<()> {
        let no_cache = self.no_cache || Settings::get().task_remote_no_cache.unwrap_or(false);
        let task_file_providers = TaskFileProvidersBuilder::new()
            .with_cache(!no_cache)
            .build();

        for t in tasks {
            if let Some(file) = &t.file {
                let source = file.to_string_lossy().to_string();

                let provider = task_file_providers.get_provider(&source);

                if provider.is_none() {
                    bail!("No provider found for file: {}", source);
                }

                let local_path = provider.unwrap().get_local_path(&source).await?;

                t.file = Some(local_path);
            }
        }

        Ok(())
    }
}

fn split_task_spec(spec: &str) -> (&str, Vec<String>) {
    let mut parts = spec.split_whitespace();
    let name = parts.next().unwrap_or("");
    let args = parts.map(|s| s.to_string()).collect_vec();
    (name, args)
}

fn is_glob_pattern(path: &str) -> bool {
    // This is the character set used for glob
    // detection by glob
    let glob_chars = ['*', '{', '}'];

    path.chars().any(|c| glob_chars.contains(&c))
}

fn last_modified_path(root: &Path, paths: &[&String]) -> Result<Option<SystemTime>> {
    let files = paths.iter().map(|p| {
        let base = Path::new(p);
        if base.is_relative() {
            Path::new(&root).join(base)
        } else {
            base.to_path_buf()
        }
    });

    last_modified_file(files)
}

fn last_modified_glob_match(
    root: impl AsRef<Path>,
    patterns: &[&String],
) -> Result<Option<SystemTime>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let files = patterns
        .iter()
        .flat_map(|pattern| {
            glob(
                root.as_ref()
                    .join(pattern)
                    .to_str()
                    .expect("Conversion to string path failed"),
            )
            .unwrap()
        })
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.metadata()
                .expect("Metadata call failed")
                .file_type()
                .is_file()
        });

    last_modified_file(files)
}

fn last_modified_file(files: impl IntoIterator<Item = PathBuf>) -> Result<Option<SystemTime>> {
    Ok(files
        .into_iter()
        .unique()
        .filter(|p| p.exists())
        .map(|p| {
            p.metadata()
                .map_err(|err| eyre!("{}: {}", display_path(p), err))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|m| m.modified().map_err(|err| eyre!(err)))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .max())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Runs the "lint" tasks. This needs to either be defined in mise.toml
    # or as a standalone script. See the project README for more information.
    $ <bold>mise run lint</bold>

    # Forces the "build" tasks to run even if its sources are up-to-date.
    $ <bold>mise run build --force</bold>

    # Run "test" with stdin/stdout/stderr all connected to the current terminal.
    # This forces `--jobs=1` to prevent interleaving of output.
    $ <bold>mise run test --raw</bold>

    # Runs the "lint", "test", and "check" tasks in parallel.
    $ <bold>mise run lint ::: test ::: check</bold>

    # Execute multiple tasks each with their own arguments.
    $ <bold>mise tasks cmd1 arg1 arg2 ::: cmd2 arg1 arg2</bold>
"#
);

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::EnumIs,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TaskOutput {
    Interleave,
    KeepOrder,
    #[default]
    Prefix,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

fn trunc(prefix: &str, msg: &str) -> String {
    let prefix_len = console::measure_text_width(prefix);
    let msg = msg.lines().next().unwrap_or_default();
    // Ensure we have at least 20 characters for the message, even with very long prefixes
    let available_width = (*env::TERM_WIDTH).saturating_sub(prefix_len + 1);
    let max_width = available_width.max(20); // Always show at least 20 chars of message
    console::truncate_str(msg, max_width, "…").to_string()
}

async fn err_no_task(config: &Config, name: &str) -> Result<()> {
    if config.tasks().await.is_ok_and(|t| t.is_empty()) {
        bail!(
            "no tasks defined in {}. Are you in a project directory?",
            display_path(dirs::CWD.clone().unwrap_or_default())
        );
    }
    if let Some(cwd) = &*dirs::CWD {
        let includes = config::task_includes_for_dir(cwd, &config.config_files);
        let path = includes
            .iter()
            .map(|d| d.join(name))
            .find(|d| d.is_file() && !file::is_executable(d));
        if let Some(path) = path {
            if !cfg!(windows) {
                warn!(
                    "no task {} found, but a non-executable file exists at {}",
                    style::ered(name),
                    display_path(&path)
                );
                let yn = prompt::confirm(
                    "Mark this file as executable to allow it to be run as a task?",
                )?;
                if yn {
                    file::make_executable(&path)?;
                    info!("marked as executable, try running this task again");
                }
            }
        }
    }
    bail!("no task {} found", style::ered(name));
}

async fn prompt_for_task() -> Result<Task> {
    let config = Config::get().await?;
    let tasks = config.tasks().await?;
    ensure!(
        !tasks.is_empty(),
        "no tasks defined. see {url}",
        url = style::eunderline("https://mise.jdx.dev/tasks/")
    );
    let mut s = Select::new("Tasks")
        .description("Select a task to run")
        .filtering(true)
        .filterable(true);
    for t in tasks.values().filter(|t| !t.hide) {
        s = s.option(
            DemandOption::new(&t.name)
                .label(&t.display_name)
                .description(&t.description),
        );
    }
    ctrlc::show_cursor_after_ctrl_c();
    match s.run() {
        Ok(name) => match tasks.get(name) {
            Some(task) => Ok(task.clone()),
            None => bail!("no tasks {} found", style::ered(name)),
        },
        Err(err) => {
            Term::stderr().show_cursor()?;
            Err(eyre!(err))
        }
    }
}

pub async fn get_task_lists(
    config: &Arc<Config>,
    args: &[String],
    prompt: bool,
) -> Result<Vec<Task>> {
    let args = args
        .iter()
        .map(|s| vec![s.to_string()])
        .coalesce(|a, b| {
            if b == vec![":::".to_string()] {
                Err((a, b))
            } else if a == vec![":::".to_string()] {
                Ok(b)
            } else {
                Ok(a.into_iter().chain(b).collect_vec())
            }
        })
        .flat_map(|args| args.split_first().map(|(t, a)| (t.clone(), a.to_vec())))
        .collect::<Vec<_>>();
    let mut tasks = vec![];
    let arg_re = regex!(r#"^((\.*|~)(/|\\)|\w:\\)"#);
    for (t, args) in args {
        // can be any of the following:
        // - ./path/to/script
        // - ~/path/to/script
        // - /path/to/script
        // - ../path/to/script
        // - C:\path\to\script
        // - .\path\to\script
        if arg_re.is_match(&t) {
            let path = PathBuf::from(&t);
            if path.exists() {
                let config_root = config
                    .project_root
                    .clone()
                    .or_else(|| dirs::CWD.clone())
                    .unwrap_or_default();
                let task = Task::from_path(config, &path, &PathBuf::new(), &config_root).await?;
                return Ok(vec![task.with_args(args)]);
            }
        }
        let cur_tasks = config
            .tasks_with_aliases()
            .await?
            .get_matching(&t)?
            .into_iter()
            .cloned()
            .collect_vec();
        if cur_tasks.is_empty() {
            if t != "default" || !prompt || !console::user_attended_stderr() {
                err_no_task(config, &t).await?;
            }
            tasks.push(prompt_for_task().await?);
        } else {
            cur_tasks
                .into_iter()
                .map(|t| t.clone().with_args(args.to_vec()))
                .for_each(|t| tasks.push(t));
        }
    }
    Ok(tasks)
}

pub async fn resolve_depends(config: &Arc<Config>, tasks: Vec<Task>) -> Result<Vec<Task>> {
    let all_tasks = config.tasks_with_aliases().await?;
    tasks
        .into_iter()
        .map(|t| {
            let depends = t.all_depends(&all_tasks)?;
            Ok(once(t).chain(depends).collect::<Vec<_>>())
        })
        .flatten_ok()
        .collect()
}
