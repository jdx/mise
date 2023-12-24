use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::iter::once;
use std::os::unix::prelude::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{exit, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::SystemTime;

use clap::ValueHint;
use console::{style, Color};
use duct::IntoExecutablePath;
use eyre::Result;
use globwalk::GlobWalkerBuilder;
use itertools::Itertools;
use once_cell::sync::Lazy;
use petgraph::graph::DiGraph;
use petgraph::Direction;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::errors::Error;
use crate::errors::Error::ScriptFailed;
use crate::file::display_path;
use crate::task::Task;
use crate::toolset::{InstallOptions, ToolsetBuilder};
use crate::ui::style;
use crate::{env, file, ui};

/// [experimental] Run a task
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "r", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Run {
    /// Task to run
    /// Can specify multiple tasks by separating with `:::`
    /// e.g.: rtx run task1 arg1 arg2 ::: task2 arg1 arg2
    pub task: String,

    /// Arguments to pass to the task
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
    /// Configure with `task_output` config or `RTX_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "interleave")]
    pub prefix: bool,

    /// Print directly to stdout/stderr instead of by line
    /// Defaults to true if --jobs == 1
    /// Configure with `task_output` config or `RTX_TASK_OUTPUT` env var
    #[clap(long, short, verbatim_doc_comment, overrides_with = "prefix")]
    pub interleave: bool,

    /// Tool(s) to also add
    /// e.g.: node@20 python@3.10
    #[clap(short, long, value_name = "TOOL@VERSION", value_parser = ToolArgParser)]
    pub tool: Vec<ToolArg>,

    /// Number of tasks to run in parallel
    /// [default: 4]
    /// Configure with `jobs` config or `RTX_JOBS` env var
    #[clap(long, short, env = "RTX_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Read/write directly to stdin/stdout/stderr instead of by line
    /// Configure with `raw` config or `RTX_RAW` env var
    #[clap(long, short, verbatim_doc_comment)]
    pub raw: bool,
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
            .map(|(t, args)| match config.tasks().get(&t) {
                Some(task) => Ok(task.clone().with_args(args.to_vec())),
                None => Err(self.err_no_task(config, &t)),
            })
            .collect()
    }

    fn parallelize_tasks(self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;

        ts.install_arg_versions(config, &InstallOptions::new())?;
        ts.notify_if_versions_missing();
        let mut env = ts.env_with_path(config);
        if let Some(root) = &config.project_root {
            env.insert("RTX_PROJECT_ROOT".into(), root.display().to_string());
        }

        let tasks = Mutex::new(Deps::new(config, tasks)?);

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
        if self.dry_run {
            return Ok(());
        }
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

        if env::var("RTX_TASK_SCRIPT_FILE").is_ok() {
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
        match &self.output(task) {
            TaskOutput::Prefix => cmd = cmd.prefix(format!("{prefix} ")),
            TaskOutput::Interleave => cmd = cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit()),
        }
        if self.raw(task) {
            cmd.with_raw();
        }
        if let Some(cd) = &self.cd.as_ref().or(task.dir.as_ref()) {
            cmd = cmd.current_dir(cd);
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

    fn output(&self, task: &Task) -> TaskOutput {
        let settings = Settings::get();
        if self.prefix {
            TaskOutput::Prefix
        } else if self.interleave {
            TaskOutput::Interleave
        } else if let Some(output) = &settings.task_output {
            TaskOutput::from_str(output).unwrap()
        } else if self.raw(task) || self.jobs() == 1 {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
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

    fn err_no_task(&self, config: &Config, t: &str) -> eyre::Report {
        let tasks = config.tasks();
        let task_names = tasks.keys().sorted().map(style::ecyan).join(", ");
        let t = style(&t).yellow().for_stderr();
        eyre!("no task named `{t}` found. Available tasks: {task_names}")
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
            .unwrap_or_else(|| env::PWD.clone())
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
  $ <bold>rtx task cmd1 arg1 arg2 ::: cmd2 arg1 arg2</bold>
  TODO
"#
);

#[derive(Debug)]
struct Deps {
    graph: DiGraph<Task, ()>,
    sent: HashSet<String>,
    tx: mpsc::Sender<Option<Task>>,
}

impl Deps {
    fn new(config: &Config, tasks: Vec<Task>) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];
        for t in tasks {
            stack.push(t.clone());
            indexes
                .entry(t.name.clone())
                .or_insert_with(|| graph.add_node(t));
        }
        while let Some(a) = stack.pop() {
            let a_idx = *indexes
                .entry(a.name.clone())
                .or_insert_with(|| graph.add_node(a.clone()));
            for b in a.resolve_depends(config)? {
                let b_idx = *indexes
                    .entry(b.name.clone())
                    .or_insert_with(|| graph.add_node(b.clone()));
                graph.add_edge(a_idx, b_idx, ());
                stack.push(b.clone());
            }
        }
        let (tx, _) = mpsc::channel();
        let sent = HashSet::new();
        Ok(Self { graph, tx, sent })
    }

    fn leaves(&self) -> Vec<Task> {
        self.graph
            .externals(Direction::Outgoing)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    fn emit_leaves(&mut self) {
        let leaves = self.leaves().into_iter().collect_vec();
        for task in leaves {
            if self.sent.contains(&task.name) {
                continue;
            }
            self.sent.insert(task.name.clone());
            self.tx.send(Some(task)).unwrap();
        }
        if self.graph.node_count() == 0 {
            self.tx.send(None).unwrap();
        }
    }

    fn subscribe(&mut self) -> mpsc::Receiver<Option<Task>> {
        let (tx, rx) = mpsc::channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    // #[requires(self.graph.node_count() > 0)]
    // #[ensures(self.graph.node_count() == old(self.graph.node_count()) - 1)]
    fn remove(&mut self, task: &Task) {
        if let Some(idx) = self
            .graph
            .node_indices()
            .find(|&idx| &self.graph[idx] == task)
        {
            self.graph.remove_node(idx);
            self.emit_leaves();
        }
    }

    fn all(&self) -> impl Iterator<Item = &Task> {
        self.graph.node_indices().map(|idx| &self.graph[idx])
    }

    // fn pop(&'a mut self) -> Option<&'a Task> {
    //     if let Some(leaf) = self.leaves().first() {
    //         self.remove(&leaf.clone())
    //     } else {
    //         None
    //     }
    // }
}

#[derive(Debug, PartialEq, EnumString)]
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
