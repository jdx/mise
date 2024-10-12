use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::iter::once;
use std::path::{Path, PathBuf};
use std::process::{exit, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;

use clap::ValueHint;
use console::Color;
use demand::{DemandOption, Select};
use duct::IntoExecutablePath;
use either::Either;
use eyre::{bail, ensure, eyre, Result};
use glob::glob;
use itertools::Itertools;
use once_cell::sync::Lazy;

use super::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, CONFIG};
use crate::errors::Error;
use crate::errors::Error::ScriptFailed;
use crate::file::display_path;
use crate::task::Deps;
use crate::task::{GetMatchingExt, Task};
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::{ctrlc, prompt, style};
use crate::{dirs, env, file, ui};

/// [experimental] Run task(s)
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
#[clap(visible_alias = "r", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    /// Tasks to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: mise run task1 arg1 arg2 ::: task2 arg1 arg2
    #[clap(verbatim_doc_comment, default_value = "default")]
    pub task: String,

    /// Arguments to pass to the tasks. Use ":::" to separate tasks.
    #[clap(allow_hyphen_values = true)]
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

    /// Shows elapsed time after each tasks
    #[clap(long, alias = "timing", verbatim_doc_comment)]
    pub timings: bool,

    #[clap(skip)]
    pub is_linear: bool,
}

