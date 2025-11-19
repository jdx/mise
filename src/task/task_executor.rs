use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, env_directive::EnvDirective};
use crate::file::{display_path, is_executable};
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_list::split_task_spec;
use crate::task::task_output::{TaskOutput, trunc};
use crate::task::task_output_handler::OutputHandler;
use crate::task::task_source_checker::{save_checksum, sources_are_fresh, task_cwd};
use crate::task::{Deps, FailedTasks, GetMatchingExt, Task};
use crate::ui::{style, time};
use duct::IntoExecutablePath;
use eyre::{Result, ensure, eyre};
use itertools::Itertools;
use std::collections::BTreeMap;
use std::io::Write;
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
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

    pub fn task_timings(&self) -> bool {
        let output_mode = self.output_handler.output(None);
        self.timings
            || Settings::get().task_timings.unwrap_or(
                output_mode == TaskOutput::Prefix
                    || output_mode == TaskOutput::Timed
                    || output_mode == TaskOutput::KeepOrder,
            )
    }

    pub async fn run_task_sched(
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
        if !self.force && sources_are_fresh(task, config).await? {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            return Ok(());
        }
        if let Some(message) = &task.confirm
            && !Settings::get().yes
            && !crate::ui::confirm(message).unwrap_or(false)
        {
            return Err(eyre!("aborted by user"));
        }

        let mut tools = self.tool.clone();
        for (k, v) in &task.tools {
            tools.push(format!("{k}@{v}").parse()?);
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
        let (mut env, task_env) = if let Some(task_cf) = task_cf {
            self.context_builder
                .resolve_task_env_with_config(config, task, task_cf, &ts)
                .await?
        } else {
            // Fallback to standard behavior
            task.render_env(config, &ts).await?
        };

        trace!(
            "task {} render_env took {}ms",
            task.name,
            env_render_start.elapsed().as_millis()
        );
        if !self.timings {
            env.insert("MISE_TASK_TIMINGS".to_string(), "0".to_string());
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
            .await?
            .unwrap_or(task.config_source.clone());
        env.insert("MISE_TASK_FILE".into(), task_file.display().to_string());
        if let Some(dir) = task_file.parent() {
            env.insert("MISE_TASK_DIR".into(), dir.display().to_string());
        }
        if let Some(config_root) = &task.config_root {
            env.insert("MISE_CONFIG_ROOT".into(), config_root.display().to_string());
        }
        let timer = std::time::Instant::now();

        if let Some(file) = task.file_path(config).await? {
            let exec_start = std::time::Instant::now();
            self.exec_file(config, &file, task, &env, &prefix).await?;
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

        save_checksum(task)?;

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
                    let resolved_spec = crate::task::resolve_task_pattern(spec, Some(task));
                    self.inject_and_wait(config, &[resolved_spec], task_env, sched_tx.clone())
                        .await?;
                }
                RunEntry::TaskGroup { tasks } => {
                    let resolved_tasks: Vec<String> = tasks
                        .iter()
                        .map(|t| crate::task::resolve_task_pattern(t, Some(task)))
                        .collect();
                    self.inject_and_wait(config, &resolved_tasks, task_env, sched_tx.clone())
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
            let matches = tasks_map.get_matching(name)?;
            ensure!(!matches.is_empty(), "task not found: {}", name);
            for t in matches {
                let mut t = (*t).clone();
                t.args = args.clone();
                if self.skip_deps {
                    t.depends.clear();
                    t.depends_post.clear();
                    t.wait_for.clear();
                }
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
        if is_executable(file) && !Settings::get().use_file_shell_for_executable_tasks {
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
                    cmd = cmd.with_on_stdout(|line| {
                        let mut map = self.output_handler.keep_order_output.lock().unwrap();
                        if !map.contains_key(task) {
                            map.insert(task.clone(), Default::default());
                        }
                        if let Some(entry) = map.get_mut(task) {
                            entry.0.push((prefix.to_string(), line));
                        }
                    });
                } else {
                    cmd = cmd.stdout(Stdio::null());
                }
                if !task.silent.suppresses_stderr() {
                    cmd = cmd.with_on_stderr(|line| {
                        let mut map = self.output_handler.keep_order_output.lock().unwrap();
                        if !map.contains_key(task) {
                            map.insert(task.clone(), Default::default());
                        }
                        if let Some(entry) = map.get_mut(task) {
                            entry.1.push((prefix.to_string(), line));
                        }
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
                    let pr = self.output_handler.task_prs.get(task).unwrap().clone();
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
        cmd.execute()?;
        trace!("{prefix} exited successfully");
        Ok(())
    }

    async fn parse_usage_spec_and_init_env(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &mut BTreeMap<String, String>,
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
}
