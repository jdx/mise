use crate::errors::Error;
use std::io::IsTerminal;
use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use super::args::ToolArg;
use crate::cli::{Cli, unescape_task_args};
use crate::config::{Config, Settings};
use crate::duration;
use crate::env;
use crate::file::display_path;
use crate::prepare::{PrepareEngine, PrepareOptions};
use crate::task::has_any_args_defined;
use crate::task::task_execution_plan::{
    ExecutionPlan, ExecutionStageKind, PlanContextIndex, PlannedTask,
    declaration_source_is_generated, execution_plan_config_files, execution_plan_hash,
    execution_plan_stats, execution_stage_hash, format_declaration_location, task_declaration_ref,
};
use crate::task::task_helpers::{
    STATIC_BARRIER_END_SEGMENT, STATIC_BARRIER_START_SEGMENT, STATIC_INTERNAL_TASK_PREFIX,
};
use crate::task::task_list::{get_task_lists, resolve_depends};
use crate::task::task_output::TaskOutput;
use crate::task::task_output_handler::OutputHandler;
use crate::task::task_plan_analysis::{
    ChangeImpact, ContentionAnalysis, GraphCycle, cycle_path_label, identity_label,
};
use crate::task::task_plan_bundle::{
    PlanBuildResolvedTasksRequest, build_execution_plan_bundle_from_resolved_tasks,
};
use crate::task::task_spawn_decision::{SpawnBarrierMode, should_skip_spawn, spawn_barrier_mode};
use crate::task::{Deps, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::{ctrlc, info, style};
use clap::{CommandFactory, ValueHint};
use eyre::{Result, bail, eyre};
use itertools::Itertools;
use serde::Serialize;
use tokio::sync::Mutex;

/// Run task(s)
///
/// This command will run a task, or multiple tasks in parallel.
/// Tasks may have dependencies on other tasks or on source files.
/// If source is configured on a task, it will only run if the source
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

    /// Continue running tasks even if one fails
    #[clap(long, short = 'c', verbatim_doc_comment)]
    pub continue_on_error: bool,

    /// Change to this directory before executing the command
    #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    pub cd: Option<PathBuf>,

    /// Force the tasks to run even if outputs are up to date
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Print directly to stdout/stderr instead of by line
    /// Defaults to true if --jobs == 1
    /// Configure with `task.output` config or `MISE_TASK_OUTPUT` env var
    #[clap(
        long,
        short,
        verbatim_doc_comment,
        hide = true,
        overrides_with = "prefix"
    )]
    pub interleave: bool,

    /// Number of tasks to run in parallel
    /// [default: 4]
    /// Configure with `jobs` config or `MISE_JOBS` env var
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Don't actually run the task(s), just print them in order of execution
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Changed files to analyze impact against task sources in `--plan` mode
    ///
    /// Can be provided multiple times:
    /// - `--changed=src/main.ts`
    /// - `--changed=src/a.ts --changed=src/b.ts`
    #[clap(
        long,
        value_name = "PATH",
        value_hint = ValueHint::AnyPath,
        requires = "plan",
        verbatim_doc_comment
    )]
    pub changed: Vec<String>,

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

    /// Print stdout/stderr by line, prefixed with the task's label
    /// Defaults to true if --jobs > 1
    /// Configure with `task.output` config or `MISE_TASK_OUTPUT` env var
    #[clap(
        long,
        short,
        verbatim_doc_comment,
        hide = true,
        overrides_with = "interleave"
    )]
    pub prefix: bool,

    /// Don't show extra output
    #[clap(long, short, verbatim_doc_comment, env = "MISE_QUIET")]
    pub quiet: bool,

    /// Read/write directly to stdin/stdout/stderr instead of by line
    /// Redactions are not applied with this option
    /// Configure with `raw` config or `MISE_RAW` env var
    #[clap(long, short, verbatim_doc_comment)]
    pub raw: bool,

    /// Shell to use to run toml tasks
    ///
    /// Defaults to `sh -c -o errexit -o pipefail` on unix, and `cmd /c` on Windows
    /// Can also be set with the setting `MISE_UNIX_DEFAULT_INLINE_SHELL_ARGS` or `MISE_WINDOWS_DEFAULT_INLINE_SHELL_ARGS`
    /// Or it can be overridden with the `shell` property on a task.
    #[clap(long, short, verbatim_doc_comment)]
    pub shell: Option<String>,

    /// Don't show any output except for errors
    #[clap(long, short = 'S', verbatim_doc_comment, env = "MISE_SILENT")]
    pub silent: bool,

    /// Tool(s) to run in addition to what is in mise.toml files
    /// e.g.: node@20 python@3.10
    #[clap(short, long, value_name = "TOOL@VERSION")]
    pub tool: Vec<ToolArg>,

    #[clap(skip)]
    pub is_linear: bool,

    /// Bypass the environment cache and recompute the environment
    #[clap(long)]
    pub fresh_env: bool,

    /// Do not use cache on remote tasks
    #[clap(long, verbatim_doc_comment, env = "MISE_TASK_REMOTE_NO_CACHE")]
    pub no_cache: bool,

    /// Skip automatic dependency preparation
    #[clap(long)]
    pub no_prepare: bool,

    /// Hides elapsed time after each task completes
    ///
    /// Default to always hide with `MISE_TASK_TIMINGS=0`
    #[clap(long, alias = "no-timing", verbatim_doc_comment)]
    pub no_timings: bool,

    /// Print the static execution plan and exit without executing tasks
    ///
    /// Optional formats:
    /// - `summary` (default)
    /// - `json`
    /// - `explain`
    ///
    /// Plan output includes each task declaration reference (`source:line`) to
    /// make static DAG tracking easier across large monorepos.
    ///
    /// Examples:
    /// - `--plan`
    /// - `--plan=json`
    /// - `--plan=explain`
    #[clap(
        long,
        value_enum,
        num_args = 0..=1,
        default_missing_value = "summary",
        require_equals = true,
        verbatim_doc_comment
    )]
    pub plan: Option<PlanOutputMode>,

    /// Run only the specified tasks skipping all dependencies
    #[clap(long, verbatim_doc_comment, env = "MISE_TASK_SKIP_DEPENDS")]
    pub skip_deps: bool,

    /// Timeout for the task to complete
    /// e.g.: 30s, 5m
    #[clap(long, verbatim_doc_comment)]
    pub timeout: Option<String>,

    /// Shows elapsed time after each task completes
    ///
    /// Default to always show with `MISE_TASK_TIMINGS=1`
    #[clap(long, alias = "timing", verbatim_doc_comment, hide = true)]
    pub timings: bool,

    #[clap(skip)]
    pub tmpdir: PathBuf,

    #[clap(skip)]
    pub output_handler: Option<OutputHandler>,

    #[clap(skip)]
    pub context_builder: crate::task::task_context_builder::TaskContextBuilder,

    #[clap(skip)]
    pub executor: Option<crate::task::task_executor::TaskExecutor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PlanOutputMode {
    Summary,
    Json,
    Explain,
}

struct InFlightGuard {
    in_flight: Arc<AtomicUsize>,
}

