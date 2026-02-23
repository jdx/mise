use crate::errors::Error;
use std::iter::once;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::args::ToolArg;
use crate::cli::{Cli, unescape_task_args};
use crate::config::{Config, Settings};
use crate::duration;
use crate::env;
use crate::file::display_path;
use crate::prepare::{PrepareEngine, PrepareOptions};
use crate::task::has_any_args_defined;
use crate::task::task_helpers::task_needs_permit;
use crate::task::task_list::{get_task_lists, resolve_depends};
use crate::task::task_output::TaskOutput;
use crate::task::task_output_handler::OutputHandler;
use crate::task::{Deps, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::{ctrlc, info, style};
use clap::{CommandFactory, ValueHint};
use eyre::{Result, bail, eyre};
use itertools::Itertools;
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

        // Build and install toolset so tools like npm are available for prepare
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

        // Run auto-enabled prepare steps (unless --no-prepare)
        // This runs after task resolution so we can discover prepare providers
        // from monorepo subdirectory configs referenced by the resolved tasks.
        if !self.no_prepare {
            let env = ts.env_with_path(&config).await?;
            let mut engine = PrepareEngine::new(&config)?;

            // Collect subdirectory config files from resolved tasks
            let subdir_configs: Vec<_> = task_list
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

        // Step 1: Prepare tasks (resolve dependencies, fetch, validate)
        let tasks = self.prepare_tasks(&config, tasks).await?;
        let num_tasks = tasks.all().count();

        // Step 2: Setup output handler and validate tasks
        self.setup_output_and_validate(&tasks)?;
        self.output = Some(self.output(None));

        // Step 3: Install tools needed by tasks
        self.install_task_tools(&mut config, &tasks).await?;

        // Step 4: Create TaskExecutor after tool installation
        self.setup_executor()?;

        let timer = std::time::Instant::now();
        let this = Arc::new(self);
        let config = config.clone();

        // Step 4: Initialize scheduler and run tasks
        let mut scheduler = crate::task::task_scheduler::Scheduler::new(this.jobs());
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

        // Step 5: Display results and handle failures
        let results_display = crate::task::task_results_display::TaskResultsDisplay::new(
            this.output_handler.clone().unwrap(),
            this.executor.as_ref().unwrap().failed_tasks.clone(),
            this.continue_on_error,
            this.timings(),
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
        if this.is_stopping() && !this.continue_on_error {
            let mut deps = deps_for_remove.lock().await;
            if !deps.is_runnable_post_dep(&task) {
                trace!(
                    "aborting spawn before start (not continue-on-error): {} {}",
                    task.name,
                    task.args.join(" ")
                );
                deps.remove(&task);
                return Ok(());
            }
            drop(deps);
        }
        let needs_permit = task_needs_permit(&task);
        let permit_opt = if needs_permit {
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
            if this.is_stopping() && !this.continue_on_error {
                let mut deps = deps_for_remove.lock().await;
                if !deps.is_runnable_post_dep(&task) {
                    trace!(
                        "aborting spawn after failure (not continue-on-error): {} {}",
                        task.name,
                        task.args.join(" ")
                    );
                    // Remove from deps so the scheduler can drain and not hang
                    deps.remove(&task);
                    return Ok(());
                }
                drop(deps);
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
            if let Some(oh) = &this.output_handler
                && oh.output(None) == TaskOutput::KeepOrder
            {
                oh.keep_order_state.lock().unwrap().on_task_finished(&task);
            }
            deps_for_remove.lock().await.remove(&task);
            trace!("deps removed: {} {}", task.name, task.args.join(" "));
            in_flight_c.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            result
        });

        Ok(())
    }

    // ============================================================================
    // High-level workflow methods
    // ============================================================================

    /// Prepare tasks: resolve dependencies, fetch remote tasks, create dependency graph
    async fn prepare_tasks(&mut self, config: &Arc<Config>, tasks: Vec<Task>) -> Result<Deps> {
        let mut tasks = resolve_depends(config, tasks).await?;
        let fetcher = crate::task::task_fetcher::TaskFetcher::new(self.no_cache);
        fetcher.fetch_tasks(&mut tasks).await?;
        let tasks = Deps::new(config, tasks).await?;
        self.is_linear = tasks.is_linear();
        Ok(tasks)
    }

    /// Initialize output handler and validate tasks
    fn setup_output_and_validate(&mut self, tasks: &Deps) -> Result<()> {
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
            self.validate_task(task)?;
            self.output_handler.as_mut().unwrap().init_task(task);
        }

        Ok(())
    }

    /// Create TaskExecutor after tool installation to ensure caches are populated
    fn setup_executor(&mut self) -> Result<()> {
        let executor_config = crate::task::task_executor::TaskExecutorConfig {
            force: self.force,
            cd: self.cd.clone(),
            shell: self.shell.clone(),
            tool: self.tool.clone(),
            timings: self.timings,
            continue_on_error: self.continue_on_error,
            dry_run: self.dry_run,
            skip_deps: self.skip_deps,
        };
        self.executor = Some(crate::task::task_executor::TaskExecutor::new(
            self.context_builder.clone(),
            self.output_handler.clone().unwrap(),
            executor_config,
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

    async fn run_task_sched(
        &self,
        task: &Task,
        config: &Arc<Config>,
        sched_tx: Arc<tokio::sync::mpsc::UnboundedSender<(Task, Arc<Mutex<Deps>>)>>,
    ) -> Result<()> {
        self.executor
            .as_ref()
            .expect("executor must be initialized before running tasks")
            .run_task_sched(task, config, sched_tx)
            .await
    }

    fn add_failed_task(&self, task: Task, status: Option<i32>) {
        if let Some(executor) = &self.executor {
            executor.add_failed_task(task, status);
        }
    }

    fn validate_task(&self, task: &Task) -> Result<()> {
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
"#
);
