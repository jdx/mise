use std::collections::BTreeMap;
use std::io::Write;
use std::iter::once;
use std::os::unix::prelude::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{exit, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use clap::ValueHint;
use console::{style, Color};
use demand::{DemandOption, Select};
use duct::IntoExecutablePath;
use eyre::{OptionExt, Result};
use globwalk::GlobWalkerBuilder;
use itertools::Itertools;
use once_cell::sync::Lazy;

use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::errors::Error;
use crate::errors::Error::ScriptFailed;
use crate::file::display_path;
use crate::task::{Deps, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::style;
use crate::{env, file, ui};

use super::args::ToolArg;

/// [experimental] Run a task
///
/// This command will run a task, or multiple tasks in parallel.
/// Tasks may have dependencies on other tasks or on source files.
/// If source is configured on a task, it will only run if the source
/// files have changed.
///
/// Tasks can be defined in .mise.toml or as standalone scripts.
/// In .mise.toml, tasks take this form:
///
///     [tasks.build]
///     run = "npm run build"
///     sources = ["src/**/*.ts"]
///     outputs = ["dist/**/*.js"]
///
/// Alternatively, tasks can be defined as standalone scripts.
/// These must be located in the `.mise/tasks` directory.
/// The name of the script will be the name of the task.
///
///     $ cat .mise/tasks/build<<EOF
///     #!/usr/bin/env bash
///     npm run build
///     EOF
///     $ mise run build
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "r", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    /// Task to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2
    #[clap(verbatim_doc_comment, default_value = "default")]
    pub task: String,

    /// Arguments to pass to the task. Use ":::" to separate tasks.
    #[clap()]
    pub args: Vec<String>,

    /// Change to this directory before executing the command
    #[clap(short = 'C', long, value_hint = ValueHint::DirPath, long)]
    pub cd: Option<PathBuf>,

    /// Don't actually run the task(s), just print them in order of execution
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Force the task to run even if outputs are up to date
    #[clap(long, short, verbatim_doc_comment)]
    pub force: bool,

    /// Print stdout/stderr by line, prefixed with the task's label
    /// Defaults to true if --jobs > 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    pub prefix: bool,

    /// Print directly to stdout/stderr instead of by line
    /// Defaults to true if --jobs == 1
    /// Configure with `task_output` config or `MISE_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "prefix")]
    pub interleave: bool,

    /// Tool(s) to also add
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

    #[clap(skip)]
    pub is_linear: bool,
}

impl Run {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental()?;
        let task_list = self.get_task_lists(&config)?;
        self.parallelize_tasks(&config, task_list)
    }

    fn get_task_lists(&self, config: &Config) -> Result<Vec<Task>> {
        once(&self.task)
            .chain(self.args.iter())
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
            .map(|(t, args)| match config.tasks_with_aliases().get(&t) {
                Some(task) => Ok(task.clone().with_args(args.to_vec())),
                None => self.prompt_for_task(config, &t),
            })
            .collect()
    }

    fn parallelize_tasks(mut self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;

        ts.install_arg_versions(config, &InstallOptions::new())?;
        ts.notify_if_versions_missing();
        let mut env = ts.env_with_path(config);
        if let Some(root) = &config.project_root {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
        }
        if console::colors_enabled() {
            env.insert("CLICOLOR_FORCE".into(), "1".into());
            env.insert("FORCE_COLOR".into(), "1".into());
        }

        let tasks = Mutex::new(Deps::new(config, tasks)?);
        self.is_linear = tasks.lock().unwrap().is_linear();

        for task in tasks.lock().unwrap().all() {
            self.validate_task(task)?;
        }

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.jobs() + 1)
            .build()?;
        pool.scope(|s| {
            let run = |task: &Task| {
                let t = task.clone();
                s.spawn(|_| {
                    let task = t;
                    trace!("running task: {task}");
                    if let Err(err) = self.run_task(config, &env, &task) {
                        error!("{err}");
                        exit(1);
                    }
                    let mut tasks = tasks.lock().unwrap();
                    tasks.remove(&task);
                });
            };
            let rx = tasks.lock().unwrap().subscribe();
            while let Some(task) = rx.recv().unwrap() {
                run(&task);
            }
        });
        Ok(())
    }

    fn run_task(&self, config: &Config, env: &BTreeMap<String, String>, task: &Task) -> Result<()> {
        let prefix = style::estyle(task.prefix()).fg(get_color()).to_string();
        if !self.force && self.sources_are_fresh(config, task) {
            info_unprefix_trunc!("{prefix} sources up-to-date, skipping");
            return Ok(());
        }

        let env: BTreeMap<String, String> = env
            .iter()
            .chain(task.env.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if let Some(file) = &task.file {
            self.exec_file(file, task, &env, &prefix)?;
        } else {
            for (i, cmd) in task.run.iter().enumerate() {
                let args = match i == task.run.len() - 1 {
                    true => task.args.iter().cloned().collect_vec(),
                    false => vec![],
                };
                self.exec_script(cmd, &args, task, &env, &prefix)?;
            }
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
        let script = format!("{} {}", script, shell_words::join(args));
        let cmd = style::ebold(format!("$ {script}")).bright().to_string();
        info_unprefix_trunc!("{prefix} {cmd}");

        if env::var("MISE_TASK_SCRIPT_FILE").is_ok() {
            let mut tmp = tempfile::NamedTempFile::new()?;
            let args = once(tmp.path().display().to_string())
                .chain(args.iter().cloned())
                .collect_vec();
            writeln!(tmp, "{}", script.trim())?;
            self.exec("sh", &args, task, env, prefix)
        } else {
            let args = vec!["-c".to_string(), script.trim().to_string()];
            self.exec("sh", &args, task, env, prefix)
        }
    }

    fn exec_file(
        &self,
        file: &Path,
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let command = file.to_string_lossy().to_string();
        let args = task.args.iter().cloned().collect_vec();

        let cmd = format!("{} {}", display_path(file), args.join(" "));
        let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
        info_unprefix_trunc!("{prefix} {cmd}");

        self.exec(&command, &args, task, env, prefix)
    }

    fn exec(
        &self,
        program: &str,
        args: &[String],
        task: &Task,
        env: &BTreeMap<String, String>,
        prefix: &str,
    ) -> Result<()> {
        let program = program.to_executable();
        let mut cmd = CmdLineRunner::new(program.clone()).args(args).envs(env);
        match &self.output(task)? {
            TaskOutput::Prefix => cmd = cmd.prefix(format!("{prefix} ")),
            TaskOutput::Interleave => {
                cmd = cmd
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
            }
        }
        if self.raw(task) {
            cmd.with_raw();
        }
        if let Some(cd) = &self.cd.as_ref().or(task.dir.as_ref()) {
            cmd = cmd.current_dir(cd);
        }
        if self.dry_run {
            return Ok(());
        }
        if let Err(err) = cmd.execute() {
            if let Some(ScriptFailed(_, Some(status))) = err.downcast_ref::<Error>() {
                if let Some(code) = status.code() {
                    error!("{prefix} exited with code {code}");
                    exit(code);
                } else if let Some(signal) = status.signal() {
                    error!("{prefix} killed by signal {signal}");
                    exit(1);
                }
            }
            error!("{err}");
            exit(1);
        }
        trace!("{prefix} exited successfully");
        Ok(())
    }

    fn output(&self, task: &Task) -> Result<TaskOutput> {
        let settings = Settings::get();
        if self.prefix {
            Ok(TaskOutput::Prefix)
        } else if self.interleave {
            Ok(TaskOutput::Interleave)
        } else if let Some(output) = &settings.task_output {
            Ok(output.parse()?)
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
            Ok(TaskOutput::Interleave)
        } else {
            Ok(TaskOutput::Prefix)
        }
    }

    fn raw(&self, task: &Task) -> bool {
        self.raw || task.raw || Settings::get().raw
    }

    fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            self.jobs.unwrap_or(Settings::get().jobs)
        }
    }

    fn prompt_for_task(&self, config: &Config, t: &str) -> Result<Task> {
        let tasks = config.tasks();
        let task_names = tasks.keys().sorted().collect_vec();
        let t = style(&t).yellow().for_stderr();
        let msg = format!("no task named `{t}` found. select a task to run:");
        let mut s = Select::new("Tasks").description(&msg).filterable(true);
        for name in task_names {
            s = s.option(DemandOption::new(name));
        }
        let task_name = s.run();
        match task_name {
            Ok(name) => tasks
                .get(name)
                .cloned()
                .ok_or_eyre(format!("no task named `{}` found", name)),
            Err(_) => Err(eyre!("there was an error, please try again")),
        }
    }

    fn validate_task(&self, task: &Task) -> Result<()> {
        if let Some(path) = &task.file {
            if !file::is_executable(path) {
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

    fn sources_are_fresh(&self, config: &Config, task: &Task) -> bool {
        let run = || -> Result<bool> {
            let sources = self.get_last_modified(&self.cwd(config, task), &task.sources)?;
            let outputs = self.get_last_modified(&self.cwd(config, task), &task.outputs)?;
            trace!("sources: {sources:?}, outputs: {outputs:?}",);
            match (sources, outputs) {
                (Some(sources), Some(outputs)) => Ok(sources < outputs),
                _ => Ok(false),
            }
        };
        run().unwrap_or_else(|err| {
            warn!("sources_are_fresh: {err}");
            false
        })
    }

    fn get_last_modified(&self, root: &Path, globs: &[String]) -> Result<Option<SystemTime>> {
        let last_mod = GlobWalkerBuilder::from_patterns(root, globs)
            .follow_links(true)
            .build()?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_owned())
            .unique()
            .map(|p| p.metadata().map_err(|err| eyre!(err)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .map(|m| m.modified().map_err(|err| eyre!(err)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .max();
        trace!("last_modified of {}: {last_mod:?}", globs.join(" "));
        Ok(last_mod)
    }

    fn cwd(&self, config: &Config, task: &Task) -> PathBuf {
        self.cd
            .as_ref()
            .or(task.dir.as_ref())
            .cloned()
            .or_else(|| config.project_root.clone())
            .unwrap_or_else(|| env::current_dir().unwrap().clone())
    }

    fn save_checksum(&self, task: &Task) -> Result<()> {
        if task.sources.is_empty() {
            return Ok(());
        }
        // TODO
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise run lint</bold>
  Runs the "lint" task. This needs to either be defined in .mise.toml
  or as a standalone script. See the project README for more information.

  $ <bold>mise run build --force</bold>
  Forces the "build" task to run even if its sources are up-to-date.

  $ <bold>mise run test --raw</bold>
  Runs "test" with stdin/stdout/stderr all connected to the current terminal.
  This forces `--jobs=1` to prevent interleaving of output.

  $ <bold>mise run lint ::: test ::: check</bold>
  Runs the "lint", "test", and "check" tasks in parallel.

  $ <bold>mise task cmd1 arg1 arg2 ::: cmd2 arg1 arg2</bold>
  Execute multiple tasks each with their own arguments.
"#
);

#[derive(Debug, PartialEq, EnumString)]
#[strum(serialize_all = "snake_case")]
enum TaskOutput {
    Prefix,
    Interleave,
}

fn get_color() -> Color {
    static COLORS: Lazy<Vec<Color>> = Lazy::new(|| {
        vec![
            Color::Blue,
            Color::Magenta,
            Color::Cyan,
            Color::Green,
            Color::Yellow,
            Color::Red,
        ]
    });
    static COLOR_IDX: AtomicUsize = AtomicUsize::new(0);
    COLORS[COLOR_IDX.fetch_add(1, Ordering::Relaxed) % COLORS.len()]
}

#[cfg(test)]
mod tests {
    use crate::file;

    #[test]
    fn test_task_run() {
        file::remove_all("test-build-output.txt").unwrap();
        assert_cli_snapshot!(
            "r",
            "filetask",
            "arg1",
            "arg2",
            ":::",
            "configtask",
            "arg3",
            "arg4"
        , @"");
        let body = file::read_to_string("test-build-output.txt").unwrap();
        assert_snapshot!(body, @r###"
        TEST_BUILDSCRIPT_ENV_VAR: VALID
        "###);
    }
}