impl InFlightGuard {
    fn new(in_flight: Arc<AtomicUsize>) -> Self {
        in_flight.fetch_add(1, Ordering::SeqCst);
        Self { in_flight }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Run {
    pub async fn run(mut self) -> Result<()> {
        // Check help flags before doing any work
        if self.task == "-h" {
            self.get_clap_command().print_help()?;
            return Ok(());
        }
        if self.task == "--help" {
            self.get_clap_command().print_long_help()?;
            return Ok(());
        }

        // Unescape task args early so we can check for help flags
        self.args = unescape_task_args(&self.args);

        // Temporarily unset cache key to force fresh env computation
        if self.fresh_env {
            env::reset_env_cache_key();
        }

        // Check if --help or -h is in the task args BEFORE toolset/prepare
        // NOTE: Only check self.args, not self.args_last, because args_last contains
        // arguments after explicit -- which should always be passed through to the task
        let has_help_in_task_args =
            self.args.contains(&"--help".to_string()) || self.args.contains(&"-h".to_string());

        let mut config = Config::get().await?;

        // Handle task help early to avoid unnecessary toolset/prepare work
        if has_help_in_task_args {
            // Build args list to get the task (filter out --help/-h for task lookup)
            let args = once(self.task.clone())
                .chain(
                    self.args
                        .iter()
                        .filter(|a| *a != "--help" && *a != "-h")
                        .cloned(),
                )
                .collect_vec();

            let task_list = get_task_lists(&config, &args, false, false).await?;

            if let Some(task) = task_list.first() {
                // Get usage spec to check if task has defined args/flags
                let spec = task.parse_usage_spec_for_display(&config).await?;

                if has_any_args_defined(&spec) {
                    // Task has usage args/flags defined, render help using usage library
                    println!("{}", usage::docs::cli::render_help(&spec, &spec.cmd, true));
                } else {
                    // Task has no usage defined, show basic task info
                    display_task_help(task)?;
                }
                return Ok(());
            } else {
                // No task found, show run command help
                self.get_clap_command().print_long_help()?;
                return Ok(());
            }
        }

        // Build and install toolset so tools like npm are available for prepare/runtime.
        // In --plan mode we skip this to keep planning fast and side-effect free.
        let mut ts = if self.plan.is_some() {
            None
        } else {
            let mut ts = ToolsetBuilder::new()
                .with_args(&self.tool)
                .with_default_to_latest(true)
                .build(&config)
                .await?;

            let opts = InstallOptions {
                jobs: self.jobs,
                raw: self.raw,
                ..Default::default()
            };
            let _ = ts.install_missing_versions(&mut config, &opts).await?;
            Some(ts)
        };

        if !self.skip_deps {
            self.skip_deps = Settings::get().task.skip_depends;
        }

        time!("run init");
        let tmpdir = tempfile::tempdir()?;
        self.tmpdir = tmpdir.path().to_path_buf();

        // Build args list - don't include args_last yet, they'll be added after task resolution
        let args = once(self.task.clone())
            .chain(self.args.clone())
            .collect_vec();

        let mut task_list = get_task_lists(&config, &args, true, self.skip_deps).await?;

        // Args after -- go directly to tasks (no prefix)
        if !self.args_last.is_empty() {
            for task in &mut task_list {
                task.args.extend(self.args_last.clone());
            }
        }
        time!("run get_task_lists");

        // Resolve transitive dependencies once upfront so we can:
        // 1. Discover prepare providers from monorepo subdirectory configs
        // 2. Reuse the resolved list for execution (avoiding duplicate work)
        let resolved_tasks = resolve_depends(&config, task_list).await?;

        // Run auto-enabled prepare steps (unless --no-prepare)
        if self.plan.is_none() && !self.no_prepare {
            let env = ts
                .as_mut()
                .expect("toolset must exist when not in --plan mode")
                .env_with_path(&config)
                .await?;
            let mut engine = PrepareEngine::new(&config)?;

            // Collect subdirectory config files from all resolved tasks
            let subdir_configs: Vec<_> = resolved_tasks
                .iter()
                .filter_map(|task| task.cf.clone())
                .collect();
            if !subdir_configs.is_empty() {
                engine.add_config_files(subdir_configs);
            }

            engine
                .run(PrepareOptions {
                    auto_only: true, // Only run providers with auto=true
                    env,
                    ..Default::default()
                })
                .await?;
        }

        // Apply global timeout for entire run if configured
        let timeout = if let Some(timeout_str) = &self.timeout {
            Some(duration::parse_duration(timeout_str)?)
        } else {
            Settings::get().task_timeout_duration()
        };

        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, self.parallelize_tasks(config, resolved_tasks))
                .await
                .map_err(|_| eyre!("mise run timed out after {:?}", timeout))??
        } else {
            self.parallelize_tasks(config, resolved_tasks).await?
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

        // Step 1: Prepare tasks (resolve dependencies, fetch, validate)
        let jobs = self.effective_jobs_for_plan();
        let bundle = build_execution_plan_bundle_from_resolved_tasks(
            &config,
            PlanBuildResolvedTasksRequest {
                requested_task_specs: vec![],
                resolved_cli_args: vec![],
                resolved_tasks: tasks,
                changed_files: self.changed.clone(),
                jobs,
                deps_skip_deps: self.skip_deps,
                fetch_remote: true,
                no_cache: self.no_cache,
            },
        )
        .await?;
        let plan_hash = bundle.plan_hash.clone();
        let tasks = bundle.deps;
        self.is_linear = tasks.is_linear();
        self.validate_pre_run(&tasks, bundle.cycle.as_ref(), bundle.plan.as_ref())?;
        let execution_plan = bundle
            .plan
            .ok_or_else(|| eyre!("execution plan missing after pre-run validation"))?;
        let num_tasks = tasks.all().count();
        let change_impact = bundle.change_impact;
        let contention = bundle.contention.unwrap_or_default();

        // Step 2: Validate tasks and optionally print plan
        if let Some(mode) = self.plan {
            miseprintln!(
                "{}",
                format_plan_output(&execution_plan, mode, &change_impact, &contention)?
            );
            return Ok(());
        }
        self.validate_runtime_tasks(&tasks)?;

        // Step 3: Setup output handler
        self.setup_output_and_validate(&tasks, &execution_plan)?;
        self.output = Some(self.output(None));

        // Step 4: Install tools needed by tasks
        self.install_task_tools(&mut config, &tasks).await?;

        // Step 5: Create TaskExecutor after tool installation
        self.setup_executor(&execution_plan)?;

        let timer = std::time::Instant::now();
        let this = Arc::new(self);
        let config = config.clone();

        // Step 6: Initialize scheduler and run tasks
        let mut scheduler = crate::task::task_scheduler::Scheduler::new(this.jobs());
        scheduler.set_plan_trace_context(&execution_plan, plan_hash);
        let main_deps = Arc::new(Mutex::new(tasks));

        // Pump deps leaves into scheduler
        let mut main_done_rx = scheduler.pump_deps(main_deps.clone()).await;
        let spawn_context = scheduler.spawn_context(config.clone());
        scheduler
            .run_loop(
                &mut main_done_rx,
                main_deps.clone(),
                || this.is_stopping(),
                this.continue_on_error,
                |task, deps_for_remove| {
                    let this = this.clone();
                    let spawn_context = spawn_context.clone();
                    async move {
                        Self::spawn_sched_job(this, task, deps_for_remove, spawn_context).await
                    }
                },
            )
            .await?;

        scheduler.join_all(this.continue_on_error).await?;

        // Step 7: Display results and handle failures
        let results_display = crate::task::task_results_display::TaskResultsDisplay::new(
            this.output_handler.clone().unwrap(),
            this.executor.as_ref().unwrap().failed_tasks.clone(),
            this.continue_on_error,
            this.timings(),
            execution_plan.clone(),
        );
        results_display.display_results(num_tasks, timer);
        time!("parallelize_tasks done");

        Ok(())
    }

    async fn spawn_sched_job(
        this: Arc<Self>,
        task: Task,
        deps_for_remove: Arc<Mutex<Deps>>,
        ctx: crate::task::task_scheduler::SpawnContext,
    ) -> Result<()> {
        // If we're already stopping due to a previous failure and not in
        // continue-on-error mode, do not launch this task unless it's a
        // post-dependency (cleanup task that should run even on failure).
        if this
            .maybe_skip_spawn_due_to_failure(&task, &deps_for_remove, "before start")
            .await
        {
            if let Some(plan_stage_barrier) = &ctx.plan_stage_barrier {
                plan_stage_barrier.mark_task_complete(&task);
            }
            return Ok(());
        }
        if let Some(plan_stage_barrier) = &ctx.plan_stage_barrier {
            trace!("waiting for static plan stage turn: {}", task.name);
            plan_stage_barrier.wait_for_task_stage(&task).await;
            if this
                .maybe_skip_spawn_due_to_failure(&task, &deps_for_remove, "after stage wait")
                .await
            {
                plan_stage_barrier.mark_task_complete(&task);
                return Ok(());
            }
        }
        let barrier_mode = spawn_barrier_mode(&task);
        let permit_opt = if barrier_mode.is_some() {
            let wait_start = std::time::Instant::now();
            let p = Some(ctx.semaphore.clone().acquire_owned().await?);
            trace!(
                "semaphore acquired for {} after {}ms",
                task.name,
                wait_start.elapsed().as_millis()
            );
            // If a failure occurred while we were waiting for a permit and we're not
            // in continue-on-error mode, skip launching this task unless it's a
            // post-dependency (cleanup task). This prevents subsequently queued
            // tasks from running after failure, while still allowing cleanup.
            if this
                .maybe_skip_spawn_due_to_failure(&task, &deps_for_remove, "after permit wait")
                .await
            {
                if let Some(plan_stage_barrier) = &ctx.plan_stage_barrier {
                    plan_stage_barrier.mark_task_complete(&task);
                }
                return Ok(());
            }
            p
        } else {
            trace!("no semaphore needed for orchestrator task: {}", task.name);
            None
        };

        let runtime_barrier_guard = if let Some(mode) = barrier_mode {
            let wait_start = std::time::Instant::now();
            let guard = match mode {
                SpawnBarrierMode::Interactive => {
                    trace!("waiting for interactive barrier: {}", task.name);
                    ctx.interactive_barrier.acquire_interactive().await
                }
                SpawnBarrierMode::Runtime => {
                    trace!("waiting for runtime barrier slot: {}", task.name);
                    ctx.interactive_barrier.acquire_runtime().await
                }
            };
            trace!(
                "barrier acquired for {} after {}ms",
                task.name,
                wait_start.elapsed().as_millis()
            );
            // If a failure occurred while we were waiting for the interactive barrier and
            // we're not in continue-on-error mode, skip launching this task unless it's a
            // post-dependency (cleanup task).
            if this
                .maybe_skip_spawn_due_to_failure(&task, &deps_for_remove, "after barrier wait")
                .await
            {
                if let Some(plan_stage_barrier) = &ctx.plan_stage_barrier {
                    plan_stage_barrier.mark_task_complete(&task);
                }
                return Ok(());
            }
            Some(guard)
        } else {
            None
        };

        let in_flight_guard = InFlightGuard::new(ctx.in_flight.clone());
        let plan_stage_barrier = ctx.plan_stage_barrier.clone();
        trace!("running task: {task}");
        // Mark task as executed synchronously before spawning so that the
        // scheduler's failure-cleanup path (which checks is_runnable_post_dep)
        // always sees the parent in `executed` — avoiding a race where a
        // concurrent task fails between spawn and first poll.
        deps_for_remove.lock().await.mark_executed(&task);
        ctx.jset.lock().await.spawn(async move {
            let _in_flight_guard = in_flight_guard;
            let _permit = permit_opt;
            let _runtime_barrier_guard = runtime_barrier_guard;
            let result = this.run_task_sched(&task, &ctx.config).await;
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
            if let Some(oh) = &this.output_handler
                && oh.output(None) == TaskOutput::KeepOrder
            {
                oh.keep_order_state.lock().unwrap().on_task_finished(&task);
            }
            deps_for_remove.lock().await.remove(&task);
            if let Some(plan_stage_barrier) = &plan_stage_barrier {
                plan_stage_barrier.mark_task_complete(&task);
            }
            trace!("deps removed: {} {}", task.name, task.args.join(" "));
            result
        });

        Ok(())
    }

    async fn maybe_skip_spawn_due_to_failure(
        &self,
        task: &Task,
        deps_for_remove: &Arc<Mutex<Deps>>,
        phase: &str,
    ) -> bool {
        if !self.is_stopping() || self.continue_on_error {
            return false;
        }

        let mut deps = deps_for_remove.lock().await;
        let should_skip = should_skip_spawn(
            self.is_stopping(),
            self.continue_on_error,
            deps.is_runnable_post_dep(task),
        );
        if should_skip {
            trace!(
                "aborting spawn {phase} (not continue-on-error): {} {}",
                task.name,
                task.args.join(" ")
            );
            deps.remove(task);
            return true;
        }
        false
    }

    // ============================================================================
    // High-level workflow methods
    // ============================================================================

    /// Initialize output handler and validate tasks
    fn setup_output_and_validate(
        &mut self,
        tasks: &Deps,
        execution_plan: &ExecutionPlan,
    ) -> Result<()> {
        let interactive_tasks = if self.dry_run || std::io::stdin().is_terminal() {
            vec![]
        } else {
            execution_plan.interactive_task_names()
        };
        if !interactive_tasks.is_empty() {
            bail!(
                "interactive task(s) require a TTY on stdin, but stdin is not a TTY: {}",
                interactive_tasks.join(", ")
            );
        }

        // Initialize OutputHandler AFTER is_linear is determined
        let output_config = crate::task::task_output_handler::OutputHandlerConfig {
            prefix: self.prefix,
            interleave: self.interleave,
            output: self.output,
            silent: self.silent,
            quiet: self.quiet,
            raw: self.raw,
            is_linear: self.is_linear,
            jobs: self.jobs,
        };
        self.output_handler = Some(OutputHandler::new(output_config));

        // Spawn timed output task if needed
        if self.output(None) == TaskOutput::Timed {
            let timed_outputs = self.output_handler.as_ref().unwrap().timed_outputs.clone();
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

        // Validate and initialize task output
        for task in tasks.all() {
            self.output_handler.as_mut().unwrap().init_task(task);
        }

        Ok(())
    }

    fn validate_pre_run(
        &self,
        tasks: &Deps,
        cycle: Option<&GraphCycle>,
        execution_plan: Option<&ExecutionPlan>,
    ) -> Result<()> {
        let mut errors = Vec::new();

        if let Some(cycle) = cycle {
            errors.push(format!(
                "circular dependency detected in static DAG: {}",
                cycle_path_label(cycle)
            ));
        }

        let plan_context = execution_plan.map(|plan| PlanContextIndex::from_plan(plan, None));

        for task in tasks.all() {
            if let Some(msg) = task.interactive_validation_error() {
                let declaration = task_validation_declaration(task, plan_context.as_ref());
                let stage_suffix = task_validation_stage_suffix(task, plan_context.as_ref());
                errors.push(format!(
                    "invalid task `{}` ({}): {msg}{stage_suffix}",
                    task.name, declaration,
                ));
            }
        }
        errors.extend(tasks.validation_errors().iter().map(ToString::to_string));

        if !errors.is_empty() {
            let body = errors
                .into_iter()
                .map(|e| format!("  - {e}"))
                .collect_vec()
                .join("\n");
            bail!(
                "pre-run validation failed:\n{body}\n\nUse `mise run --plan=explain ...` after fixing errors to inspect the static plan."
            );
        }
        Ok(())
    }

    fn validate_runtime_tasks(&self, tasks: &Deps) -> Result<()> {
        for task in tasks.all() {
            self.validate_runtime_task(task)?;
        }
        Ok(())
    }

    /// Create TaskExecutor after tool installation to ensure caches are populated
    fn setup_executor(&mut self, execution_plan: &ExecutionPlan) -> Result<()> {
        let executor_config = crate::task::task_executor::TaskExecutorConfig {
            force: self.force,
            cd: self.cd.clone(),
            shell: self.shell.clone(),
            tool: self.tool.clone(),
            timings: self.timings,
            dry_run: self.dry_run,
        };
        self.executor = Some(crate::task::task_executor::TaskExecutor::new(
            self.context_builder.clone(),
            self.output_handler.clone().unwrap(),
            executor_config,
            Some(execution_plan),
            execution_plan_hash(execution_plan).ok(),
        ));

        Ok(())
    }

    /// Collect and install all tools needed by tasks
    async fn install_task_tools(&self, config: &mut Arc<Config>, tasks: &Deps) -> Result<()> {
        let installer = crate::task::task_tool_installer::TaskToolInstaller::new(
            &self.context_builder,
            &self.tool,
        );
        installer.install_tools(config, tasks).await
    }

    // ============================================================================
    // Helper methods
    // ============================================================================

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        self.output_handler
            .as_ref()
            .unwrap()
            .eprint(task, prefix, line);
    }

    fn output(&self, task: Option<&Task>) -> TaskOutput {
        self.output_handler.as_ref().unwrap().output(task)
    }

    fn jobs(&self) -> usize {
        self.output_handler.as_ref().unwrap().jobs()
    }

    fn is_stopping(&self) -> bool {
        self.executor
            .as_ref()
            .map(|e| e.is_stopping())
            .unwrap_or(false)
    }

    async fn run_task_sched(&self, task: &Task, config: &Arc<Config>) -> Result<()> {
        self.executor
            .as_ref()
            .expect("executor must be initialized before running tasks")
            .run_task_sched(task, config)
            .await
    }

    fn add_failed_task(&self, task: Task, status: Option<i32>) {
        if let Some(executor) = &self.executor {
            executor.add_failed_task(task, status);
        }
    }

    fn validate_runtime_task(&self, task: &Task) -> Result<()> {
        use crate::file;
        use crate::ui;
        if let Some(path) = &task.file
            && path.exists()
            && !file::is_executable(path)
        {
            let dp = crate::file::display_path(path);
            let msg = format!("Script `{dp}` is not executable. Make it executable?");
            if ui::confirm(msg)? {
                file::make_executable(path)?;
            } else {
                bail!("`{dp}` is not executable")
            }
        }
        Ok(())
    }

    fn timings(&self) -> bool {
        !self.quiet(None) && !self.no_timings
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.output_handler.as_ref().unwrap().quiet(task)
    }

    fn effective_jobs_for_plan(&self) -> usize {
        if self.raw || Settings::get().raw {
            1
        } else {
            self.jobs.unwrap_or(Settings::get().jobs)
        }
    }
}

fn task_validation_declaration(task: &Task, plan_context: Option<&PlanContextIndex>) -> String {
    if let Some(context) = plan_context {
        return context.declaration_for_task(task);
    }
    format_declaration_location(&task_declaration_ref(task))
}

fn task_validation_stage_suffix(task: &Task, plan_context: Option<&PlanContextIndex>) -> String {
    plan_context
        .map(|ctx| ctx.stage_suffix_for_task(task))
        .unwrap_or_default()
}

fn display_task_help(task: &Task) -> Result<()> {
    let name = if task.display_name.is_empty() {
        &task.name
    } else {
        &task.display_name
    };
    info::inline_section("Task", name)?;
    if !task.aliases.is_empty() {
        info::inline_section("Aliases", task.aliases.join(", "))?;
    }
    if !task.description.is_empty() {
        info::inline_section("Description", &task.description)?;
    }
    info::inline_section("Source", display_path(&task.config_source))?;
    if !task.depends.is_empty() {
        info::inline_section("Depends on", task.depends.iter().join(", "))?;
    }
    let run = task.run();
    if !run.is_empty() {
        info::section("Run", run.iter().map(|e| e.to_string()).join("\n"))?;
    }
    miseprintln!();
    miseprintln!("This task does not accept any arguments.");
    let hint = if task.file.is_some() {
        "To define arguments, add #USAGE comments to the script file."
    } else {
        "To define arguments, add a `usage` field to the task definition in the config file."
    };
    miseprintln!("{hint}");
    miseprintln!("See https://mise.jdx.dev/tasks/task-configuration.html for more information.");
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Runs the "lint" tasks. This needs to either be defined in mise.toml
    # or as a standalone script. See the project README for more information.
    $ <bold>mise run lint</bold>

    # Forces the "build" tasks to run even if its sources are up-to-date.
    $ <bold>mise run --force build</bold>

    # Run "test" with stdin/stdout/stderr all connected to the current terminal.
    # This forces `--jobs=1` to prevent interleaving of output.
    $ <bold>mise run --raw test</bold>

    # Runs the "lint", "test", and "check" tasks in parallel.
    $ <bold>mise run lint ::: test ::: check</bold>

    # Execute multiple tasks each with their own arguments.
    $ <bold>mise run cmd1 arg1 arg2 ::: cmd2 arg1 arg2</bold>

    # Print the static execution plan without running tasks.
    $ <bold>mise run --plan test</bold>
    $ <bold>mise run --plan=json test</bold>
    $ <bold>mise run --plan=explain test</bold>
    $ <bold>mise run --plan=explain --changed=src/main.ts test</bold>
"#
);

fn format_planned_task_summary(task: &PlannedTask) -> String {
    let (label, origin) = format_planned_task_summary_parts(task);
    format!("{label} {origin}")
}

fn format_planned_task_summary_parts(task: &PlannedTask) -> (String, String) {
    let base_name = planned_task_base_name(task);
    let kind = summary_task_kind(task, &base_name);
    let mut label = format!(
        "{} {}",
        summary_task_badge(kind),
        summary_task_name(kind, &base_name)
    );
    let suffix = planned_task_suffix(task);
    if !suffix.is_empty() {
        label = format!("{label}{suffix}");
    }
    let origin = planned_task_origin_summary(task);
    (label, origin)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SummaryTaskKind {
    Task,
    Script,
    Interactive,
}

fn stage_kind_label(kind: ExecutionStageKind) -> &'static str {
    match kind {
        ExecutionStageKind::Parallel => "parallel",
        ExecutionStageKind::InteractiveExclusive => "interactive",
    }
}

fn stage_kind_display(kind: ExecutionStageKind) -> &'static str {
    match kind {
        ExecutionStageKind::Parallel => "runnable",
        ExecutionStageKind::InteractiveExclusive => "blocking (interactive)",
    }
}

