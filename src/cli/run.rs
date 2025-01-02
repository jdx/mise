use std::collections::BTreeMap;
use std::io::Write;
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::{panic, thread};

use super::args::ToolArg;
use crate::cli::Cli;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::env_diff::EnvMap;
use crate::errors::Error;
use crate::file::display_path;
use crate::http::HTTP;
use crate::task::{Deps, GetMatchingExt, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::{ctrlc, prompt, style, time};
use crate::{dirs, env, exit, file, ui};
use clap::{CommandFactory, ValueHint};
use console::Term;
use crossbeam_channel::{select, unbounded};
use demand::{DemandOption, Select};
use duct::IntoExecutablePath;
use eyre::{bail, ensure, eyre, Result};
use glob::glob;
use indexmap::IndexMap;
use itertools::Itertools;
#[cfg(unix)]
use nix::sys::signal::SIGTERM;
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
    /// Configure with `raw` config or `MISE_RAW` env var
    #[clap(long, short, verbatim_doc_comment)]
    pub raw: bool,

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

    /// Don't show any output except for errors
    #[clap(long, short = 'S', verbatim_doc_comment, env = "MISE_SILENT")]
    pub silent: bool,

    #[clap(skip)]
    pub is_linear: bool,

    #[clap(skip)]
    pub failed_tasks: Mutex<Vec<(Task, i32)>>,

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
    pub keep_order_output: Mutex<IndexMap<Task, KeepOrderOutputs>>,

    #[clap(skip)]
    pub task_prs: IndexMap<Task, Arc<Box<dyn SingleReport>>>,

    #[clap(skip)]
    pub timed_outputs: Arc<Mutex<IndexMap<String, (SystemTime, String)>>>,
}

type KeepOrderOutputs = (Vec<(String, String)>, Vec<(String, String)>);

