use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::SystemTime;

use super::args::ToolArg;
use crate::cli::Cli;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::errors::Error;
use crate::file::display_path;
use crate::http::HTTP;
use crate::task::{Deps, EitherIntOrBool, GetMatchingExt, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::{ctrlc, prompt, style, time};
use crate::{dirs, env, exit, file, ui};
use clap::{CommandFactory, ValueHint};
use crossbeam_channel::{select, unbounded};
use demand::{DemandOption, Select};
use duct::IntoExecutablePath;
use either::Either;
use eyre::{bail, ensure, eyre, Result};
use glob::glob;
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
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "r", verbatim_doc_comment, disable_help_flag = true, after_long_help = AFTER_LONG_HELP
)]
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
    #[clap(allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,

    /// Change to this directory before executing the command
    #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    pub cd: Option<PathBuf>,

    /// Don't actually run the tasks(s), just print them in order of execution
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Force the tasks to run even if outputs are up to date
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Print stdout/stderr by line, prefixed with the tasks's label
    /// Defaults to true if --jobs > 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    pub prefix: bool,

    /// Print directly to stdout/stderr instead of by line
    /// Defaults to true if --jobs == 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "prefix")]
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
    #[clap(long, short, verbatim_doc_comment)]
    pub quiet: bool,

    #[clap(skip)]
    pub is_linear: bool,

    #[clap(skip)]
    pub failed_tasks: Mutex<Vec<(Task, i32)>>,

    #[clap(skip)]
    pub output: TaskOutput,

    #[clap(skip)]
    pub tmpdir: PathBuf,
}

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

        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&Config::get())?;

        ts.install_missing_versions(&InstallOptions {
            missing_args_only: !SETTINGS.task_run_auto_install,
            ..Default::default()
        })?;
        let mut env = ts.env_with_path(&Config::get())?;
        if let Some(cwd) = &*dirs::CWD {
            env.insert("MISE_ORIGINAL_CWD".into(), cwd.display().to_string());
        }
        if let Some(root) = Config::get().project_root.clone() {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
            env.insert("root".into(), root.display().to_string());
        }

        self.fetch_tasks(&mut tasks)?;
        let tasks = Deps::new(tasks)?;
        for task in tasks.all() {
            self.validate_task(task)?;
        }

        let num_tasks = tasks.all().count();
        self.is_linear = tasks.is_linear();
        if let Some(task) = tasks.all().next() {
            self.output = self.output(task)?;
            if let TaskOutput::Quiet = self.output {
                self.quiet = true;
            }
        }

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
                    if !self.is_stopping() {
                        trace!("running task: {task}");
                        if let Err(err) = self.run_task(&env, &task) {
                            let status = Error::get_exit_status(&err);
                            if !self.is_stopping() && status.is_none() {
                                // only show this if it's the first failure, or we haven't killed all the remaining tasks
                                // otherwise we'll get unhelpful error messages about being killed by mise which we expect
                                let prefix = task.estyled_prefix();
                                eprintln!("{prefix} {} {err:?}", style::ered("ERROR"));
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
                        #[cfg(unix)]
                        CmdLineRunner::kill_all(SIGTERM); // start killing other running tasks
                        #[cfg(windows)]
                        CmdLineRunner::kill_all();
                    }
                }
            }
        });

        if let Some((task, status)) = self.failed_tasks.lock().unwrap().first() {
            let prefix = task.estyled_prefix();
            eprintln!("{prefix} {} task failed", style::ered("ERROR"));
            exit(*status);
        }

        if self.timings() && num_tasks > 1 {
            let msg = format!("Finished in {}", time::format_duration(timer.elapsed()));
            eprintln!("{}", style::edim(msg));
        };

        time!("paralellize_tasks done");

        Ok(())
    }

    fn run_task(&self, env: &BTreeMap<String, String>, task: &Task) -> Result<()> {
        let prefix = task.estyled_prefix();
        if SETTINGS.task_skip.contains(&task.name) {
            if !self.quiet {
                eprintln!("{prefix} skipping task");
            }
            return Ok(());
        }
        if !self.force && self.sources_are_fresh(task)? {
            if !self.quiet {
                eprintln!("{prefix} sources up-to-date, skipping");
            }
            return Ok(());
        }

        let mut env = env.clone();
        env.insert("MISE_TASK_NAME".into(), task.name.clone());
        let task_file = task.file.as_ref().unwrap_or(&task.config_source);
        env.insert("MISE_TASK_FILE".into(), task_file.display().to_string());
        if let Some(dir) = task_file.parent() {
            env.insert("MISE_TASK_DIR".into(), dir.display().to_string());
        }
        if let Some(config_root) = &task.config_root {
            env.insert("MISE_CONFIG_ROOT".into(), config_root.display().to_string());
        }
        let string_env: Vec<(String, String)> = task
            .env
            .iter()
            .filter_map(|(k, v)| match &v.0 {
                Either::Left(v) => Some((k.to_string(), v.to_string())),
                Either::Right(EitherIntOrBool(Either::Left(v))) => {
                    Some((k.to_string(), v.to_string()))
                }
                _ => None,
            })
            .collect_vec();
        let rm_env = task
            .env
            .iter()
            .filter(|(_, v)| v.0 == Either::Right(EitherIntOrBool(Either::Right(false))))
            .map(|(k, _)| k)
            .collect::<HashSet<_>>();
        let env: BTreeMap<String, String> = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .chain(string_env)
            .filter(|(k, _)| !rm_env.contains(k))
            .collect();

        let timer = std::time::Instant::now();

        if let Some(file) = &task.file {
            self.exec_file(file, task, &env, &prefix)?;
        } else {
            for (script, args) in task.render_run_scripts_with_args(self.cd.clone(), &task.args)? {
                self.exec_script(&script, &args, task, &env, &prefix)?;
            }
        }

        if self.timings() && (task.file.as_ref().is_some() || !task.run().is_empty()) {
            eprintln!(
                "{prefix} finished in {}",
                time::format_duration(timer.elapsed())
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
        let script = script.trim_start();
        let cmd = trunc(
            &style::ebold(format!("$ {script} {args}", args = args.join(" ")))
                .bright()
                .to_string(),
        );
        if !self.quiet {
            eprintln!("{prefix} {cmd}");
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

    fn exec_file(
        &self,
        file: &Path,
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let mut env = env.clone();
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();
        let (spec, _) = task.parse_usage_spec(self.cd.clone())?;
        if !spec.cmd.args.is_empty() || !spec.cmd.flags.is_empty() {
            let args = once(command.clone()).chain(args.clone()).collect_vec();
            let po = usage::parse(&spec, &args).map_err(|err| eyre!(err))?;
            for (k, v) in po.as_env() {
                env.insert(k, v);
            }
        }

        let cmd = format!("{} {}", display_path(file), args.join(" "));
        let cmd = trunc(&style::ebold(format!("$ {cmd}")).bright().to_string());
        if !self.quiet {
            eprintln!("{prefix} {cmd}");
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
        let program = program.to_executable();
        let mut cmd = CmdLineRunner::new(program.clone())
            .args(args)
            .envs(env)
            .raw(self.raw(task));
        cmd.with_pass_signals();
        match self.output {
            TaskOutput::Prefix => cmd = cmd.prefix(format!("{prefix} ")),
            TaskOutput::Quiet | TaskOutput::Interleave => {
                cmd = cmd
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
            }
        }
        let dir = self.cwd(task)?;
        if !dir.exists() {
            eprintln!(
                "{prefix} {} task directory does not exist: {}",
                style::eyellow("WARN"),
                display_path(&dir)
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

    fn output(&self, task: &Task) -> Result<TaskOutput> {
        if self.quiet {
            Ok(TaskOutput::Quiet)
        } else if self.prefix {
            Ok(TaskOutput::Prefix)
        } else if self.interleave {
            Ok(TaskOutput::Interleave)
        } else if let Some(output) = &SETTINGS.task_output {
            Ok(output.parse()?)
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
            Ok(TaskOutput::Interleave)
        } else {
            Ok(TaskOutput::Prefix)
        }
    }

    fn raw(&self, task: &Task) -> bool {
        self.raw || task.raw || SETTINGS.raw
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
        if task.sources.is_empty() && task.outputs.is_empty() {
            return Ok(false);
        }
        let run = || -> Result<bool> {
            let sources = self.get_last_modified(&self.cwd(task)?, &task.sources)?;
            let outputs = self.get_last_modified(&self.cwd(task)?, &task.outputs)?;
            trace!("sources: {sources:?}, outputs: {outputs:?}");
            match (sources, outputs) {
                (Some(sources), Some(outputs)) => Ok(sources < outputs),
                _ => Ok(false),
            }
        };
        Ok(run().unwrap_or_else(|err| {
            warn!("sources_are_fresh: {err}");
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
                .unwrap_or_else(|| env::current_dir().unwrap()))
        }
    }

    fn save_checksum(&self, task: &Task) -> Result<()> {
        if task.sources.is_empty() {
            return Ok(());
        }
        // TODO
        Ok(())
    }

    fn timings(&self) -> bool {
        !self.quiet
            && !self.no_timings
            && SETTINGS
                .task_timings
                .unwrap_or(self.output == TaskOutput::Prefix)
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

fn last_modified_path(
    root: impl AsRef<std::ffi::OsStr>,
    paths: &[&String],
) -> Result<Option<SystemTime>> {
    let files = paths
        .iter()
        .map(|p| {
            let base = Path::new(p);

            if base.is_relative() {
                base.to_path_buf()
            } else {
                Path::new(&root).join(base)
            }
        })
        .filter(|p| p.is_file());

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
        .map(|p| p.metadata().map_err(|err| eyre!(err)))
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

#[derive(Debug, Default, PartialEq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum TaskOutput {
    #[default]
    Prefix,
    Interleave,
    Quiet,
}

fn trunc(msg: &str) -> String {
    let msg = msg.lines().next().unwrap_or_default();
    console::truncate_str(msg, *env::TERM_WIDTH, "…").to_string()
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
        .description("Select a tasks to run")
        .filterable(true);
    for t in tasks.values().filter(|t| !t.hide) {
        s = s.option(DemandOption::new(&t.name).description(&t.description));
    }
    ctrlc::show_cursor_after_ctrl_c();
    let name = s.run()?;
    match tasks.get(name) {
        Some(task) => Ok((*task).clone()),
        None => bail!("no tasks {} found", style::ered(name)),
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
                if t != "default" || !prompt {
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