fn planned_task_base_name(task: &PlannedTask) -> String {
    if task.identity.name.starts_with(STATIC_INTERNAL_TASK_PREFIX) {
        prettify_static_task_name(&task.identity.name)
    } else {
        task.identity.name.clone()
    }
}

fn planned_task_suffix(task: &PlannedTask) -> String {
    let mut suffix = String::new();
    if !task.identity.args.is_empty() {
        suffix.push(' ');
        suffix.push_str(&task.identity.args.join(" "));
    }
    if !task.identity.env.is_empty() {
        let env = task
            .identity
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        suffix.push(' ');
        suffix.push_str(&format!("{{{env}}}"));
    }
    suffix
}

fn prettify_static_task_name(raw: &str) -> String {
    let cleaned = raw.replace(STATIC_INTERNAL_TASK_PREFIX, "");
    let mut parts = cleaned
        .split("::")
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>();
    while parts.len() > 1
        && parts
            .last()
            .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
    {
        parts.pop();
    }
    parts
        .into_iter()
        .map(|p| match p {
            "script" => "inline-script",
            STATIC_BARRIER_START_SEGMENT => "barrier-start",
            STATIC_BARRIER_END_SEGMENT => "barrier-end",
            _ => p,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn planned_task_origin(task: &PlannedTask) -> String {
    if declaration_source_is_generated(task.declaration.source.as_str()) {
        if task.identity.name.starts_with(STATIC_INTERNAL_TASK_PREFIX) {
            if task
                .identity
                .name
                .contains(&format!("::{STATIC_BARRIER_START_SEGMENT}::"))
                || task
                    .identity
                    .name
                    .contains(&format!("::{STATIC_BARRIER_END_SEGMENT}::"))
            {
                return "virtual barrier task".to_string();
            }
            return "virtual generated task".to_string();
        }
        return "generated task".to_string();
    }

    format!(
        "declared at {}",
        format_declaration_location(&task.declaration)
    )
}

fn planned_task_origin_summary(task: &PlannedTask) -> String {
    if declaration_source_is_generated(task.declaration.source.as_str()) {
        return style::nstyle(format!("[{}]", planned_task_origin(task)))
            .black()
            .bright()
            .to_string();
    }

    let source_with_line = format_declaration_location(&task.declaration);
    let (dir, file_line) = split_path_dir_file(source_with_line.as_str());

    format!(
        "{}{}{}{}",
        style::nstyle("[declared at ").black().bright(),
        style::nstyle(dir).black().bright().dim(),
        style::nstyle(file_line).black().bright().bold(),
        style::nstyle("]").black().bright()
    )
}

fn split_path_dir_file(path: &str) -> (&str, &str) {
    if let Some(idx) = path.rfind(['/', '\\']) {
        path.split_at(idx + 1)
    } else {
        ("", path)
    }
}

fn explain_task_runtime_interactive(task: &PlannedTask) -> String {
    let runtime_value = if task.runtime {
        style::nstyle(task.runtime).green().bold().to_string()
    } else {
        style::nstyle(task.runtime).dim().to_string()
    };
    let interactive_value = if task.interactive {
        style::nstyle(task.interactive).yellow().bold().to_string()
    } else {
        style::nstyle(task.interactive).dim().to_string()
    };

    let runtime = if task.runtime {
        format!(
            "{}={} {}",
            style::nstyle("runtime").dim(),
            runtime_value,
            style::nstyle("(spawns a user process)").dim()
        )
    } else {
        format!("{}={}", style::nstyle("runtime").dim(), runtime_value)
    };

    let interactive = if task.interactive {
        format!(
            "{}={} {}",
            style::nstyle("interactive").dim(),
            interactive_value,
            style::nstyle("(global exclusive barrier for runtime tasks)").dim()
        )
    } else {
        format!(
            "{}={}",
            style::nstyle("interactive").dim(),
            interactive_value
        )
    };

    format!("{runtime} {interactive}")
}

fn summary_task_kind(task: &PlannedTask, base_name: &str) -> SummaryTaskKind {
    if task.interactive {
        SummaryTaskKind::Interactive
    } else if base_name == "inline-script" || base_name.ends_with("/inline-script") {
        SummaryTaskKind::Script
    } else {
        SummaryTaskKind::Task
    }
}

fn summary_task_badge(kind: SummaryTaskKind) -> String {
    summary_task_badge_with_min_width(kind, 0)
}

fn summary_task_badge_with_min_width(kind: SummaryTaskKind, min_width: usize) -> String {
    let raw = match kind {
        SummaryTaskKind::Task => "[task]",
        SummaryTaskKind::Script => "[script]",
        SummaryTaskKind::Interactive => "[interactive]",
    };
    let padded = if min_width > 0 {
        format!("{raw:<min_width$}")
    } else {
        raw.to_string()
    };
    match kind {
        SummaryTaskKind::Task => style::nstyle(padded).blue().to_string(),
        SummaryTaskKind::Script => style::nstyle(padded).magenta().to_string(),
        SummaryTaskKind::Interactive => style::nstyle(padded).yellow().bold().to_string(),
    }
}

fn summary_task_name(kind: SummaryTaskKind, name: &str) -> String {
    let (dir, leaf) = if let Some((prefix, leaf)) = name.rsplit_once('/') {
        (format!("{prefix}/"), leaf.to_string())
    } else {
        (String::new(), name.to_string())
    };
    let dir_styled = match kind {
        SummaryTaskKind::Task => style::nstyle(dir).blue().dim().to_string(),
        SummaryTaskKind::Script => style::nstyle(dir).magenta().dim().to_string(),
        SummaryTaskKind::Interactive => style::nstyle(dir).cyan().dim().to_string(),
    };
    let leaf_styled = match kind {
        SummaryTaskKind::Task => style::nstyle(leaf).blue().bold().to_string(),
        SummaryTaskKind::Script => style::nstyle(leaf).magenta().bold().to_string(),
        SummaryTaskKind::Interactive => style::nstyle(leaf).yellow().bold().to_string(),
    };
    format!("{dir_styled}{leaf_styled}")
}

fn barrier_scope_start(task: &PlannedTask) -> Option<String> {
    if !task.identity.name.starts_with(STATIC_INTERNAL_TASK_PREFIX)
        || !task
            .identity
            .name
            .contains(&format!("::{STATIC_BARRIER_START_SEGMENT}::"))
    {
        return None;
    }
    let pretty = prettify_static_task_name(&task.identity.name);
    Some(
        pretty
            .strip_suffix("/barrier-start")
            .unwrap_or(&pretty)
            .to_string(),
    )
}

fn barrier_scope_end(task: &PlannedTask) -> Option<String> {
    if !task.identity.name.starts_with(STATIC_INTERNAL_TASK_PREFIX)
        || !task
            .identity
            .name
            .contains(&format!("::{STATIC_BARRIER_END_SEGMENT}::"))
    {
        return None;
    }
    let pretty = prettify_static_task_name(&task.identity.name);
    Some(
        pretty
            .strip_suffix("/barrier-end")
            .unwrap_or(&pretty)
            .to_string(),
    )
}

fn format_execution_plan_summary(plan: &ExecutionPlan) -> Result<String> {
    let hash = execution_plan_hash(plan)?;
    let mut lines = vec![format!(
        "{} {}",
        style::nstyle(format!("Execution plan: {} stage(s)", plan.stages.len())).bold(),
        style::nstyle(format!("[{hash}]")).dim(),
    )];
    let config_files = execution_plan_config_files(plan);
    if !config_files.is_empty() {
        lines.push(
            style::nstyle(format!("Config files used ({}):", config_files.len()))
                .dim()
                .to_string(),
        );
        for cf in config_files {
            lines.push(format!("  - {}", style::nstyle(cf).dim()));
        }
    }
    lines.push(
        style::nstyle(
            "Order: stage number is execution order; tasks inside one stage are runnable together.",
        )
        .dim()
        .to_string(),
    );
    let executable_stage_count = plan
        .stages
        .iter()
        .filter(|stage| {
            if stage.tasks.len() != 1 {
                return true;
            }
            let task = &stage.tasks[0];
            barrier_scope_start(task).is_none() && barrier_scope_end(task).is_none()
        })
        .count();
    let stage_number_width = executable_stage_count.to_string().len().max(3);
    let mut barrier_depth = 0usize;
    let mut executable_stage_idx = 0usize;
    let virtual_prefix = " ".repeat(stage_number_width + 1);
    let continuation_prefix = " ".repeat(stage_number_width + 2);
    for stage in &plan.stages {
        if stage.tasks.len() == 1
            && let Some(scope) = barrier_scope_start(&stage.tasks[0])
        {
            let indent = "│  ".repeat(barrier_depth);
            let barrier = style::nstyle(format!("┌─ group: {scope}"))
                .cyan()
                .to_string();
            lines.push(format!("{virtual_prefix} {indent}{barrier}"));
            barrier_depth += 1;
            continue;
        }
        if stage.tasks.len() == 1
            && let Some(scope) = barrier_scope_end(&stage.tasks[0])
        {
            barrier_depth = barrier_depth.saturating_sub(1);
            let indent = "│  ".repeat(barrier_depth);
            let barrier = style::nstyle(format!("└─ group: {scope}"))
                .cyan()
                .to_string();
            lines.push(format!("{virtual_prefix} {indent}{barrier}"));
            continue;
        }
        executable_stage_idx += 1;
        let stage_num = format!(
            "{:>width$}",
            executable_stage_idx,
            width = stage_number_width
        );
        let stage_prefix = style::nstyle(format!("{stage_num}.")).dim().to_string();
        let indent = "│  ".repeat(barrier_depth);
        if stage.kind == ExecutionStageKind::Parallel && stage.tasks.len() > 1 {
            let header = style::nstyle(format!("∥ parallel ({})", stage.tasks.len()))
                .cyan()
                .dim()
                .to_string();
            lines.push(format!("{stage_prefix} {indent}{header}"));
            for (task_idx, task) in stage.tasks.iter().enumerate() {
                let is_last = task_idx + 1 == stage.tasks.len();
                let branch = if is_last { "└─" } else { "├─" };
                let connector = if is_last { "   " } else { "│  " };

                let base_name = planned_task_base_name(task);
                let kind = summary_task_kind(task, &base_name);
                let badge = summary_task_badge_with_min_width(kind, "[interactive]".len());
                let mut label = format!("{badge} {}", summary_task_name(kind, &base_name));
                let suffix = planned_task_suffix(task);
                if !suffix.is_empty() {
                    label = format!("{label}{suffix}");
                }
                let origin = planned_task_origin_summary(task);

                lines.push(format!("{continuation_prefix}{indent}{branch} {label}"));
                lines.push(format!("{continuation_prefix}{indent}{connector} {origin}"));
            }
        } else {
            let tasks = stage
                .tasks
                .iter()
                .map(format_planned_task_summary)
                .join(", ");
            lines.push(format!("{stage_prefix} {indent}{tasks}"));
        }
    }
    Ok(lines.join("\n"))
}

pub(crate) fn render_execution_plan_explain(
    plan: &ExecutionPlan,
    change_impact: &ChangeImpact,
    contention: &ContentionAnalysis,
) -> Result<String> {
    let hash = execution_plan_hash(plan)?;
    let stats = execution_plan_stats(plan);
    let executable_stage_count = plan
        .stages
        .iter()
        .filter(|stage| {
            if stage.tasks.len() != 1 {
                return true;
            }
            let task = &stage.tasks[0];
            barrier_scope_start(task).is_none() && barrier_scope_end(task).is_none()
        })
        .count();
    let config_files = execution_plan_config_files(plan);
    let mut lines = vec![
        format!(
            "{} {}",
            style::nstyle("Plan:").bold(),
            style::nstyle("valid (static)").green().bold()
        ),
        format!(
            "{} {}",
            style::nstyle("Hash:").dim(),
            style::nstyle(hash).black().bright()
        ),
        format!(
            "{} {} | {} {} | {} {} | {} {} | {} {} | {} {}",
            style::nstyle("Stages:").dim(),
            style::nstyle(stats.stage_count).bold(),
            style::nstyle("Executable stages:").dim(),
            style::nstyle(executable_stage_count).bold(),
            style::nstyle("Tasks:").dim(),
            style::nstyle(stats.task_count).bold(),
            style::nstyle("Runtime:").dim(),
            style::nstyle(stats.runtime_count).bold(),
            style::nstyle("Interactive:").dim(),
            style::nstyle(stats.interactive_count).yellow().bold(),
            style::nstyle("Orchestrator:").dim(),
            style::nstyle(stats.orchestrator_count).bold()
        ),
        style::nstyle(
            "Determinism: stable topological stages + lexicographic tie-break on (task.name, args, env)",
        )
        .dim()
        .to_string(),
        String::new(),
    ];
    if !config_files.is_empty() {
        lines.push(
            style::nstyle(format!("Config files used ({}):", config_files.len()))
                .cyan()
                .bold()
                .to_string(),
        );
        for cf in config_files {
            let (dir, file) = split_path_dir_file(cf.as_str());
            lines.push(format!(
                "  - {}{}",
                style::nstyle(dir).black().bright().dim(),
                style::nstyle(file).black().bright().bold()
            ));
        }
        lines.push(String::new());
    }

    let mut barrier_depth = 0usize;
    let mut executable_stage_idx = 0usize;
    for stage in &plan.stages {
        if stage.tasks.len() == 1
            && let Some(scope) = barrier_scope_start(&stage.tasks[0])
        {
            let indent = "│  ".repeat(barrier_depth);
            lines.push(format!(
                "{indent}{}",
                style::nstyle(format!("┌─ group: {scope}")).cyan()
            ));
            barrier_depth += 1;
            continue;
        }
        if stage.tasks.len() == 1
            && let Some(scope) = barrier_scope_end(&stage.tasks[0])
        {
            barrier_depth = barrier_depth.saturating_sub(1);
            let indent = "│  ".repeat(barrier_depth);
            lines.push(format!(
                "{indent}{}",
                style::nstyle(format!("└─ group: {scope}")).cyan()
            ));
            continue;
        }

        executable_stage_idx += 1;
        let kind = stage_kind_display(stage.kind);
        let kind_styled = match stage.kind {
            ExecutionStageKind::Parallel => style::nstyle(kind).blue().bold().to_string(),
            ExecutionStageKind::InteractiveExclusive => {
                style::nstyle(kind).yellow().bold().to_string()
            }
        };
        let why = match stage.kind {
            ExecutionStageKind::Parallel => {
                "tasks in this stage can run after prior stages are complete (subject to jobs)"
            }
            ExecutionStageKind::InteractiveExclusive => {
                "interactive exclusivity: this stage blocks concurrent runtime starts"
            }
        };
        let indent = "│  ".repeat(barrier_depth);
        lines.push(format!(
            "{indent}{} {} [{}]",
            style::nstyle("Stage").bold(),
            style::nstyle(executable_stage_idx).bold(),
            kind_styled
        ));
        lines.push(format!(
            "{indent}  {} {}",
            style::nstyle("Hash:").dim(),
            style::nstyle(execution_stage_hash(stage)?).black().bright()
        ));
        lines.push(format!(
            "{indent}  {} {}",
            style::nstyle("Why:").dim(),
            style::nstyle(why).dim()
        ));
        if stage.kind == ExecutionStageKind::Parallel && stage.tasks.len() > 1 {
            lines.push(format!(
                "{indent}{}",
                style::nstyle(format!("∥ parallel ({})", stage.tasks.len()))
                    .cyan()
                    .dim()
            ));
            for (task_idx, task) in stage.tasks.iter().enumerate() {
                let is_last = task_idx + 1 == stage.tasks.len();
                let branch = if is_last { "└─" } else { "├─" };
                let connector = if is_last { "   " } else { "│  " };
                let (label, origin) = format_planned_task_summary_parts(task);
                lines.push(format!("{indent}{branch} {label}"));
                lines.push(format!("{indent}{connector} {origin}"));
                lines.push(format!(
                    "{indent}{connector} {}",
                    explain_task_runtime_interactive(task)
                ));
            }
        } else {
            lines.push(format!("{indent}{}", style::nstyle("Tasks:").dim()));
            for task in &stage.tasks {
                let (label, origin) = format_planned_task_summary_parts(task);
                lines.push(format!("{indent}  - {label}"));
                lines.push(format!("{indent}    {origin}"));
                lines.push(format!(
                    "{indent}    {}",
                    explain_task_runtime_interactive(task)
                ));
            }
        }
        lines.push(String::new());
    }

    lines.push(
        style::nstyle("Scheduler Contention:")
            .cyan()
            .bold()
            .to_string(),
    );
    lines.push(format!(
        "  {}={} | {}={} | {}={}",
        style::nstyle("jobs").dim(),
        style::nstyle(contention.jobs).bold(),
        style::nstyle("interactive_stages").dim(),
        style::nstyle(contention.interactive_stage_count).bold(),
        style::nstyle("max_parallel_runtime_tasks").dim(),
        style::nstyle(contention.max_parallel_runtime_tasks).bold()
    ));
    if contention.warnings.is_empty() {
        lines.push(format!(
            "  {} {}",
            style::nstyle("warnings:").dim(),
            style::nstyle("none").dim()
        ));
    } else {
        for warning in &contention.warnings {
            lines.push(format!(
                "  {} {}",
                style::nstyle("warning:").yellow().bold(),
                style::nstyle(warning).yellow()
            ));
        }
    }
    lines.push(String::new());

    if !change_impact.changed_files.is_empty() {
        lines.push(style::nstyle("Change Impact:").cyan().bold().to_string());
        lines.push(format!(
            "  {}={} | {}={} | {}={}",
            style::nstyle("changed_files").dim(),
            style::nstyle(change_impact.changed_files.len()).bold(),
            style::nstyle("directly_matched").dim(),
            style::nstyle(change_impact.directly_matched.len()).bold(),
            style::nstyle("impacted_total").dim(),
            style::nstyle(change_impact.impacted.len()).bold()
        ));
        if change_impact.directly_matched.is_empty() {
            lines.push(format!(
                "  {} {}",
                style::nstyle("direct_matches:").dim(),
                style::nstyle("none").dim()
            ));
        } else {
            for matched in &change_impact.directly_matched {
                lines.push(format!("  direct: {}", identity_label(&matched.task)));
                lines.push(format!("    files={}", matched.matched_files.join(", ")));
                lines.push(format!(
                    "    source_patterns={}",
                    matched.matched_source_patterns.join(", ")
                ));
            }
        }
        if !change_impact.impacted.is_empty() {
            lines.push(format!(
                "  impacted_tasks={}",
                change_impact
                    .impacted
                    .iter()
                    .map(identity_label)
                    .collect_vec()
                    .join(", ")
            ));
        }
        lines.push(String::new());
    }

    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    Ok(lines.join("\n"))
}

#[derive(Debug, Serialize)]
struct PlanJsonSummary {
    stages: usize,
    tasks: usize,
    runtime_tasks: usize,
    interactive_tasks: usize,
    orchestrator_tasks: usize,
}

#[derive(Debug, Serialize)]
struct PlanJsonStage {
    index: usize,
    kind: &'static str,
    hash: String,
    tasks: Vec<PlannedTask>,
}

#[derive(Debug, Serialize)]
struct PlanJsonOutput {
    format_version: u8,
    kind: &'static str,
    plan_hash: String,
    summary: PlanJsonSummary,
    stages: Vec<PlanJsonStage>,
    diagnostics: PlanJsonDiagnostics,
}

#[derive(Debug, Serialize)]
struct PlanJsonDiagnostics {
    change_impact: ChangeImpact,
    contention: ContentionAnalysis,
}

fn format_execution_plan_json(
    plan: &ExecutionPlan,
    change_impact: &ChangeImpact,
    contention: &ContentionAnalysis,
) -> Result<String> {
    let stats = execution_plan_stats(plan);
    let stages = plan
        .stages
        .iter()
        .enumerate()
        .map(|(idx, stage)| {
            Ok(PlanJsonStage {
                index: idx + 1,
                kind: stage_kind_label(stage.kind),
                hash: execution_stage_hash(stage)?,
                tasks: stage.tasks.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let output = PlanJsonOutput {
        format_version: 1,
        kind: "static-dag",
        plan_hash: execution_plan_hash(plan)?,
        summary: PlanJsonSummary {
            stages: stats.stage_count,
            tasks: stats.task_count,
            runtime_tasks: stats.runtime_count,
            interactive_tasks: stats.interactive_count,
            orchestrator_tasks: stats.orchestrator_count,
        },
        stages,
        diagnostics: PlanJsonDiagnostics {
            change_impact: change_impact.clone(),
            contention: contention.clone(),
        },
    };
    Ok(serde_json::to_string_pretty(&output)?)
}

fn format_plan_output(
    plan: &ExecutionPlan,
    mode: PlanOutputMode,
    change_impact: &ChangeImpact,
    contention: &ContentionAnalysis,
) -> Result<String> {
    match mode {
        PlanOutputMode::Summary => format_execution_plan_summary(plan),
        PlanOutputMode::Json => format_execution_plan_json(plan, change_impact, contention),
        PlanOutputMode::Explain => render_execution_plan_explain(plan, change_impact, contention),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::task_execution_plan::{
        ExecutionPlan, ExecutionStage, ExecutionStageKind, PlannedTask,
    };
    use crate::task::task_identity::TaskIdentity;

    fn no_change_impact() -> ChangeImpact {
        ChangeImpact::default()
    }

    fn no_contention() -> ContentionAnalysis {
        ContentionAnalysis::default()
    }

    #[test]
    fn test_in_flight_guard_decrements_on_drop() {
        // MatrixRef: F10 / C12
        let in_flight = Arc::new(AtomicUsize::new(1));
        {
            let _guard = InFlightGuard::new(in_flight.clone());
            assert_eq!(in_flight.load(Ordering::SeqCst), 2);
        }
        assert_eq!(in_flight.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_in_flight_guard_handles_multiple_guards() {
        // MatrixRef: O4,F10 / C12
        let in_flight = Arc::new(AtomicUsize::new(0));
        let g1 = InFlightGuard::new(in_flight.clone());
        let g2 = InFlightGuard::new(in_flight.clone());
        assert_eq!(in_flight.load(Ordering::SeqCst), 2);
        drop(g1);
        assert_eq!(in_flight.load(Ordering::SeqCst), 1);
        drop(g2);
        assert_eq!(in_flight.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_format_execution_plan_summary_prints_stage_kinds_order_and_hash() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "build".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(1),
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::InteractiveExclusive,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "prompt".to_string(),
                            args: vec!["--force".to_string()],
                            env: vec![("A".to_string(), "1".to_string())],
                        },
                        runtime: true,
                        interactive: true,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(4),
                        },
                    }],
                },
            ],
        };

        let out = format_plan_output(
            &plan,
            PlanOutputMode::Summary,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        assert!(out.contains("Execution plan: 2 stage(s)"));
        assert!(out.contains("Config files used (1):"));
        assert!(out.contains("stage number is execution order"));
        assert!(out.contains("sha256:"));
        assert!(out.contains("1. [task] build [declared at mise.toml:1]"));
        assert!(out.contains("2. [interactive] prompt --force {A=1} [declared at mise.toml:4]"));
    }

    #[test]
    fn test_format_execution_plan_summary_renders_barrier_scopes_without_virtual_tasks() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_start__::1".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "build".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(2),
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::InteractiveExclusive,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "prompt".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: true,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(3),
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_end__::2".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
            ],
        };

        let out = format_plan_output(
            &plan,
            PlanOutputMode::Summary,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        assert!(out.contains("┌─ group: root"));
        assert!(out.contains("1. │  [task] build [declared at mise.toml:2]"));
        assert!(out.contains("2. │  [interactive] prompt [declared at mise.toml:3]"));
        assert!(out.contains("└─ group: root"));
        assert!(!out.contains("barrier-start"));
        assert!(!out.contains("barrier-end"));
    }

    #[test]
    fn test_format_execution_plan_summary_pads_stage_numbers_to_min_width_three() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::Parallel,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                        source: "mise.toml".to_string(),
                        line: Some(1),
                    },
                }],
            }],
        };

        let out = format_plan_output(
            &plan,
            PlanOutputMode::Summary,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        let stage_line = out
            .lines()
            .map(|l| console::strip_ansi_codes(l))
            .find(|l| l.contains("[task]"))
            .unwrap_or_default();
        assert!(stage_line.starts_with("  1. "));
    }

    #[test]
    fn test_format_execution_plan_summary_expands_parallel_stage_items_to_multiline_tree() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::Parallel,
                tasks: vec![
                    PlannedTask {
                        identity: TaskIdentity {
                            name: "a".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(1),
                        },
                    },
                    PlannedTask {
                        identity: TaskIdentity {
                            name: "b".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(2),
                        },
                    },
                ],
            }],
        };

        let out = format_plan_output(
            &plan,
            PlanOutputMode::Summary,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        assert!(out.contains("∥ parallel (2)"));
        assert!(out.contains("├─ [task]"));
        assert!(out.contains("└─ [task]"));
        assert!(out.contains("mise.toml:1"));
        assert!(out.contains("mise.toml:2"));
    }

    #[test]
    fn test_format_execution_plan_json_contains_hash_and_summary() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::Parallel,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                        source: "mise.toml".to_string(),
                        line: Some(1),
                    },
                }],
            }],
        };
        let out = format_plan_output(
            &plan,
            PlanOutputMode::Json,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["format_version"], 1);
        assert_eq!(value["kind"], "static-dag");
        assert!(value["plan_hash"].as_str().unwrap().starts_with("sha256:"));
        assert_eq!(value["summary"]["stages"], 1);
        assert_eq!(
            value["stages"][0]["tasks"][0]["declaration"]["source"],
            "mise.toml"
        );
        assert_eq!(value["stages"][0]["tasks"][0]["declaration"]["line"], 1);
        assert_eq!(value["diagnostics"]["contention"]["jobs"], 0);
    }

    #[test]
    fn test_format_execution_plan_explain_contains_stage_hashes_and_reasons() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::InteractiveExclusive,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "prompt".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: true,
                    declaration: Default::default(),
                }],
            }],
        };
        let out = format_plan_output(
            &plan,
            PlanOutputMode::Explain,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        assert!(out.contains("Plan: valid (static)"));
        assert!(out.contains("Hash: sha256:"));
        assert!(out.contains("Stage 1 [blocking (interactive)]"));
        assert!(
            out.contains("interactive exclusivity: this stage blocks concurrent runtime starts")
        );
        assert!(out.contains("Determinism: stable topological stages"));
        assert!(out.contains("Scheduler Contention:"));
    }

    #[test]
    fn test_plan_hash_is_stable_for_same_plan() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::Parallel,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: Default::default(),
                }],
            }],
        };
        let h1 = execution_plan_hash(&plan).unwrap();
        let h2 = execution_plan_hash(&plan).unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_format_execution_plan_explain_includes_change_impact_details() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::Parallel,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "build".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: false,
                    declaration: Default::default(),
                }],
            }],
        };
        let impact = ChangeImpact {
            changed_files: vec!["src/main.ts".to_string()],
            directly_matched: vec![crate::task::task_plan_analysis::ChangedTaskMatch {
                task: TaskIdentity {
                    name: "build".to_string(),
                    args: vec![],
                    env: vec![],
                },
                matched_files: vec!["src/main.ts".to_string()],
                matched_source_patterns: vec!["src/**/*.ts".to_string()],
            }],
            impacted: vec![TaskIdentity {
                name: "build".to_string(),
                args: vec![],
                env: vec![],
            }],
        };
        let contention = ContentionAnalysis {
            jobs: 4,
            ..Default::default()
        };
        let out = format_plan_output(&plan, PlanOutputMode::Explain, &impact, &contention).unwrap();
        assert!(out.contains("Change Impact:"));
        assert!(out.contains("direct: build"));
        assert!(out.contains("impacted_tasks=build"));
    }

    #[test]
    fn test_format_execution_plan_explain_renders_groups_without_virtual_stage_numbering() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_start__::1".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "build".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(1),
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_end__::2".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
            ],
        };

        let out = format_plan_output(
            &plan,
            PlanOutputMode::Explain,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        assert!(out.contains("Config files used (1):"));
        assert!(out.contains("┌─ group: root"));
        assert!(out.contains("└─ group: root"));
        assert!(out.contains("Stage 1 [runnable]"));
        assert!(out.contains("[task] build"));
        assert!(!out.contains("barrier-start"));
        assert!(!out.contains("barrier-end"));
    }

    #[test]
    fn test_format_execution_plan_explain_keeps_group_guides_left_aligned() {
        let plan = ExecutionPlan {
            stages: vec![
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_start__::1".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::sub::__barrier_start__::2".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "build".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: true,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "mise.toml".to_string(),
                            line: Some(1),
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::sub::__barrier_end__::3".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
                ExecutionStage {
                    kind: ExecutionStageKind::Parallel,
                    tasks: vec![PlannedTask {
                        identity: TaskIdentity {
                            name: "__mise_static::root::__barrier_end__::4".to_string(),
                            args: vec![],
                            env: vec![],
                        },
                        runtime: false,
                        interactive: false,
                        declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                            source: "<generated>".to_string(),
                            line: None,
                        },
                    }],
                },
            ],
        };
        let out = format_plan_output(
            &plan,
            PlanOutputMode::Explain,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        let clean = out
            .lines()
            .map(console::strip_ansi_codes)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(clean.contains("│  │  Stage 1 [runnable]"));
        assert!(clean.contains("│  │  Tasks:"));
        assert!(!clean.contains("  │  │  Tasks:"));
    }

    #[test]
    fn test_format_execution_plan_explain_separates_declaration_and_explains_true_flags() {
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage {
                kind: ExecutionStageKind::InteractiveExclusive,
                tasks: vec![PlannedTask {
                    identity: TaskIdentity {
                        name: "prompt".to_string(),
                        args: vec![],
                        env: vec![],
                    },
                    runtime: true,
                    interactive: true,
                    declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                        source: "mise.toml".to_string(),
                        line: Some(4),
                    },
                }],
            }],
        };
        let out = format_plan_output(
            &plan,
            PlanOutputMode::Explain,
            &no_change_impact(),
            &no_contention(),
        )
        .unwrap();
        let clean = out
            .lines()
            .map(console::strip_ansi_codes)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(clean.contains("  - [interactive] prompt"));
        assert!(clean.contains("    [declared at mise.toml:4]"));
        assert!(clean.contains("runtime=true (spawns a user process)"));
        assert!(clean.contains("interactive=true (global exclusive barrier for runtime tasks)"));
    }

    #[test]
    fn test_task_validation_helpers_use_planned_context_when_available() {
        let task = Task {
            name: "build".to_string(),
            config_source: "/tmp/fallback.toml".into(),
            ..Default::default()
        };
        let plan = ExecutionPlan {
            stages: vec![ExecutionStage::parallel(vec![PlannedTask {
                identity: TaskIdentity::from_task(&task),
                runtime: true,
                interactive: false,
                declaration: crate::task::task_execution_plan::TaskDeclarationRef {
                    source: "/tmp/mise.toml".to_string(),
                    line: Some(12),
                },
            }])],
        };
        let context = PlanContextIndex::from_plan(&plan, None);
        let declaration = task_validation_declaration(&task, Some(&context));
        let suffix = task_validation_stage_suffix(&task, Some(&context));

        assert_eq!(declaration, "/tmp/mise.toml:12");
        assert_eq!(suffix, " [stage 1/1, kind=parallel]");
    }

    #[test]
    fn test_task_validation_helpers_fallback_without_plan_context() {
        let task = Task {
            name: "build".to_string(),
            config_source: "/tmp/fallback.toml".into(),
            ..Default::default()
        };
        let declaration = task_validation_declaration(&task, None);
        let suffix = task_validation_stage_suffix(&task, None);

        assert_eq!(declaration, "/tmp/fallback.toml");
        assert_eq!(suffix, "");
    }
}