impl Run {
    pub fn run(mut self) -> Result<()> {
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
        let task_list = get_task_lists(&args, true)?;
        time!("run get_task_lists");
        self.parallelize_tasks(task_list)?;
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

    fn parallelize_tasks(mut self, mut tasks: Vec<Task>) -> Result<()> {
        time!("paralellize_tasks start");

        ctrlc::exit_on_ctrl_c(false);

        if self.output(None) == TaskOutput::Timed {
            let timed_outputs = self.timed_outputs.clone();
            thread::spawn(move || loop {
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
                thread::sleep(Duration::from_millis(100));
            });
        }

        self.fetch_tasks(&mut tasks)?;
        let tasks = Deps::new(tasks)?;
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

        let mut all_tools = self.tool.clone();
        for t in tasks.all() {
            for (k, v) in &t.tools {
                all_tools.push(format!("{}@{}", k, v).parse()?);
            }
        }
        let mut ts = ToolsetBuilder::new()
            .with_args(&all_tools)
            .build(&Config::get())?;

        ts.install_missing_versions(&InstallOptions {
            missing_args_only: !SETTINGS.task_run_auto_install,
            ..Default::default()
        })?;

        let tasks = Mutex::new(tasks);
        let timer = std::time::Instant::now();

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.jobs() + 1)
            .build()?;
        pool.scope(|s| {
            let (tx_err, rx_err) = unbounded();
            let run = |task: &Task| {
                let t = task.clone();
                let tx_err = tx_err.clone();
                s.spawn(|_| {
                    let task = t;
                    let tx_err = tx_err;
                    let prefix = task.estyled_prefix();
                    panic::set_hook(Box::new(move |info| {
                        prefix_eprintln!(prefix, "panic in task: {info}");
                        exit(1);
                    }));
                    if !self.is_stopping() {
                        trace!("running task: {task}");
                        if let Err(err) = self.run_task(&task) {
                            let status = Error::get_exit_status(&err);
                            if !self.is_stopping() && status.is_none() {
                                // only show this if it's the first failure, or we haven't killed all the remaining tasks
                                // otherwise we'll get unhelpful error messages about being killed by mise which we expect
                                let prefix = task.estyled_prefix();
                                self.eprint(
                                    &task,
                                    &prefix,
                                    &format!("{} {err:?}", style::ered("ERROR")),
                                );
                            }
                            let _ = tx_err.send((task.clone(), status));
                        }
                    }
                    tasks.lock().unwrap().remove(&task);
                });
            };
            let rx = tasks.lock().unwrap().subscribe();
            while !self.is_stopping() && !tasks.lock().unwrap().is_empty() {
                select! {
                    recv(rx) -> task => { // receive a task from Deps
                        if let Some(task) = task.unwrap() {
                            run(&task);
                        }
                    }
                    recv(rx_err) -> task => { // a task errored
                        let (task, status) = task.unwrap();
                        self.add_failed_task(task, status);
                        if !self.continue_on_error {
                            #[cfg(unix)]
                            CmdLineRunner::kill_all(SIGTERM); // start killing other running tasks
                            #[cfg(windows)]
                            CmdLineRunner::kill_all();
                        }
                    }
                }
            }
        });

        if let Some((task, status)) = self.failed_tasks.lock().unwrap().first() {
            let prefix = task.estyled_prefix();
            self.eprint(
                task,
                &prefix,
                &format!("{} task failed", style::ered("ERROR")),
            );
            exit(*status);
        }

        if self.timings() && num_tasks > 1 && *env::MISE_TASK_LEVEL == 0 {
            let msg = format!("Finished in {}", time::format_duration(timer.elapsed()));
            eprintln!("{}", style::edim(msg));
        };

        if self.output(None) == TaskOutput::KeepOrder {
            // TODO: display these as tasks complete in order somehow rather than waiting until everything is done
            let output = self.keep_order_output.lock().unwrap();
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
        time!("parallelize_tasks done");

        Ok(())
    }

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        match self.output(Some(task)) {
            TaskOutput::Replacing => {
                let pr = self.task_prs.get(task).unwrap().clone();
                pr.set_message(format!("{} {}", prefix, line));
            }
            _ => {
                prefix_eprintln!(prefix, "{line}");
            }
        }
    }