impl Run {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental("`mise run`")?;
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
            .map(|(t, args)| {
                let tasks = config
                    .tasks_with_aliases()?
                    .get_matching(&t)?
                    .into_iter()
                    .cloned()
                    .collect_vec();
                if tasks.is_empty() {
                    if t != "default" {
                        err_no_task(&t)?;
                    }

                    Ok(vec![self.prompt_for_task(config)?])
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

    fn parallelize_tasks(mut self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;

        ts.install_arg_versions(config, &InstallOptions::new())?;
        ts.notify_if_versions_missing();
        let mut env = ts.env_with_path(config)?;
        if let Some(root) = &config.project_root {
            env.insert("MISE_PROJECT_ROOT".into(), root.display().to_string());
            env.insert("root".into(), root.display().to_string());
        }

        let tasks = Deps::new(config, tasks)?;
        for task in tasks.all() {
            self.validate_task(task)?;
        }

        let num_tasks = tasks.all().count();
        self.is_linear = tasks.is_linear();

        let tasks = Mutex::new(tasks);
        let timer = std::time::Instant::now();

        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(self.jobs() + 1)
            .build()?;
        let exit_status = Mutex::new(None);
        pool.scope(|s| {
            let run = |task: &Task| {
                let t = task.clone();
                s.spawn(|_| {
                    let task = t;
                    trace!("running tasks: {task}");
                    if let Err(err) = self.run_task(config, &env, &task) {
                        error!("{err}");
                        if let Some(ScriptFailed(_, Some(status))) = err.downcast_ref::<Error>() {
                            *exit_status.lock().unwrap() = status.code();
                        } else {
                            *exit_status.lock().unwrap() = Some(1);
                        }
                    }
                    let mut tasks = tasks.lock().unwrap();
                    tasks.remove(&task);
                });
            };
            let rx = tasks.lock().unwrap().subscribe();
            while let Some(task) = rx.recv().unwrap() {
                if exit_status.lock().unwrap().is_some() {
                    break;
                }
                run(&task);
            }
        });

        if self.timings && num_tasks > 1 {
            let msg = format!("finished in {}", format_duration(timer.elapsed()));
            info!("{}", style::edim(msg));
        };

        if let Some(status) = *exit_status.lock().unwrap() {
            debug!("exiting with status: {status}");
            exit(status);
        }

        Ok(())
    }

    fn run_task(&self, config: &Config, env: &BTreeMap<String, String>, task: &Task) -> Result<()> {
        let prefix = style::estyle(task.prefix()).fg(get_color()).to_string();
        if !self.force && self.sources_are_fresh(config, task) {
            info_unprefix_trunc!("{prefix} sources up-to-date, skipping");
            return Ok(());
        }

        let string_env = task.env.iter().filter_map(|(k, v)| match &v.0 {
            Either::Left(v) => Some((k, v)),
            _ => None,
        });
        let rm_env = task
            .env
            .iter()
            .filter(|(_, v)| v.0 == Either::Right(false))
            .map(|(k, _)| k)
            .collect::<HashSet<_>>();
        let env: BTreeMap<String, String> = env
            .iter()
            .chain(string_env)
            .filter(|(k, _)| !rm_env.contains(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let timer = std::time::Instant::now();

        if let Some(file) = &task.file {
            self.exec_file(file, task, &env, &prefix)?;
        } else {
            for (script, args) in task.render_run_scripts_with_args(self.cd.clone(), &task.args)? {
                self.exec_script(&script, &args, task, &env, &prefix)?;
            }
        }

        if self.timings {
            miseprintln!(
                "{} finished in {}",
                prefix,
                format_duration(timer.elapsed())
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
        let cmd = style::ebold(format!("$ {script}")).bright().to_string();
        info_unprefix_trunc!("{prefix} {cmd}");

        if script.starts_with("#!") {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("script");
            let mut tmp = std::fs::File::create(&file)?;
            tmp.write_all(script.as_bytes())?;
            tmp.flush()?;
            drop(tmp);
            file::make_executable(&file)?;
            let filename = file.display().to_string();
            self.exec(&filename, args, task, env, prefix)
        } else {
            let shell = self.get_shell(task);
            trace!("using shell: {} {}", shell.0, shell.1);
            #[cfg(windows)]
            {
                let script = format!("{} {}", script, args.join(" "));
                let args = vec![shell.1, script];
                self.exec(shell.0.as_str(), &args, task, env, prefix)
            }
            #[cfg(unix)]
            {
                let script = format!("{} {}", script, shell_words::join(args));
                let args = vec![shell.1, script];
                self.exec(shell.0.as_str(), &args, task, env, prefix)
            }
        }
    }

    fn get_shell(&self, task: &Task) -> (String, String) {
        let default_shell = if cfg!(windows) {
            ("cmd".to_string(), "/c".to_string())
        } else {
            ("sh".to_string(), "-c".to_string())
        };

        if let Some(shell) = task.shell.clone() {
            let shell_cmd = shell
                .split_whitespace()
                .map(|s| s.to_string())
                .collect_tuple()
                .unwrap_or_else(|| {
                    warn!("invalid shell '{shell}', expected '<program> <argument>' (e.g. sh -c)");
                    default_shell
                });
            return shell_cmd;
        }
        default_shell
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
        let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
        info_unprefix_trunc!("{prefix} {cmd}");

        self.exec(&command, &args, task, &env, prefix)
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
        cmd.with_pass_signals();
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
        cmd.execute()?;
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

    fn prompt_for_task(&self, config: &Config) -> Result<Task> {
        let tasks = config.tasks()?;
        ensure!(
            !tasks.is_empty(),
            "no tasks defined. see {url}",
            url = style::eunderline("https://mise.jdx.dev/tasks/")
        );
        let mut s = Select::new("Tasks")
            .description("Select a tasks to run")
            .filterable(true);
        for name in tasks.keys() {
            s = s.option(DemandOption::new(name));
        }
        let _ctrlc = ctrlc::handle_ctrlc()?;
        let name = s.run()?;
        match tasks.get(name) {
            Some(task) => Ok((*task).clone()),
            None => bail!("no tasks {} found", style::ered(name)),
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

    fn get_last_modified(
        &self,
        root: &Path,
        patterns_or_paths: &[String],
    ) -> Result<Option<SystemTime>> {
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

fn err_no_task(name: &str) -> Result<()> {
    if let Some(cwd) = &*dirs::CWD {
        let includes = CONFIG.task_includes_for_dir(cwd);
        let path = includes
            .iter()
            .map(|d| d.join(name))
            .find(|d| d.is_file() && !file::is_executable(d));
        if let Some(path) = path {
            warn!(
                "no task {} found, but a non-executable file exists at {}",
                style::ered(name),
                display_path(&path)
            );
            let yn =
                prompt::confirm("Mark this file as executable to allow it to be run as a task?")?;
            if yn {
                file::make_executable(&path)?;
                info!("marked as executable, try running this task again");
            }
        }
    }
    bail!("no task {} found", style::ered(name));
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

#[derive(Debug, PartialEq, strum::EnumString)]
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

fn format_duration(dur: std::time::Duration) -> String {
    if dur < std::time::Duration::from_secs(1) {
        format!("{:.0?}", dur)
    } else {
        format!("{:.2?}", dur)
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use crate::file;
    use crate::test::reset;

    #[test]
    fn test_task_run() {
        reset();
        file::remove_all("test-build-output.txt").unwrap();
        assert_cli_snapshot!(
            "r",
            "filetask",
            "--user=jdx",
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

    #[test]
    fn test_task_custom_shell() {
        reset();
        file::remove_all("test-build-output.txt").unwrap();
        assert_cli_snapshot!(
          "r",
          "shell",
      @"");
        let body = file::read_to_string("test-build-output.txt").unwrap();
        assert_snapshot!(body, @r###"
        using shell bash
        "###);
    }

    #[test]
    fn test_task_custom_shell_invalid() {
        reset();
        assert_cli_snapshot!(
            "r",
            "shell invalid",
        @"mise invalid shell 'bash', expected '<program> <argument>' (e.g. sh -c)");
    }
}