    fn run_task(&self, task: &Task) -> Result<()> {
        let prefix = task.estyled_prefix();
        if SETTINGS.task_skip.contains(&task.name) {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "skipping task");
            }
            return Ok(());
        }
        if !self.force && self.sources_are_fresh(task)? {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            return Ok(());
        }

        let config = Config::get();
        let mut tools = self.tool.clone();
        for (k, v) in &task.tools {
            tools.push(format!("{}@{}", k, v).parse()?);
        }
        let ts = ToolsetBuilder::new().with_args(&tools).build(&config)?;
        let mut env = task.render_env(&ts)?;
        let output = self.output(Some(task));
        env.insert("MISE_TASK_OUTPUT".into(), output.to_string());
        if output == TaskOutput::Prefix {
            env.insert(
                "MISE_TASK_LEVEL".into(),
                (*env::MISE_TASK_LEVEL + 1).to_string(),
            );
        }
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
            self.exec_file(file, task, &env, &prefix)?;
        } else {
            for (script, args) in
                task.render_run_scripts_with_args(self.cd.clone(), &task.args, &env)?
            {
                self.exec_script(&script, &args, task, &env, &prefix)?;
            }
        }

        if self.timings() && (task.file.as_ref().is_some() || !task.run().is_empty()) {
            self.eprint(
                task,
                &prefix,
                &format!("finished in {}", time::format_duration(timer.elapsed())),
            );
        }

        self.save_checksum(task)?;

        Ok(())
    }

    fn exec_script(
        &self,
        script: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let config = Config::get();
        let script = script.trim_start();
        let cmd = style::ebold(format!("$ {script} {args}", args = args.join(" ")))
            .bright()
            .to_string();
        if !self.quiet(Some(task)) {
            let msg = trunc(prefix, config.redact(cmd).trim());
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
            self.exec(&file, args, task, env, prefix)
        } else {
            let (program, args) = self.get_cmd_program_and_args(script, task, args)?;
            self.exec_program(&program, &args, task, env, prefix)
        }
    }

    fn get_file_program_and_args(
        &self,
        file: &Path,
        task: &Task,
        args: &[String],
    ) -> Result<(String, Vec<String>)> {
        let display = file.display().to_string();
        if file::is_executable(file) && !SETTINGS.use_file_shell_for_executable_tasks {
            if cfg!(windows) && file.extension().is_some_and(|e| e == "ps1") {
                let args = vec!["-File".to_string(), display]
                    .into_iter()
                    .chain(args.iter().cloned())
                    .collect_vec();
                return Ok(("pwsh".to_string(), args));
            }
            return Ok((display, args.to_vec()));
        }
        let shell = task.shell().unwrap_or(SETTINGS.default_file_shell()?);
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
            SETTINGS.default_inline_shell()
        }
    }

    fn exec_file(&self, file: &Path, task: &Task, env: &EnvMap, prefix: &str) -> Result<()> {
        let config = Config::get();
        let mut env = env.clone();
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();
        let (spec, _) = task.parse_usage_spec(self.cd.clone(), &env)?;
        if !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty() {
            let args = once(command.clone()).chain(args.clone()).collect_vec();
            let po = usage::parse(&spec, &args).map_err(|err| eyre!(err))?;
            for (k, v) in po.as_env() {
                env.insert(k, v);
            }
        }

        if !self.quiet(Some(task)) {
            let cmd = format!("{} {}", display_path(file), args.join(" "))
                .trim()
                .to_string();
            let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
            let cmd = trunc(prefix, config.redact(cmd).trim());
            self.eprint(task, prefix, &cmd);
        }

        self.exec(file, &args, task, &env, prefix)
    }

    fn exec(
        &self,
        file: &Path,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let (program, args) = self.get_file_program_and_args(file, task, args)?;
        self.exec_program(&program, &args, task, env, prefix)
    }

    fn exec_program(
        &self,
        program: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let config = Config::get();
        let program = program.to_executable();
        let redactions = config.redactions();
        let mut cmd = CmdLineRunner::new(program.clone())
            .args(args)
            .envs(env)
            .redact(redactions.deref().clone())
            .raw(self.raw(Some(task)));
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
                    self.keep_order_output
                        .lock()
                        .unwrap()
                        .get_mut(task)
                        .unwrap()
                        .0
                        .push((prefix.to_string(), line));
                });
                cmd = cmd.with_on_stderr(|line| {
                    self.keep_order_output
                        .lock()
                        .unwrap()
                        .get_mut(task)
                        .unwrap()
                        .1
                        .push((prefix.to_string(), line));
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
                if redactions.is_empty() {
                    cmd = cmd
                        .stdin(Stdio::inherit())
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                }
            }
        }
        let dir = self.cwd(task)?;
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
        } else if let Some(output) = SETTINGS.task_output {
            output
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
        }
    }

    fn silent(&self, task: Option<&Task>) -> bool {
        self.silent
            || SETTINGS.silent
            || self.output.is_some_and(|o| o.is_silent())
            || task.is_some_and(|t| t.silent)
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.quiet
            || SETTINGS.quiet
            || self.output.is_some_and(|o| o.is_quiet())
            || task.is_some_and(|t| t.quiet)
            || self.silent(task)
    }

    fn raw(&self, task: Option<&Task>) -> bool {
        self.raw || SETTINGS.raw || task.is_some_and(|t| t.raw)
    }

    fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            self.jobs.unwrap_or(SETTINGS.jobs)
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

    fn sources_are_fresh(&self, task: &Task) -> Result<bool> {
        let outputs = task.outputs.paths(task);
        if task.sources.is_empty() && outputs.is_empty() {
            return Ok(false);
        }
        let run = || -> Result<bool> {
            let mut sources = task.sources.clone();
            sources.push(task.config_source.to_string_lossy().to_string());
            let sources = self.get_last_modified(&self.cwd(task)?, &sources)?;
            let outputs = self.get_last_modified(&self.cwd(task)?, &outputs)?;
            trace!("sources: {sources:?}, outputs: {outputs:?}");
            match (sources, outputs) {
                (Some(sources), Some(outputs)) => Ok(sources < outputs),
                _ => Ok(false),
            }
        };
        Ok(run().unwrap_or_else(|err| {
            warn!("sources_are_fresh: {err:?}");
            false
        }))
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

    fn cwd(&self, task: &Task) -> Result<PathBuf> {
        if let Some(d) = &self.cd {
            Ok(d.clone())
        } else if let Some(d) = task.dir()? {
            Ok(d)
        } else {
            Ok(Config::get()
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
        !self.quiet(None)
            && !self.no_timings
            && SETTINGS
                .task_timings
                .unwrap_or(self.output == Some(TaskOutput::Prefix))
    }

    fn fetch_tasks(&self, tasks: &mut Vec<Task>) -> Result<()> {
        let http_re = regex!("https?://");
        for t in tasks {
            if let Some(file) = t.file.clone() {
                let source = file.to_string_lossy().to_string();
                if http_re.is_match(&source) {
                    let filename = file.file_name().unwrap().to_string_lossy().to_string();
                    let tmp_path = self.tmpdir.join(&filename);
                    HTTP.download_file(&source, &tmp_path, None)?;
                    file::make_executable(&tmp_path)?;
                    t.file = Some(tmp_path);
                }
            }
        }
        Ok(())
    }
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
    console::truncate_str(msg, *env::TERM_WIDTH - prefix_len - 1, "…").to_string()
}

fn err_no_task(name: &str) -> Result<()> {
    if Config::get().tasks().is_ok_and(|t| t.is_empty()) {
        bail!(
            "no tasks defined in {}. Are you in a project directory?",
            display_path(dirs::CWD.clone().unwrap_or_default())
        );
    }
    if let Some(cwd) = &*dirs::CWD {
        let includes = Config::get().task_includes_for_dir(cwd);
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

fn prompt_for_task() -> Result<Task> {
    let config = Config::get();
    let tasks = config.tasks()?;
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
                .label(&t.display_name())
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

pub fn get_task_lists(args: &[String], prompt: bool) -> Result<Vec<Task>> {
    args.iter()
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
        .map(|(t, args)| {
            // can be any of the following:
            // - ./path/to/script
            // - ~/path/to/script
            // - /path/to/script
            // - ../path/to/script
            // - C:\path\to\script
            // - .\path\to\script
            if regex!(r#"^((\.*|~)(/|\\)|\w:\\)"#).is_match(&t) {
                let path = PathBuf::from(&t);
                if path.exists() {
                    let config_root = Config::get()
                        .project_root
                        .clone()
                        .or_else(|| dirs::CWD.clone())
                        .unwrap_or_default();
                    let task = Task::from_path(&path, &PathBuf::new(), &config_root)?;
                    return Ok(vec![task.with_args(args)]);
                }
            }
            let config = Config::get();
            let tasks = config
                .tasks_with_aliases()?
                .get_matching(&t)?
                .into_iter()
                .cloned()
                .collect_vec();
            if tasks.is_empty() {
                if t != "default" || !prompt || !console::user_attended_stderr() {
                    err_no_task(&t)?;
                }

                Ok(vec![prompt_for_task()?])
            } else {
                Ok(tasks
                    .into_iter()
                    .map(|t| t.clone().with_args(args.to_vec()))
                    .collect())
            }
        })
        .flatten_ok()
        .collect()
}
