use crate::cli::args::ToolArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings, env_directive::EnvDirective};
use crate::duration;
use crate::env_diff::EnvDiff;
use crate::file::{can_execute_directly, display_path, replace_path, strip_utf8_bom};
use crate::sandbox::SandboxConfig;
use crate::task::TaskArtifactCache;
use crate::task::task_cache::{
    CommandInput, TaskCacheContext, TaskCacheMissReason, TaskCacheRestore,
};
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_list::split_task_spec;
use crate::task::task_output::{TaskOutput, trunc};
use crate::task::task_output_handler::OutputHandler;
use crate::task::task_scheduler::SchedMsg;
use crate::task::task_script_parser::subcommand_name_from_parse;
use crate::task::task_source_checker::{
    remove_auto_output, save_checksum, sources_are_fresh, task_cwd,
};
use crate::task::{
    Deps, FailedTasks, GetMatchingExt, Task, TaskCacheAudit, TaskCacheMode, TaskCacheOutput,
};
use crate::task::{TaskCompletionState, TaskDependencyState};
use crate::tera::{contains_template_syntax, render_str};
use crate::toolset::Toolset;
use crate::toolset::env_cache::CachedEnv;
use crate::ui::{style, time};
use duct::IntoExecutablePath;
use eyre::{Context, Report, Result, ensure, eyre};
use indexmap::IndexMap;
#[cfg(windows)]
use indoc::formatdoc;
use itertools::Itertools;
#[cfg(unix)]
use nix::errno::Errno;
use std::collections::{BTreeMap, HashSet};
use std::iter::once;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use xx::file;

/// Global lock for interactive task exclusivity.
/// Interactive tasks acquire a write lock (exclusive), non-interactive tasks acquire a read lock (shared).
static TASK_RUNTIME_LOCK: LazyLock<RwLock<()>> = LazyLock::new(|| RwLock::new(()));
type TaskOutputCapture = Arc<StdMutex<Vec<TaskCacheOutput>>>;
const COMMAND_INPUT_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_INPUT_MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct TaskRunContext<'a> {
    pub(crate) task: &'a Task,
    pub(crate) config: &'a Arc<Config>,
    pub(crate) sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    pub(crate) completion_state: TaskCompletionState,
    pub(crate) dependency_state: TaskDependencyState,
    pub(crate) semaphore: Arc<Semaphore>,
    pub(crate) permit: &'a mut Option<OwnedSemaphorePermit>,
    pub(crate) allow_during_interruption: bool,
}

#[derive(Clone, Copy)]
struct TaskExecContext<'a> {
    task: &'a Task,
    env: &'a BTreeMap<String, String>,
    prefix: &'a str,
    output_capture: Option<&'a TaskOutputCapture>,
    allow_during_interruption: bool,
}

struct TaskRunEntriesContext<'a> {
    config: &'a Arc<Config>,
    exec: TaskExecContext<'a>,
    task_env: &'a [(String, String)],
    sched_tx: Arc<mpsc::UnboundedSender<SchedMsg>>,
    existing_guard: Option<RuntimeLockGuard<'static>>,
    completion_state: &'a TaskCompletionState,
    semaphore: Arc<Semaphore>,
    permit: &'a mut Option<OwnedSemaphorePermit>,
}

struct PreparedTaskContext {
    toolset: Toolset,
    env: BTreeMap<String, String>,
    task_env: Vec<(String, String)>,
    extra_vars: Option<IndexMap<String, String>>,
}

#[derive(Clone, Copy)]
struct TaskInjectionContext<'a> {
    config: &'a Arc<Config>,
    task_env: &'a [(String, String)],
    sched_tx: &'a Arc<mpsc::UnboundedSender<SchedMsg>>,
    completion_state: &'a TaskCompletionState,
    allow_during_interruption: bool,
}

#[allow(dead_code)] // Guards are held for their Drop impl, not read
enum RuntimeLockGuard<'a> {
    Read(tokio::sync::RwLockReadGuard<'a, ()>),
    Write(tokio::sync::RwLockWriteGuard<'a, ()>),
}

async fn acquire_runtime_lock(interactive: bool) -> RuntimeLockGuard<'static> {
    if interactive {
        RuntimeLockGuard::Write(TASK_RUNTIME_LOCK.write().await)
    } else {
        RuntimeLockGuard::Read(TASK_RUNTIME_LOCK.read().await)
    }
}

fn resolve_task_sandbox_path(p: &Path, task_base: Option<&Path>) -> PathBuf {
    if p.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let p = replace_path(p);
    if p.is_absolute() {
        p
    } else if let Some(base) = task_base {
        base.join(p)
    } else {
        p
    }
}

/// Build the single-line command shown in a task's header (the `$ ...` line).
///
/// Skips leading shebang/blank/`set ...` boilerplate so the first real command is
/// shown, and joins backslash-continued lines into one logical line. Without the
/// join, a command wrapped across physical lines would display only its first line
/// ending in `\`, and any extra CLI args would be glued onto that dangling
/// backslash (e.g. `$ echo foo \ --bar`). Returns the whole script if it contains
/// only boilerplate.
///
/// A trailing backslash with no following line is left untouched, so a literal
/// trailing backslash that is data rather than a continuation (e.g. a Windows path
/// like `echo C:\tmp\`) is shown as-is. The remaining ambiguity — a literal `\` at
/// the end of a line that *is* followed by another line — is still treated as a
/// continuation, which is acceptable for a display-only string.
fn display_first_command(script: &str) -> String {
    let mut lines = script.lines();
    let Some(first) = lines.find(|line| {
        let t = line.trim_start();
        !t.is_empty() && !t.starts_with("#!") && t != "set" && !t.starts_with("set ")
    }) else {
        return script.to_string();
    };
    let mut cmd = first.to_string();
    while cmd.trim_end().ends_with('\\') {
        let Some(next) = lines.next() else {
            // Trailing backslash with no continuation line: keep it (literal data).
            break;
        };
        let truncated = cmd.trim_end();
        let base = truncated[..truncated.len() - 1].trim_end().to_string();
        let next = next.trim();
        cmd = if base.is_empty() || next.is_empty() {
            format!("{base}{next}")
        } else {
            format!("{base} {next}")
        };
    }
    cmd
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
enum InlineArgsStyle {
    PosixCommandText,
    CmdCommandText,
    SeparateArgv,
}

/// Whether the shell mise is about to spawn cannot use `dir` as its working directory.
///
/// cmd.exe refuses a UNC working directory. It says so on stderr, starts in `C:\Windows` instead,
/// and carries on — so the task runs somewhere the user never asked for while mise reports success.
/// Reuses [`crate::path::is_cmd_shell_program`] and [`crate::file::is_unc_path`]; both already know
/// the shapes involved, including `cmd`/`cmd.exe`/`CMD.EXE` and the verbatim `\\?\UNC\` form.
#[cfg(windows)]
fn cmd_shell_cannot_use_dir(program: &str, dir: &Path) -> bool {
    crate::path::is_cmd_shell_program(Path::new(program)) && crate::file::is_unc_path(dir)
}

/// What to say when it cannot. Names both ways out, because neither setting is guessable.
#[cfg(windows)]
fn unc_working_dir_error(dir: &Path) -> String {
    formatdoc! {r#"
        cmd.exe cannot use a UNC path as a working directory

          working directory: {dir}

        It would start in C:\Windows instead and run the command there, so mise stops rather
        than running it somewhere you did not ask for.

        Use a shell that accepts UNC paths, either for this task:

          shell = "pwsh -c"

        or for every task:

          mise settings windows_default_inline_shell_args="pwsh -c"

        A file task takes its shell from windows_default_file_shell_args instead."#,
        dir = display_path(dir),
    }
}

fn inline_args_style(program: &str, shell_args: &[String]) -> InlineArgsStyle {
    #[cfg(windows)]
    {
        let runs_command = shell_args
            .iter()
            .any(|f| f.eq_ignore_ascii_case("/c") || f.eq_ignore_ascii_case("/k"));
        if crate::path::is_cmd_shell_program(Path::new(program)) && runs_command {
            return InlineArgsStyle::CmdCommandText;
        }
        if !crate::path::is_posix_shell_program(Path::new(program)) {
            return InlineArgsStyle::SeparateArgv;
        }
    }
    #[cfg(not(windows))]
    let _ = (program, shell_args);
    InlineArgsStyle::PosixCommandText
}

fn append_inline_args(script: &str, args: &[String], style: InlineArgsStyle) -> String {
    let args = match style {
        InlineArgsStyle::PosixCommandText => shell_words::join(args),
        InlineArgsStyle::CmdCommandText => args
            .iter()
            .map(|arg| crate::path::quote_arg_for_cmd_body(arg))
            .join(" "),
        InlineArgsStyle::SeparateArgv => return script.to_string(),
    };
    match (script.is_empty(), args.is_empty()) {
        (true, true) => String::new(),
        (true, false) => args,
        (false, true) => script.to_string(),
        (false, false) => format!("{script} {args}"),
    }
}

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
    pub task_cache: TaskCacheMode,
    pub task_cache_explain: bool,
    pub task_cache_explain_json: bool,
    pub cache_session: Option<crate::cache::session::CacheSessionEnvironment>,
    /// CLI-level sandbox overrides (merged with task-level sandbox config)
    pub sandbox: crate::sandbox::SandboxConfig,
}

/// Executes tasks with proper context, environment, and output handling
pub struct TaskExecutor {
    pub context_builder: TaskContextBuilder,
    pub output_handler: OutputHandler,
    pub failed_tasks: FailedTasks,
    pub(crate) cache_stats: Arc<StdMutex<TaskCacheStats>>,
    interrupted: AtomicBool,

    // CLI flags
    pub force: bool,
    pub cd: Option<PathBuf>,
    pub shell: Option<String>,
    pub tool: Vec<ToolArg>,
    pub timings: bool,
    pub continue_on_error: bool,
    pub dry_run: bool,
    pub skip_deps: bool,
    pub task_cache: TaskCacheMode,
    pub task_cache_explain: bool,
    pub task_cache_explain_json: bool,
    pub cache_session: Option<crate::cache::session::CacheSessionEnvironment>,
    pub sandbox: crate::sandbox::SandboxConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskRunOutcome {
    pub did_work: bool,
    pub cache_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TaskCacheStats {
    pub(crate) hits: u64,
    pub(crate) misses: u64,
    pub(crate) restored_bytes: u64,
    pub(crate) time_saved: Duration,
}

impl TaskCacheStats {
    fn record_hit(&mut self, restored_bytes: u64, time_saved: Duration) {
        self.hits = self.hits.saturating_add(1);
        self.restored_bytes = self.restored_bytes.saturating_add(restored_bytes);
        self.time_saved = self.time_saved.saturating_add(time_saved);
    }

    fn record_miss(&mut self) {
        self.misses = self.misses.saturating_add(1);
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
            cache_stats: Arc::new(StdMutex::new(TaskCacheStats::default())),
            interrupted: AtomicBool::new(false),
            force: config.force,
            cd: config.cd,
            shell: config.shell,
            tool: config.tool,
            timings: config.timings,
            continue_on_error: config.continue_on_error,
            dry_run: config.dry_run,
            skip_deps: config.skip_deps,
            task_cache: config.task_cache,
            task_cache_explain: config.task_cache_explain,
            task_cache_explain_json: config.task_cache_explain_json,
            cache_session: config.cache_session,
            sandbox: config.sandbox,
        }
    }

    pub fn is_stopping(&self) -> bool {
        self.is_interrupted() || !self.failed_tasks.lock().unwrap().is_empty()
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::Relaxed)
    }

    pub fn mark_interrupted(&self) {
        self.interrupted.store(true, Ordering::Relaxed);
    }

    fn check_interruption(allow_during_interruption: bool) -> Result<()> {
        if !allow_during_interruption && crate::ui::ctrlc::is_cancelled() {
            return Err(crate::errors::Error::TaskInterrupted.into());
        }
        Ok(())
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

    /// Build a SandboxConfig for a task by merging task-level config with CLI overrides.
    ///
    /// Task-level relative `allow_read`/`allow_write` paths are resolved against the task's
    /// effective working directory (`task.dir(config)`, which itself falls back to `config_root`)
    /// so that `allow_read = ["."]` means "the directory the task runs in", matching how `dir`
    /// resolves. CLI-supplied paths are left as-is and resolved against cwd by `resolve_paths()`.
    async fn build_sandbox_for_task(
        &self,
        task: &Task,
        config: &Arc<Config>,
    ) -> Result<SandboxConfig> {
        let task_base = task.dir(config).await?;
        let resolve_task_path =
            |p: &PathBuf| -> PathBuf { resolve_task_sandbox_path(p, task_base.as_deref()) };
        let mut sandbox = SandboxConfig {
            deny_read: task.deny_all || task.deny_read || self.sandbox.deny_read,
            deny_write: task.deny_all || task.deny_write || self.sandbox.deny_write,
            deny_net: task.deny_all || task.deny_net || self.sandbox.deny_net,
            deny_env: task.deny_all || task.deny_env || self.sandbox.deny_env,
            allow_read: task
                .allow_read
                .iter()
                .map(&resolve_task_path)
                .chain(self.sandbox.allow_read.iter().cloned())
                .collect(),
            allow_write: task
                .allow_write
                .iter()
                .map(&resolve_task_path)
                .chain(self.sandbox.allow_write.iter().cloned())
                .collect(),
            allow_net: task
                .allow_net
                .iter()
                .chain(self.sandbox.allow_net.iter())
                .cloned()
                .collect(),
            allow_env: task
                .allow_env
                .iter()
                .chain(self.sandbox.allow_env.iter())
                .cloned()
                .collect(),
            pass_through_env: task
                .pass_through_env
                .iter()
                .chain(self.sandbox.pass_through_env.iter())
                .cloned()
                .collect(),
            cache_env: task
                .cache
                .iter()
                .filter(|cache| cache.enabled)
                .flat_map(|cache| &cache.env)
                .chain(self.sandbox.cache_env.iter())
                .cloned()
                .collect(),
            deny_system_temp_write: self.sandbox.deny_system_temp_write,
        };
        if task.rust_cache.as_ref().is_some_and(|cache| cache.enabled)
            && let Some(session) = &self.cache_session
        {
            if sandbox.effective_deny_read() {
                sandbox.allow_read.extend(session.sandbox_paths());
            }
            if sandbox.effective_deny_write() {
                sandbox.allow_write.extend(session.sandbox_paths());
            }
            if sandbox.effective_deny_env() {
                sandbox.pass_through_env.extend([
                    "MISE_CACHE_SOCKET".into(),
                    "MISE_CACHE_STAGING_DIR".into(),
                    "MISE_CACHE_TASK".into(),
                    "MISE_CACHE_RUST_VERIFY".into(),
                    "MISE_CACHE_PREVIOUS_RUSTC_WRAPPER".into(),
                    "RUSTC_WRAPPER".into(),
                    "CARGO_INCREMENTAL".into(),
                ]);
            }
        }
        sandbox.resolve_paths();
        Ok(sandbox)
    }

    pub fn task_timings(&self, task: Option<&Task>) -> bool {
        // Resolve the style/verbosity for *this* task so a per-task `output`
        // override is honored (e.g. a task with `output = "interleave"` must not
        // get a timing line just because the global default is `prefix`).
        let output_mode = self.output_handler.output(task);
        // Quiet/silent suppresses mise's own output, so the per-task "Finished in …"
        // timing line must not leak. This matters now that quiet keeps its style
        // (e.g. `--quiet` with parallel tasks still resolves to `Prefix`).
        let default = !self.output_handler.quiet(task)
            && (output_mode == TaskOutput::Prefix
                || output_mode == TaskOutput::Timed
                || output_mode == TaskOutput::KeepOrder);
        self.timings || Settings::get().task.timings.unwrap_or(default)
    }

    /// Run a task, returning whether it did work and any stable artifact identity
    /// it produced or reused.
    pub async fn run_task_sched(&self, ctx: TaskRunContext<'_>) -> Result<TaskRunOutcome> {
        let TaskRunContext {
            task,
            config,
            sched_tx,
            completion_state,
            dependency_state,
            semaphore,
            permit,
            allow_during_interruption,
        } = ctx;
        let prefix = task.estyled_prefix();
        let total_start = std::time::Instant::now();
        Self::check_interruption(allow_during_interruption)?;
        if Settings::get().task.skip.contains(&task.name) {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "skipping task");
            }
            return Ok(TaskRunOutcome::default());
        }
        // If any dependency executed or restored, skip the source freshness check
        // so that downstream tasks are invalidated by upstream changes.
        let artifact_cache_enabled =
            self.task_cache.enabled() && task.cache.as_ref().is_some_and(|cache| cache.enabled);
        if !artifact_cache_enabled
            && !self.force
            && !dependency_state.any_did_work
            && sources_are_fresh(task, config).await?
        {
            if !self.quiet(Some(task)) {
                self.eprint(task, &prefix, "sources up-to-date, skipping");
            }
            return Ok(TaskRunOutcome::default());
        }

        let PreparedTaskContext {
            toolset: ts,
            mut env,
            task_env,
            extra_vars,
        } = self.prepare_task_context(config, task).await?;
        let task_file = self
            .parse_task_usage(config, task, &mut env, extra_vars.clone())
            .await?;

        // Confirmation must happen before a cache restore because restoring
        // outputs mutates the working tree just like executing the task.
        let confirm_guard = if task.interactive {
            Some(acquire_runtime_lock(task.interactive).await)
        } else {
            None
        };
        self.check_confirmation(config, task, &env).await?;

        let artifact_cache = if self.task_cache.enabled()
            && task.cache.as_ref().is_some_and(|cache| cache.enabled)
        {
            match TaskArtifactCache::prepare(task, config, self.dry_run).await? {
                Some(_)
                    if self.dry_run
                        && !self.task_cache_explain
                        && !self.task_cache_explain_json =>
                {
                    None
                }
                Some(_)
                    if self.raw(Some(task))
                        && !self.task_cache_explain
                        && !self.task_cache_explain_json =>
                {
                    warn!(
                        "task {} artifact caching disabled for raw or interactive execution",
                        task.name
                    );
                    None
                }
                Some(prepared) => {
                    let command_inputs = self
                        .resolve_cache_command_inputs(task, config, &env)
                        .await?;
                    let cache = prepared
                        .finish(TaskCacheContext {
                            task,
                            config,
                            toolset: &ts,
                            resolved_env: &env,
                            declared_env: &task_env,
                            dependency_keys: &dependency_state.cache_keys,
                            command_inputs,
                            explain: self.task_cache_explain || self.task_cache_explain_json,
                            mode: self.task_cache,
                        })
                        .await?;
                    if let Some(explanation) = cache.explanation() {
                        if self.task_cache_explain_json {
                            miseprintln!("{}", explanation.to_json(&task.name, cache.key())?);
                        } else if !self.quiet(Some(task)) {
                            for line in explanation.lines() {
                                self.eprint(task, &prefix, &line);
                            }
                        }
                    }
                    let raw = self.raw(Some(task));
                    if raw {
                        warn!(
                            "task {} artifact caching disabled for raw or interactive execution",
                            task.name
                        );
                    }
                    let bypass_cache = self.dry_run || raw;
                    let current_output = if !bypass_cache
                        && self.task_cache.reads()
                        && !self.force
                        && !dependency_state.any_unkeyed_did_work
                        && (task.outputs.is_no_files() || sources_are_fresh(task, config).await?)
                    {
                        cache.current_output().await
                    } else {
                        None
                    };
                    if let Some(output) = current_output {
                        if !self.quiet(Some(task)) {
                            self.eprint(task, &prefix, "sources up-to-date, skipping");
                        }
                        self.output_handler
                            .replay_cached_output(task, &prefix, &output);
                        return Ok(TaskRunOutcome {
                            did_work: false,
                            cache_key: Some(cache.key().to_string()),
                        });
                    }
                    if bypass_cache {
                        None
                    } else {
                        let miss_reason = if !self.task_cache.reads() {
                            TaskCacheMissReason::ReadDisabled
                        } else if self.force {
                            TaskCacheMissReason::Forced
                        } else if dependency_state.any_unkeyed_did_work {
                            TaskCacheMissReason::DependencyWithoutKey
                        } else {
                            Self::check_interruption(allow_during_interruption)?;
                            match cache.restore(task).await? {
                                TaskCacheRestore::Hit(hit) => {
                                    self.cache_stats
                                        .lock()
                                        .unwrap()
                                        .record_hit(hit.restored_bytes, hit.saved_duration);
                                    if !self.quiet(Some(task)) {
                                        let kind = if task.outputs.is_no_files() {
                                            "result"
                                        } else {
                                            "outputs"
                                        };
                                        self.eprint(
                                            task,
                                            &prefix,
                                            &format!("restored {kind} from cache {}", cache.key()),
                                        );
                                    }
                                    self.output_handler.replay_cached_output(
                                        task,
                                        &prefix,
                                        &hit.output,
                                    );
                                    if let Err(err) = save_checksum(task, config).await {
                                        warn!(
                                            "task {} artifact cache checksum update failed: {err}",
                                            task.name
                                        );
                                    }
                                    if self.task_cache.writes()
                                        && let Err(err) = cache.mark_current()
                                    {
                                        warn!(
                                            "task {} artifact cache state update failed: {err}",
                                            task.name
                                        );
                                    }
                                    return Ok(TaskRunOutcome {
                                        did_work: true,
                                        cache_key: Some(cache.key().to_string()),
                                    });
                                }
                                TaskCacheRestore::Miss(reason) => reason,
                            }
                        };
                        self.cache_stats.lock().unwrap().record_miss();
                        if !self.quiet(Some(task)) {
                            self.eprint(task, &prefix, &format!("cache miss: {miss_reason}"));
                        }
                        Some(cache)
                    }
                }
                None => None,
            }
        } else {
            None
        };
        let output_capture = artifact_cache
            .as_ref()
            .filter(|_| self.task_cache.writes())
            .map(|_| Arc::new(StdMutex::new(Vec::new())));
        let action_cache_run = if let Some(session) = self.cache_session.as_ref() {
            session.apply(task, &mut env).await
        } else {
            None
        };
        let exec_ctx = TaskExecContext {
            task,
            env: &env,
            prefix: &prefix,
            output_capture: output_capture.as_ref(),
            allow_during_interruption,
        };

        let timer = std::time::Instant::now();

        if let Some(file) = task_file {
            let exec_start = std::time::Instant::now();
            Self::check_interruption(allow_during_interruption)?;
            remove_auto_output(task, config).await?;
            self.exec_file(config, &file, confirm_guard, exec_ctx)
                .await?;
            trace!(
                "task {} exec_file took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        } else {
            let rendered_run_scripts = task
                .render_run_scripts_with_args(
                    config,
                    self.cd.clone(),
                    &task.args,
                    &env,
                    extra_vars.clone(),
                )
                .await?;

            let exec_start = std::time::Instant::now();
            Self::check_interruption(allow_during_interruption)?;
            remove_auto_output(task, config).await?;
            self.exec_task_run_entries(
                rendered_run_scripts,
                TaskRunEntriesContext {
                    config,
                    exec: exec_ctx,
                    task_env: &task_env,
                    sched_tx,
                    existing_guard: confirm_guard,
                    completion_state: &completion_state,
                    semaphore,
                    permit,
                },
            )
            .await?;
            trace!(
                "task {} exec_task_run_entries took {}ms (total {}ms)",
                task.name,
                exec_start.elapsed().as_millis(),
                total_start.elapsed().as_millis()
            );
        }

        let execution_duration = timer.elapsed();
        if self.task_timings(Some(task))
            && (task.file.as_ref().is_some() || !task.run_script_strings().is_empty())
        {
            self.eprint(
                task,
                &prefix,
                &format!("Finished in {}", time::format_duration(execution_duration)),
            );
        }

        save_checksum(task, config).await?;
        let cache_key = if self.task_cache.writes()
            && let Some(cache) = artifact_cache
        {
            let output = output_capture
                .as_ref()
                .map(|output| output.lock().unwrap().clone())
                .unwrap_or_default();
            match cache.store(task, &output, execution_duration).await {
                Ok(()) => {
                    if let Err(err) = cache.mark_current() {
                        warn!(
                            "task {} artifact cache state update failed: {err}",
                            task.name
                        );
                    }
                    Some(cache.key().to_string())
                }
                Err(err) => {
                    warn!("task {} artifact cache write failed: {err}", task.name);
                    None
                }
            }
        } else {
            None
        };
        if let Some(run) = action_cache_run
            && let Err(err) = run.commit().await
        {
            warn!("task {} action manifest write failed: {err}", task.name);
        }

        Ok(TaskRunOutcome {
            did_work: true,
            cache_key,
        })
    }

    fn insert_env_excluded_from_nested_mise_diff(
        env: &mut BTreeMap<String, String>,
        excluded_keys: &mut HashSet<String>,
        key: &str,
        value: String,
    ) {
        env.insert(key.to_string(), value);
        if key != crate::env::PATH_KEY.as_str() {
            excluded_keys.insert(key.to_string());
        }
    }

    fn env_for_nested_mise_diff(
        &self,
        env: &BTreeMap<String, String>,
        excluded_keys: &HashSet<String>,
    ) -> BTreeMap<String, String> {
        let mut env = env.clone();
        for key in excluded_keys {
            env.remove(key);
        }
        env
    }

    async fn exec_task_run_entries(
        &self,
        rendered_scripts: Vec<(String, Vec<String>)>,
        ctx: TaskRunEntriesContext<'_>,
    ) -> Result<()> {
        let TaskRunEntriesContext {
            config,
            exec,
            task_env,
            sched_tx,
            existing_guard,
            completion_state,
            semaphore,
            permit,
        } = ctx;
        let task = exec.task;
        use crate::task::RunEntry;
        let mut script_iter = rendered_scripts.into_iter();
        let mut completion_state = completion_state.clone();

        let needs_tera = task.run().iter().any(RunEntry::has_tera_template);
        let mut tera_state = if needs_tera {
            let usage_values = crate::task::parse_usage_values_from_task(config, task).await?;
            let config_root = task.config_root.clone().unwrap_or_default();
            let tera = crate::tera::get_tera(Some(&config_root));
            let mut tera_ctx = task.tera_ctx_for_usage(config).await?;
            if !usage_values.is_empty() {
                tera_ctx.insert("usage", &usage_values);
            }
            tera_ctx.insert("env", exec.env);
            Some((tera, tera_ctx))
        } else {
            None
        };

        // Use an existing guard (e.g. from confirmation) or acquire a new one.
        // The lock is held across consecutive script entries for exclusivity
        // and temporarily dropped around inject_and_wait to avoid deadlocking.
        let mut guard = match existing_guard {
            Some(g) => Some(g),
            None => Some(acquire_runtime_lock(task.interactive).await),
        };
        for raw_entry in task.run() {
            let rendered;
            let entry = if let Some((ref mut tera, ref tera_ctx)) = tera_state
                && raw_entry.has_tera_template()
            {
                rendered = raw_entry.render(tera, tera_ctx)?;
                &rendered
            } else {
                raw_entry
            };
            match entry {
                RunEntry::Script(_) => {
                    if let Some((script, args)) = script_iter.next() {
                        if guard.is_none() {
                            guard = Some(acquire_runtime_lock(task.interactive).await);
                        }
                        self.exec_script(&script, &args, exec).await?;
                    }
                }
                RunEntry::SingleTask {
                    task: spec,
                    args: entry_args,
                    env: entry_env,
                } => {
                    let resolved_spec = crate::task::resolve_task_pattern(spec, Some(task));
                    let override_args = if entry_args.is_empty() {
                        None
                    } else {
                        Some(entry_args.clone())
                    };
                    let override_env: Vec<(String, String)> = entry_env
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    let override_env_ref = if override_env.is_empty() {
                        None
                    } else {
                        Some(override_env.as_slice())
                    };
                    guard = None; // drop lock before waiting on sub-tasks
                    // Release the semaphore permit before waiting on sub-tasks to
                    // avoid deadlock when MISE_JOBS=1 (the sub-task needs a permit
                    // but we're holding the only one).
                    let had_permit = permit.is_some();
                    *permit = None;
                    let completed = self
                        .inject_and_wait(
                            &[resolved_spec],
                            override_args.as_deref(),
                            override_env_ref,
                            TaskInjectionContext {
                                config,
                                task_env,
                                sched_tx: &sched_tx,
                                completion_state: &completion_state,
                                allow_during_interruption: exec.allow_during_interruption,
                            },
                        )
                        .await?;
                    completion_state.merge(completed);
                    if had_permit {
                        *permit = Some(semaphore.clone().acquire_owned().await?);
                    }
                }
                RunEntry::TaskGroup { tasks } => {
                    let resolved_tasks: Vec<String> = tasks
                        .iter()
                        .map(|t| crate::task::resolve_task_pattern(t, Some(task)))
                        .collect();
                    guard = None; // drop lock before waiting on sub-tasks
                    let had_permit = permit.is_some();
                    *permit = None;
                    let completed = self
                        .inject_and_wait(
                            &resolved_tasks,
                            None,
                            None,
                            TaskInjectionContext {
                                config,
                                task_env,
                                sched_tx: &sched_tx,
                                completion_state: &completion_state,
                                allow_during_interruption: exec.allow_during_interruption,
                            },
                        )
                        .await?;
                    completion_state.merge(completed);
                    if had_permit {
                        *permit = Some(semaphore.clone().acquire_owned().await?);
                    }
                }
            }
        }
        Ok(())
    }

    async fn inject_and_wait(
        &self,
        specs: &[String],
        override_args: Option<&[String]>,
        override_env: Option<&[(String, String)]>,
        ctx: TaskInjectionContext<'_>,
    ) -> Result<TaskCompletionState> {
        let TaskInjectionContext {
            config,
            task_env,
            sched_tx,
            completion_state,
            allow_during_interruption,
        } = ctx;
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
            .values()
            .flat_map(|t| {
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
                t.args = override_args
                    .map(|a| a.to_vec())
                    .unwrap_or_else(|| args.clone());
                // Apply entry-level env via with_dependency_env (high priority,
                // consistent with depends/depends_post) so it overrides the
                // sub-task's own declared env.
                if let Some(env) = override_env {
                    let env_directives: Vec<EnvDirective> = env
                        .iter()
                        .map(|(k, v)| EnvDirective::Val(k.clone(), v.clone(), Default::default()))
                        .collect();
                    t = t.with_dependency_env(&env_directives);
                    if let Some(config_root) = &t.config_root {
                        let env_map: IndexMap<String, String> = env.iter().cloned().collect();
                        t.outputs.re_render_with_env(
                            &t.raw_outputs.clone(),
                            &env_map,
                            config_root,
                        )?;
                    } else {
                        trace!(
                            "re_render_with_env skipped: task {} has no config_root",
                            t.name
                        );
                    }
                }
                if self.skip_deps {
                    t.depends.clear();
                    t.depends_post.clear();
                    t.wait_for.clear();
                }
                to_run.push(t);
            }
        }
        let sub_deps = Deps::new_pruned(config, to_run, completion_state).await?;
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
                            let _ = sched_tx.send(SchedMsg::new(
                                task,
                                sub_deps_clone.clone(),
                                allow_during_interruption,
                            ));
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
                            let _ = sched_tx.send(SchedMsg::new(
                                task,
                                sub_deps_clone.clone(),
                                allow_during_interruption,
                            ));
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
            if self.is_stopping() && !self.continue_on_error && !allow_during_interruption {
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
        if self.is_stopping() && !self.continue_on_error && !allow_during_interruption {
            return Err(eyre!("task sequence aborted due to failure"));
        }

        let completion_state = sub_deps.lock().await.completion_state();
        Ok(completion_state)
    }

    async fn exec_script(
        &self,
        script: &str,
        args: &[String],
        ctx: TaskExecContext<'_>,
    ) -> Result<()> {
        let config = Config::get().await?;
        let script = script.trim_start();
        let display_script = self.display_script_with_args(script, args, ctx.task)?;
        // For display, skip leading shebang/blank/`set ...` boilerplate and join
        // backslash-continued lines so the header shows the first real command as a
        // single logical line (see display_first_command). When show_full_cmd is set,
        // keep the whole script instead — the reduction would otherwise discard every
        // line past the first command, making the setting a no-op (#10469, #9844).
        let display_script = if Settings::get().task.show_full_cmd {
            display_script
        } else {
            display_first_command(&display_script)
        };
        let cmd = match display_script.is_empty() {
            true => "$".to_string(),
            false => format!("$ {display_script}"),
        };
        if !self.quiet(Some(ctx.task)) {
            let msg = style::ebold(trunc(ctx.prefix, config.redact(&cmd).trim()))
                .bright()
                .to_string();
            self.eprint(ctx.task, ctx.prefix, &msg)
        }

        if script.starts_with("#!") {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("script");
            tokio::fs::write(&file, script.as_bytes()).await?;
            file::make_executable(&file)?;
            self.exec_with_text_file_busy_retry(&file, args, ctx).await
        } else {
            let (program, args, cmd_verbatim) =
                self.get_cmd_program_and_args(script, ctx.task, args)?;
            self.exec_program(&program, &args, cmd_verbatim, ctx).await
        }
    }

    /// Build the script text represented by the task command header.
    ///
    /// Inline task arguments follow the same shell-specific strategy used for
    /// execution before the first command is selected for display. Shebang tasks
    /// receive arguments as script argv instead, so attaching them to a command in
    /// the script would be misleading.
    fn display_script_with_args(
        &self,
        script: &str,
        args: &[String],
        task: &Task,
    ) -> Result<String> {
        if script.starts_with("#!") || args.is_empty() {
            return Ok(script.to_string());
        }
        let shell = task.shell()?.unwrap_or(self.clone_default_inline_shell()?);
        let (program, shell_args) = task_shell_parts(&shell, "inline shell")?;
        Ok(append_inline_args(
            script,
            args,
            inline_args_style(program, shell_args),
        ))
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
        let mut shell = task
            .shell()?
            .or_else(|| shell_from_shebang(file))
            .or_else(|| shell_from_extension(file))
            .unwrap_or(Settings::get().default_file_shell()?);
        Settings::get().maybe_no_profile(&mut shell);
        let (program, _) = task_shell_parts(&shell, "file shell")?;
        trace!("using shell: {}", shell.join(" "));
        let mut full_args = shell.to_vec();
        full_args.push(display);
        if !args.is_empty() {
            full_args.extend(args.iter().cloned());
        }
        Ok((program.to_string(), full_args[1..].to_vec()))
    }

    /// Build the `(program, args, cmd_verbatim)` for an inline script. When
    /// `cmd_verbatim` is true (Windows + a `cmd.exe` inline shell), `args` are
    /// already wrapped for cmd and must be appended to the command line
    /// verbatim (via `CmdLineRunner::raw_arg`) rather than through std's
    /// MSVCRT-style quoting — see the windows branch below and discussion #9355.
    fn get_cmd_program_and_args(
        &self,
        script: &str,
        task: &Task,
        args: &[String],
    ) -> Result<(String, Vec<String>, bool)> {
        let shell = task.shell()?.unwrap_or(self.clone_default_inline_shell()?);
        let (program, _shell_args) = task_shell_parts(&shell, "inline shell")?;
        trace!("using shell: {}", shell.join(" "));
        let mut full_args = shell.clone();

        #[cfg(windows)]
        {
            // When the inline shell is cmd.exe, hand the script to cmd verbatim
            // instead of letting std::process::Command apply MSVCRT-style
            // quoting. std would wrap the script in quotes and escape any inner
            // `"` as `\"`, but cmd.exe does not understand that escaping, so
            // commands like `python -c "import x"` get mangled (the child sees
            // `\"import`). We assemble the whole command into one body, wrap it
            // in a single outer quote pair, and use `/s` so cmd strips exactly
            // that pair and runs the rest — inner quotes included — verbatim.
            // See discussion #9355.
            match inline_args_style(program, _shell_args) {
                InlineArgsStyle::CmdCommandText => {
                    let cmd_args = crate::path::cmd_verbatim_args(_shell_args, script, args);
                    return Ok((program.to_string(), cmd_args, true));
                }
                InlineArgsStyle::SeparateArgv => {
                    // Non-POSIX, non-cmd shells (e.g. `pwsh -Command`) use a
                    // different quoting convention than `shell_words` (which is
                    // POSIX), so keep passing forwarded args as separate argv.
                    full_args.push(script.to_string());
                    full_args.extend(args.iter().cloned());
                    return Ok((program.to_string(), full_args[1..].to_vec(), false));
                }
                InlineArgsStyle::PosixCommandText => {}
            }
        }

        // Shared (Unix, and Windows POSIX shells like `bash -c`): append the
        // forwarded args to the command string so they reach an inline `-c` shell
        // as part of the command — the documented behavior for inline TOML
        // scripts — rather than as positional parameters. Passing them as separate
        // argv to `bash -c` on Windows shifted the user's first arg into `$0`.
        // See #9355.
        let mut script = script.to_string();
        if !args.is_empty() {
            script = format!("{script} {}", shell_words::join(args));
        }
        full_args.push(script);
        Ok((program.to_string(), full_args[1..].to_vec(), false))
    }

    fn clone_default_inline_shell(&self) -> Result<Vec<String>> {
        if let Some(shell) = &self.shell {
            let mut shell = crate::path::split_shell_command(shell)?;
            Settings::get().maybe_no_profile(&mut shell);
            Ok(shell)
        } else {
            Settings::get().default_inline_shell()
        }
    }

    async fn resolve_cache_command_inputs(
        &self,
        task: &Task,
        config: &Arc<Config>,
        resolved_env: &BTreeMap<String, String>,
    ) -> Result<Vec<CommandInput>> {
        let cache = task.cache.as_ref().expect("cache must be configured");
        if cache.command_inputs.is_empty() {
            return Ok(Vec::new());
        }
        let root = task_cwd(task, config).await?;
        let sandbox = self.build_sandbox_for_task(task, config).await?;
        let filtered_env = if sandbox.is_active() {
            sandbox.filter_env(resolved_env)
        } else {
            resolved_env.clone()
        };
        let timeout = task
            .timeout
            .as_ref()
            .and_then(|value| match duration::parse_duration(value) {
                Ok(timeout) => Some(timeout),
                Err(err) => {
                    warn!("invalid timeout {:?} for task {}: {err}", value, task.name);
                    None
                }
            })
            .unwrap_or(COMMAND_INPUT_TIMEOUT);
        let mut inputs = Vec::with_capacity(cache.command_inputs.len());
        for command in &cache.command_inputs {
            if command.trim().is_empty() {
                eyre::bail!(
                    "task {} cache command input must not be empty: {command:?}",
                    task.name
                );
            }
            let (program, args, cmd_verbatim) =
                self.get_cmd_program_and_args(command, task, &[])?;
            // The same refusal as in `exec_program`, and it matters more here: these commands feed
            // the cache key, so running them from C:\Windows would hash the wrong directory's
            // answer. Measured on 2026.8.6 with the project on a UNC share — a `command_inputs`
            // entry reading a file that exists in the project fails, while the same config on a
            // local path succeeds. `--dry-run` does not reach here (see the cache branch in
            // `run_task`), so there is nothing to exempt.
            #[cfg(windows)]
            if cmd_shell_cannot_use_dir(&program, &root) {
                eyre::bail!("{}", unc_working_dir_error(&root));
            }
            #[cfg(not(windows))]
            let _ = cmd_verbatim;
            let program = program.to_executable();
            #[cfg(windows)]
            let program = crate::path::resolve_posix_shell_program_path(&program, &filtered_env)
                .unwrap_or(program);
            let env = maybe_convert_env_for_msys_shell(Path::new(&program), &filtered_env);
            let runner = CmdLineRunner::new(program);
            #[cfg(windows)]
            let runner = if cmd_verbatim {
                args.iter().fold(runner, |runner, arg| runner.raw_arg(arg))
            } else {
                runner.args(&args)
            };
            #[cfg(not(windows))]
            let runner = runner.args(&args);
            let mut runner = runner
                .current_dir(&root)
                .env_clear()
                .envs(env.as_ref())
                .with_timeout(timeout)
                .with_sandbox(sandbox.clone());
            runner.apply_sandbox().await?;
            let (stdout_hash, stderr_hash) = runner
                .execute_hashes_async(COMMAND_INPUT_MAX_OUTPUT_BYTES)
                .await
                .wrap_err_with(|| {
                    format!("task {} cache command input failed: {command:?}", task.name)
                })?;
            inputs.push(CommandInput {
                command: command.clone(),
                stdout_hash,
                stderr_hash,
            });
        }
        Ok(inputs)
    }

    async fn exec_file(
        &self,
        config: &Arc<Config>,
        file: &Path,
        guard: Option<RuntimeLockGuard<'static>>,
        ctx: TaskExecContext<'_>,
    ) -> Result<()> {
        let args = ctx.task.args.iter().cloned().collect_vec();

        if !self.quiet(Some(ctx.task)) {
            let cmd = format!("{} {}", display_path(file), args.join(" "))
                .trim()
                .to_string();
            let cmd = style::ebold(format!("$ {cmd}")).bright().to_string();
            let cmd = trunc(ctx.prefix, config.redact(&cmd).trim());
            self.eprint(ctx.task, ctx.prefix, &cmd);
        }

        let _guard = if guard.is_some() {
            guard
        } else {
            Some(acquire_runtime_lock(ctx.task.interactive).await)
        };
        self.exec(file, &args, ctx).await
    }

    async fn exec(&self, file: &Path, args: &[String], ctx: TaskExecContext<'_>) -> Result<()> {
        let (program, args) = self.get_file_program_and_args(file, ctx.task, args)?;
        self.exec_program(&program, &args, false, ctx).await
    }

    async fn exec_with_text_file_busy_retry(
        &self,
        file: &Path,
        args: &[String],
        ctx: TaskExecContext<'_>,
    ) -> Result<()> {
        const ETXTBUSY_RETRIES: usize = 3;
        const ETXTBUSY_SLEEP_MS: u64 = 50;

        let mut attempt = 0;
        loop {
            match self.exec(file, args, ctx).await {
                Ok(()) => break Ok(()),
                Err(err) if Self::is_text_file_busy(&err) && attempt < ETXTBUSY_RETRIES => {
                    attempt += 1;
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
        cmd_verbatim: bool,
        ctx: TaskExecContext<'_>,
    ) -> Result<()> {
        let TaskExecContext {
            task,
            env,
            prefix,
            output_capture,
            allow_during_interruption,
        } = ctx;
        #[cfg(not(windows))]
        let _ = cmd_verbatim;
        // `program` is shadowed several times below — `to_executable`, POSIX-shell resolution,
        // audit wrapping — so keep the shell mise was asked to spawn for the working-directory
        // check further down. `cmd_verbatim` is not a substitute: it is only set for inline
        // scripts, and a file task can reach cmd.exe through windows_default_file_shell_args.
        #[cfg(windows)]
        let requested_program = program.to_string();
        let config = Config::get().await?;
        let program = program.to_executable();
        let redactions = config.redactions();
        let raw = self.raw(Some(task));
        let sandbox = self.build_sandbox_for_task(task, &config).await?;
        let env = if sandbox.is_active() {
            &sandbox.filter_env(env)
        } else {
            env
        };
        // On Windows, when about to spawn a POSIX shell, resolve the program to
        // an absolute path *before* converting PATH for the child. Otherwise the
        // converted Unix-form PATH is also what Win32 CreateProcess uses to find
        // the program, and `bash` cannot be located in `/c/...:/c/...` entries.
        #[cfg(windows)]
        let program =
            crate::path::resolve_posix_shell_program_path(&program, env).unwrap_or(program);
        let env = maybe_convert_env_for_msys_shell(Path::new(&program), env);
        let audit = if raw || self.dry_run {
            None
        } else {
            TaskCacheAudit::prepare(task, &config).await?
        };
        let (program, args) = if let Some(audit) = &audit {
            audit.wrap(program, args)
        } else {
            (program, args.to_vec())
        };
        let runner = CmdLineRunner::new(program.clone());
        // On Windows, `cmd_verbatim` means `args` are already wrapped for cmd.exe
        // and must be appended to the command line without std's MSVCRT-style
        // quoting (which would escape inner `"` as `\"` and break the command).
        // See `get_cmd_program_and_args` and discussion #9355.
        #[cfg(windows)]
        let runner = if cmd_verbatim {
            args.iter().fold(runner, |r, a| r.raw_arg(a))
        } else {
            runner.args(&args)
        };
        #[cfg(not(windows))]
        let runner = runner.args(&args);
        // Command inherits the current process environment in addition to the
        // explicit task env, so remove usage_* keys that argument parsing
        // intentionally cleared from the task env.
        let inherited_usage_keys = std::env::vars_os()
            .filter(|(key, _)| {
                let key = key.to_string_lossy();
                crate::task::is_usage_env_key(&key)
                    && !crate::task::env_contains_key(env.as_ref(), &key)
            })
            .map(|(key, _)| key);
        let runner = inherited_usage_keys.fold(runner, |runner, key| runner.env_remove(key));
        let mut cmd = runner
            .envs(env.as_ref())
            .redact(redactions.deref().clone())
            .raw(raw)
            .with_sandbox(sandbox);
        if raw && !redactions.is_empty() {
            if task.interactive && !task.raw && !Settings::get().raw {
                hint!(
                    "interactive_redactions",
                    "interactive tasks bypass redactions—secrets may appear in terminal output",
                    ""
                );
            } else {
                hint!(
                    "raw_redactions",
                    "--raw will prevent mise from being able to use redactions",
                    ""
                );
            }
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
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stdout(|_| {});
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
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stderr(|_| {});
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::KeepOrder => {
                if !task.silent.suppresses_stdout() {
                    let state = self.output_handler.keep_order_state.clone();
                    let task_clone = task.clone();
                    let prefix_str = prefix.to_string();
                    cmd = cmd.with_on_stdout(move |line| {
                        state
                            .lock()
                            .unwrap()
                            .on_stdout(&task_clone, prefix_str.clone(), line);
                    });
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stdout(|_| {});
                } else {
                    cmd = cmd.stdout(Stdio::null());
                }
                if !task.silent.suppresses_stderr() {
                    let state = self.output_handler.keep_order_state.clone();
                    let task_clone = task.clone();
                    let prefix_str = prefix.to_string();
                    cmd = cmd.with_on_stderr(move |line| {
                        state
                            .lock()
                            .unwrap()
                            .on_stderr(&task_clone, prefix_str.clone(), line);
                    });
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stderr(|_| {});
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::Replacing => {
                // Replacing mode shows a progress indicator unless both streams are suppressed
                if task.silent.suppresses_stdout() {
                    if output_capture.is_some() {
                        cmd = cmd.with_on_stdout(|_| {});
                    } else {
                        cmd = cmd.stdout(Stdio::null());
                    }
                }
                if task.silent.suppresses_stderr() {
                    if output_capture.is_some() {
                        cmd = cmd.with_on_stderr(|_| {});
                    } else {
                        cmd = cmd.stderr(Stdio::null());
                    }
                }
                // Show progress indicator except when both streams are fully suppressed
                if !task.silent.suppresses_both() {
                    let pr = self.output_handler.get_or_init_task_pr(task);
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
                            .insert(prefix.to_string(), (SystemTime::now(), vec![line]));
                    });
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stdout(|_| {});
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
                } else if output_capture.is_some() {
                    cmd = cmd.with_on_stderr(|_| {});
                } else {
                    cmd = cmd.stderr(Stdio::null());
                }
            }
            TaskOutput::Silent => {
                if output_capture.is_some() {
                    cmd = cmd.with_on_stdout(|_| {}).with_on_stderr(|_| {});
                } else {
                    cmd = cmd.stdout(Stdio::null()).stderr(Stdio::null());
                }
            }
            // `Quiet` is no longer returned by `output()` (verbosity is decoupled
            // from style; it maps to `Interleave`), but the variant still exists as
            // a config value so it's kept here for match exhaustiveness.
            TaskOutput::Quiet | TaskOutput::Interleave => {
                if raw || redactions.is_empty() {
                    cmd = cmd.stdin(Stdio::inherit());
                }
                if output_capture.is_some() {
                    if task.silent.suppresses_stdout() {
                        cmd = cmd.with_on_stdout(|_| {});
                    }
                    if task.silent.suppresses_stderr() {
                        cmd = cmd.with_on_stderr(|_| {});
                    }
                } else if raw || redactions.is_empty() {
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
        if let Some(output_capture) = output_capture {
            let stdout = output_capture.clone();
            let stderr = output_capture.clone();
            cmd = cmd
                .with_stdout_observer(move |line| {
                    stdout
                        .lock()
                        .unwrap()
                        .push(TaskCacheOutput::Stdout(line.to_string()));
                })
                .with_stderr_observer(move |line| {
                    stderr
                        .lock()
                        .unwrap()
                        .push(TaskCacheOutput::Stderr(line.to_string()));
                });
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
        // Not under `--dry-run`: nothing is spawned there, so a preview of what *would* run has no
        // reason to fail on where it would have run.
        #[cfg(windows)]
        if !self.dry_run && cmd_shell_cannot_use_dir(&requested_program, &dir) {
            eyre::bail!("{}", unc_working_dir_error(&dir));
        }
        cmd = cmd.current_dir(dir);
        if self.dry_run {
            return Ok(());
        }
        let effective_timeout =
            task.timeout
                .as_ref()
                .and_then(|s| match duration::parse_duration(s) {
                    Ok(d) => Some(d),
                    Err(e) => {
                        warn!("invalid timeout {:?} for task {}: {e}", s, task.name);
                        None
                    }
                });
        if let Some(timeout) = effective_timeout {
            cmd = cmd.with_timeout(timeout);
        }
        // Apply sandbox async (DNS resolution for macOS) before spawning.
        cmd.apply_sandbox().await?;
        let result = cmd
            .execute_async_with_cancel_check(|| {
                !allow_during_interruption && crate::ui::ctrlc::is_cancelled()
            })
            .await;
        if let Some(audit) = audit {
            audit.report(task);
        }
        result?;
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

    fn parse_confirm_default(default: &str) -> Result<bool> {
        match default.trim().to_ascii_lowercase().as_str() {
            "yes" | "y" | "true" => Ok(true),
            "no" | "n" | "false" => Ok(false),
            _ => Err(eyre!(
                "invalid task confirm default: {default:?}, expected one of yes/no/y/n/true/false"
            )),
        }
    }

    async fn check_confirmation(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &BTreeMap<String, String>,
    ) -> Result<()> {
        if let Some(confirm) = &task.confirm
            && !Settings::get().yes
        {
            let message = if contains_template_syntax(confirm.message()) {
                let config_root = task.config_root.clone().unwrap_or_default();
                let mut tera = crate::tera::get_tera(Some(&config_root));
                let mut tera_ctx = task.tera_ctx_for_usage(config).await?;

                // Add usage values from parsed environment
                let mut usage_ctx = std::collections::HashMap::new();
                for (key, value) in env {
                    if let Some(usage_key) = key.strip_prefix("usage_") {
                        usage_ctx.insert(usage_key.to_string(), tera::Value::from(value.clone()));
                    }
                }
                tera_ctx.insert("usage", &usage_ctx);
                render_str(&mut tera, confirm.message(), &tera_ctx)?
            } else {
                confirm.message().to_string()
            };
            let default_yes = match confirm.default_value() {
                Some(default) => Self::parse_confirm_default(default)?,
                None => true, // keep backwards compatible default of yes if not specified
            };
            if !crate::ui::prompt::confirm_with_default(&message, default_yes).unwrap_or(false) {
                return Err(eyre!("aborted by user"));
            }
        }
        Ok(())
    }

    /// Validate a task invocation before the scheduler starts any task commands.
    /// Runtime execution repeats this work so configuration changes made while
    /// dependencies run are still detected.
    pub async fn preflight_task_usage(&self, config: &Arc<Config>, task: &Task) -> Result<()> {
        if task.should_bypass_usage_parser() {
            return Ok(());
        }

        // This path must remain side-effect free: a dependency may create an
        // env file consumed by this task, and source/plugin env hooks must not
        // run once here and again during execution. The display parser extracts
        // the usage declaration without resolving the complete task environment.
        let dynamic_usage = contains_template_syntax(&task.usage)
            || (task.usage.trim().is_empty()
                && task
                    .run_script_strings()
                    .iter()
                    .any(|script| contains_template_syntax(script)));
        if contains_template_syntax(&task.usage) {
            task.validate_template_syntax_for_preflight(&task.usage)
                .wrap_err_with(|| format!("invalid usage template for task {}", task.name))?;
        }
        if task.usage.trim().is_empty() {
            for script in task
                .run_script_strings()
                .into_iter()
                .filter(|script| contains_template_syntax(script))
            {
                task.validate_template_syntax_for_preflight(&script)
                    .wrap_err_with(|| {
                        format!("invalid task script template for task {}", task.name)
                    })?;
            }
        }
        if dynamic_usage {
            // The side-effect-free context intentionally omits task/subproject
            // env and vars. A dynamic template can still render successfully
            // with those empty maps while producing a different spec from the
            // runtime context, so only its syntax is safe to validate here.
            debug!(
                "deferring dynamic usage argument validation for task {} until execution",
                task.name
            );
            return Ok(());
        }
        let spec = task.parse_usage_spec_for_preflight(config).await?;

        let mut env = crate::env::PRISTINE_ENV.clone();
        for directive in task.inherited_env.0.iter().chain(task.env.0.iter()) {
            Self::apply_literal_preflight_env(&mut env, directive);
        }
        for (directive, _) in &task.overlay_env {
            Self::apply_literal_preflight_env(&mut env, directive);
        }

        let task_file = task.file_path_raw();
        let usage_args: Vec<String> = if let Some(file) = &task_file {
            once(file.to_string_lossy().to_string())
                .chain(task.args.iter().cloned())
                .collect()
        } else {
            once(String::new())
                .chain(task.args.iter().cloned())
                .collect()
        };
        match self.parse_usage_spec_and_init_env_from_spec(task, &mut env, &usage_args, &spec) {
            Ok(()) => Ok(()),
            Err(_) if Self::has_unavailable_required_env_input(&spec.cmd, &env) => {
                let mut probe_env = env.clone();
                Self::fill_unavailable_required_env_inputs(&spec.cmd, &mut probe_env);
                match self.parse_usage_spec_and_init_env_from_spec(
                    task,
                    &mut probe_env,
                    &usage_args,
                    &spec,
                ) {
                    Ok(()) => {
                        debug!(
                            "deferring environment-backed usage validation for task {} until execution",
                            task.name
                        );
                        Ok(())
                    }
                    Err(independent_err) => Err(independent_err),
                }
            }
            Err(err) => Err(err),
        }
    }

    fn apply_literal_preflight_env(env: &mut BTreeMap<String, String>, directive: &EnvDirective) {
        match directive {
            EnvDirective::Val(key, value, _) if !contains_template_syntax(value) => {
                env.insert(key.clone(), value.clone());
            }
            EnvDirective::Default(key, value, _)
                if !contains_template_syntax(value)
                    && env.get(key).is_none_or(|current| current.is_empty()) =>
            {
                env.insert(key.clone(), value.clone());
            }
            EnvDirective::Rm(key, _) => {
                env.remove(key);
            }
            _ => {}
        }
    }

    fn has_unavailable_required_env_input(
        cmd: &usage::SpecCommand,
        env: &BTreeMap<String, String>,
    ) -> bool {
        cmd.args
            .iter()
            .any(|arg| arg.required && arg.env.as_ref().is_some_and(|key| !env.contains_key(key)))
            || cmd.flags.iter().any(|flag| {
                flag.required && flag.env.as_ref().is_some_and(|key| !env.contains_key(key))
            })
            || cmd
                .subcommands
                .values()
                .any(|subcmd| Self::has_unavailable_required_env_input(subcmd, env))
    }

    fn fill_unavailable_required_env_inputs(
        cmd: &usage::SpecCommand,
        env: &mut BTreeMap<String, String>,
    ) {
        for key in cmd
            .args
            .iter()
            .filter(|arg| arg.required)
            .filter_map(|arg| arg.env.as_ref())
            .chain(
                cmd.flags
                    .iter()
                    .filter(|flag| flag.required)
                    .filter_map(|flag| flag.env.as_ref()),
            )
        {
            env.entry(key.clone())
                .or_insert_with(|| "__MISE_PREFLIGHT_ENV_INPUT__".to_string());
        }
        for subcmd in cmd.subcommands.values() {
            Self::fill_unavailable_required_env_inputs(subcmd, env);
        }
    }

    async fn prepare_task_context(
        &self,
        config: &Arc<Config>,
        task: &Task,
    ) -> Result<PreparedTaskContext> {
        let mut tools = self.tool.clone();
        tools.extend(task.tool_args()?);
        let ts_build_start = std::time::Instant::now();

        // Remote tasks need tools from the full config hierarchy rather than a
        // config file rooted at their downloaded source.
        let task_cf = if task.is_remote() {
            None
        } else {
            task.cf(config)
        };
        let toolset = self
            .context_builder
            .build_toolset_for_task(config, task, task_cf, &tools)
            .await?;
        trace!(
            "task {} ToolsetBuilder::build took {}ms",
            task.name,
            ts_build_start.elapsed().as_millis()
        );

        let env_render_start = std::time::Instant::now();
        // extra_vars contains resolved vars from the task's config hierarchy.
        let (mut env, task_env, extra_vars) = if let Some(task_cf) = task_cf {
            self.context_builder
                .resolve_task_env_with_config(config, task, task_cf, &toolset)
                .await?
        } else {
            let (env, task_env) = task.render_env(config, &toolset).await?;
            (env, task_env, None)
        };
        trace!(
            "task {} render_env took {}ms",
            task.name,
            env_render_start.elapsed().as_millis()
        );

        let mut nested_mise_diff_exclude_keys: HashSet<String> = task_env
            .iter()
            .map(|(key, _)| key.clone())
            .filter(|key| key.as_str() != crate::env::PATH_KEY.as_str())
            .chain(once("__MISE_DIFF".to_string()))
            .collect();
        if !self.timings {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_TASK_TIMINGS",
                "0".to_string(),
            );
        }
        if !crate::env::MISE_ENV.is_empty() {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_ENV",
                crate::env::MISE_ENV.join(","),
            );
        }
        if let Some(cwd) = &*crate::dirs::CWD {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_ORIGINAL_CWD",
                cwd.display().to_string(),
            );
        }

        // Prefer the task's own config root for project tasks. Global and
        // remote tasks retain the invoking project's root.
        let project_root = if task.global || task.is_remote() {
            config.project_root.clone().or(task.config_root.clone())
        } else {
            task.config_root.clone().or(config.project_root.clone())
        };
        if let Some(root) = project_root {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_PROJECT_ROOT",
                root.display().to_string(),
            );
        }
        if let Some(monorepo_root) = config.monorepo_root() {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_MONOREPO_ROOT",
                monorepo_root.display().to_string(),
            );
        }
        Self::insert_env_excluded_from_nested_mise_diff(
            &mut env,
            &mut nested_mise_diff_exclude_keys,
            "MISE_TASK_NAME",
            task.name.clone(),
        );
        let task_color = self.output_handler.task_prefix_color(task);
        Self::insert_env_excluded_from_nested_mise_diff(
            &mut env,
            &mut nested_mise_diff_exclude_keys,
            "MISE_TASK_COLOR",
            task_color,
        );
        let task_file = task
            .file_path(config)
            .await?
            .unwrap_or(task.config_source.clone());
        Self::insert_env_excluded_from_nested_mise_diff(
            &mut env,
            &mut nested_mise_diff_exclude_keys,
            "MISE_TASK_FILE",
            task_file.display().to_string(),
        );
        if let Some(dir) = task_file.parent() {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_TASK_DIR",
                dir.display().to_string(),
            );
        }
        if let Some(config_root) = &task.config_root {
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "MISE_CONFIG_ROOT",
                config_root.display().to_string(),
            );
        }
        if Settings::get().env_cache {
            let key = CachedEnv::ensure_encryption_key();
            Self::insert_env_excluded_from_nested_mise_diff(
                &mut env,
                &mut nested_mise_diff_exclude_keys,
                "__MISE_ENV_CACHE_KEY",
                key,
            );
        }

        let env_for_diff = self.env_for_nested_mise_diff(&env, &nested_mise_diff_exclude_keys);
        if let Ok(serialized) =
            EnvDiff::from_final_env(&crate::env::PRISTINE_ENV, &env_for_diff).serialize()
        {
            env.insert("__MISE_DIFF".into(), serialized);
        }

        Ok(PreparedTaskContext {
            toolset,
            env,
            task_env,
            extra_vars,
        })
    }

    async fn parse_task_usage(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &mut BTreeMap<String, String>,
        extra_vars: Option<IndexMap<String, String>>,
    ) -> Result<Option<PathBuf>> {
        let task_file = task.file_path(config).await?;
        let usage_args = || {
            if let Some(file) = &task_file {
                once(file.to_string_lossy().to_string())
                    .chain(task.args.iter().cloned())
                    .collect()
            } else {
                once(String::new())
                    .chain(task.args.iter().cloned())
                    .collect()
            }
        };
        self.parse_usage_spec_and_init_env(config, task, env, usage_args, extra_vars)
            .await?;
        Ok(task_file)
    }

    async fn parse_usage_spec_and_init_env(
        &self,
        config: &Arc<Config>,
        task: &Task,
        env: &mut BTreeMap<String, String>,
        get_args: impl Fn() -> Vec<String>,
        extra_vars: Option<IndexMap<String, String>>,
    ) -> Result<()> {
        let bypass_usage_parser = task.should_bypass_usage_parser();
        if !task.raw_args {
            // usage_* variables are outputs of this task's argument parser,
            // so they must not influence spec discovery or parsing.
            crate::task::clear_usage_env(env);
        }
        let (spec, _) = task
            .parse_usage_spec_with_vars(config, self.cd.clone(), env, extra_vars)
            .await?;
        if bypass_usage_parser {
            trace!("Usage parser bypassed");
            return Ok(());
        }
        let args = get_args();
        self.parse_usage_spec_and_init_env_from_spec(task, env, &args, &spec)
    }

    fn parse_usage_spec_and_init_env_from_spec(
        &self,
        task: &Task,
        env: &mut BTreeMap<String, String>,
        args: &[String],
        spec: &usage::Spec,
    ) -> Result<()> {
        if !spec.cmd.args.is_empty()
            || !spec.cmd.flags.is_empty()
            || !spec.cmd.subcommands.is_empty()
        {
            let args = task.args_for_usage_parser(spec, args);
            trace!("Parsing usage spec for {:?}", args);
            // Pass env vars to Parser so it can resolve env= defaults in usage specs
            let env_map: std::collections::HashMap<String, String> =
                env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let po = usage::Parser::new(spec)
                .with_env(env_map)
                .parse(&args)
                .map_err(|err| eyre!(err))?;
            for (k, v) in po.as_env() {
                trace!("Adding key {} value {} in env", k, v);
                env.insert(k, v);
            }
            // always export $usage_cmd when spec has subcommands so
            // shell scripts with `set -u` don't fail when none is chosen
            if !spec.cmd.subcommands.is_empty() {
                env.entry("usage_cmd".to_string()).or_default();
            }
            if let Some(subcmd) = subcommand_name_from_parse(&po.cmds) {
                trace!("Adding key usage_cmd value {} in env", subcmd);
                env.insert("usage_cmd".to_string(), subcmd);
            }
        } else {
            trace!("Usage spec has no args, flags, or subcommands");
        }

        Ok(())
    }
}

/// Determine the shell from a file's extension.
/// e.g. `.ps1` → `["pwsh", "-File"]`
///
/// This covers exactly the extensions [`crate::file::can_execute_directly`] rejects as
/// interpreter-only, so every file the spawn predicate turns down has an explicit
/// interpreter here rather than falling through to `windows_default_file_shell_args`.
///
/// `.vbs` names `cscript`, the *console* script host, instead of letting `cmd /c` resolve
/// the file association: the registered handler is whichever host the machine has, and
/// `wscript` — the Windows-based host — writes its output to message boxes rather than the
/// pipes mise reads. Naming the console host keeps task output capturable, the same reason
/// `.ps1` names `pwsh -File`. `//nologo` keeps the banner out of the task's output.
///
/// That arm is `cfg(windows)`-only because `cscript` does not exist elsewhere, and this
/// function is not gated — selecting it on unix would replace the configured default file
/// shell with a program that cannot be found. `.ps1` needs no gate: `pwsh` is
/// cross-platform, which is why that arm predates this and was never gated.
///
/// Matched case-insensitively because `can_execute_directly` is, so `TASK.PS1` cannot be
/// rejected there and then miss its interpreter here.
fn shell_from_extension(path: &Path) -> Option<Vec<String>> {
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "ps1" => Some(vec!["pwsh".to_string(), "-File".to_string()]),
        #[cfg(windows)]
        "vbs" => Some(vec!["cscript".to_string(), "//nologo".to_string()]),
        _ => None,
    }
}

fn task_shell_parts<'a>(shell: &'a [String], shell_kind: &str) -> Result<(&'a str, &'a [String])> {
    shell
        .split_first()
        .map(|(program, args)| (program.as_str(), args))
        .ok_or_else(|| {
            eyre!("{shell_kind} is empty; check task shell, --shell, or default shell settings")
        })
}

/// On Windows, when spawning a POSIX-style shell (bash/sh/zsh/...) for a task, the
/// child needs PATH in MSYS Unix format — `/c/foo:/d/bar` rather than `C:\foo;D:\bar`.
/// PowerShell-launched mise inherits no `MSYSTEM`, so the conversion has to happen
/// here at the spawn boundary (driven by the target program), not in mise's own env.
///
/// The cfg-attribute pattern keeps the call site OS-agnostic and avoids cloning the
/// env on the common path (Windows + non-POSIX-shell, or any non-Windows host).
fn maybe_convert_env_for_msys_shell<'a>(
    program: &Path,
    env: &'a BTreeMap<String, String>,
) -> std::borrow::Cow<'a, BTreeMap<String, String>> {
    #[cfg(windows)]
    {
        if crate::path::is_posix_shell_program(program)
            && let Some(path_val) = env.get(&*crate::env::PATH_KEY)
            // Skip the clone+convert cycle when PATH is already in Unix form (no
            // `;` separator, no `\` to translate). This is the common case when
            // mise itself runs inside Git Bash and spawns another bash subshell.
            && (path_val.contains(';') || path_val.contains('\\'))
        {
            let drive_prefix = msys_drive_prefix_for(program, env);
            let converted = crate::path::windows_path_list_to_unix(path_val, &drive_prefix);
            let mut new_env = env.clone();
            new_env.insert((*crate::env::PATH_KEY).to_string(), converted);
            return std::borrow::Cow::Owned(new_env);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = program;
    }
    std::borrow::Cow::Borrowed(env)
}

/// The cygdrive prefix inserted before drive letters when converting PATH for a
/// POSIX shell. `is_cygwin_shell` only selects the *default* when no override is set:
/// empty for MSYS2 / Git Bash (`/c/...`), `/cygdrive` for Cygwin (`/cygdrive/c/...`).
///
/// The `cygdrive` automount mechanism is shared by Cygwin and MSYS2 / Git Bash — both
/// let the user change the mount root in `/etc/fstab` (Cygwin's default is `/cygdrive`,
/// MSYS2's is `/`). mise does not parse fstab, so `MISE_CYGDRIVE_PREFIX` is an explicit
/// override honored for *both* shells. A trailing `/` is trimmed since the converter
/// emits its own separator after the prefix, so `MISE_CYGDRIVE_PREFIX=/` collapses to
/// the MSYS `/c/...` form. A non-empty value that is not absolute (no leading `/`, e.g.
/// `mnt`) would produce relative PATH entries that bash silently ignores, so it is
/// rejected with a warning and the shell's default is used instead.
#[cfg(windows)]
fn msys_drive_prefix_for(program: &Path, env: &BTreeMap<String, String>) -> String {
    // Default automount root when no override is set: empty for Git Bash / MSYS2
    // (`/c/...`), `/cygdrive` for Cygwin (`/cygdrive/c/...`).
    let default = if crate::path::is_cygwin_shell(program) {
        "/cygdrive"
    } else {
        ""
    };
    let raw = env
        .get("MISE_CYGDRIVE_PREFIX")
        .cloned()
        .or_else(|| std::env::var("MISE_CYGDRIVE_PREFIX").ok())
        .filter(|s| !s.is_empty());
    let Some(mut s) = raw else {
        return default.to_string();
    };
    // Trim trailing slashes in place — the converter appends its own separator.
    s.truncate(s.trim_end_matches('/').len());
    if s.is_empty() {
        // `MISE_CYGDRIVE_PREFIX=/` → empty prefix → MSYS `/c/...` form.
        String::new()
    } else if s.starts_with('/') {
        s
    } else {
        // Describe the default clearly: an empty prefix is the Git Bash `/c/...` form,
        // otherwise the Cygwin `/cygdrive` root.
        let default_desc = if default.is_empty() {
            "the Git Bash `/c/...` form".to_string()
        } else {
            format!("the default `{default}`")
        };
        warn!(
            "MISE_CYGDRIVE_PREFIX={s:?} is not absolute (must start with `/`); \
             using {default_desc}"
        );
        default.to_string()
    }
}

/// Read the shebang from a file and parse it into a shell command.
/// e.g. `#!/usr/bin/env bash` → `["bash"]`
/// e.g. `#!/bin/bash` → `["/bin/bash"]`
///
/// A byte-order mark in front of the `#!` is skipped, the same way `crate::file::has_shebang`
/// skips one when deciding the file is a task at all. Without that, a marked script fell through
/// to `default_file_shell` -- `cmd /c` on Windows -- not the interpreter its author named.
fn shell_from_shebang(path: &Path) -> Option<Vec<String>> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut first_line = String::new();
    reader.read_line(&mut first_line).ok()?;
    let shebang = strip_utf8_bom(&first_line).strip_prefix("#!")?;
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
    use super::*;

    #[test]
    fn task_cache_stats_saturate_and_accumulate() {
        let mut stats = TaskCacheStats::default();
        stats.record_miss();
        stats.record_hit(512, Duration::from_millis(25));
        stats.record_hit(256, Duration::from_millis(15));

        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.restored_bytes, 768);
        assert_eq!(stats.time_saved, Duration::from_millis(40));
    }

    fn env_with_path(path: &str) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert((*crate::env::PATH_KEY).to_string(), path.to_string());
        env.insert("OTHER".to_string(), "unchanged".to_string());
        env
    }

    /// Not gated on Windows: the mark reaches a shared repository from any platform, and the
    /// fallback it caused -- `default_file_shell` instead of the named interpreter -- is wrong
    /// everywhere, just most visible where that default is `cmd /c`.
    #[test]
    fn shell_from_shebang_looks_past_a_utf8_bom() {
        let tmp = tempfile::tempdir().unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let path = tmp.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        const SCRIPT: &[u8] = b"#!/usr/bin/env bash\necho hi\n";
        let mut marked = b"\xef\xbb\xbf".to_vec();
        marked.extend_from_slice(SCRIPT);

        let expected = Some(vec!["bash".to_string()]);
        assert_eq!(shell_from_shebang(&write("bom", &marked)), expected);
        // Control: the unmarked twin resolves the same way, so the mark was the only difference.
        assert_eq!(shell_from_shebang(&write("plain", SCRIPT)), expected);
        // A file with no shebang still yields nothing, so callers keep falling back as before.
        assert_eq!(shell_from_shebang(&write("none", b"echo hi\n")), None);
    }

    #[test]
    #[cfg(windows)]
    fn test_shell_from_extension_has_a_mapping_for_every_interpreter_only_extension() {
        // The invariant, enforced rather than merely documented: an extension that
        // `file::can_execute_directly` rejects as interpreter-only must be named in
        // `shell_from_extension`, or the task silently falls through to
        // `windows_default_file_shell_args` — and for .vbs that means whichever script host
        // the machine's file association points at, where `wscript` writes to message boxes
        // instead of the pipes mise reads.
        //
        // Derived from the setting rather than from a hand-kept list of interpreter-only
        // extensions, so that adding one to the shipped default without a mapping here is caught
        // too. `shell_from_extension` cannot do the check itself: it is not cfg(windows)-gated
        // while `os_can_launch_extension` is.
        let needs_interpreter: Vec<String> = Settings::get()
            .windows_executable_extensions
            .iter()
            .filter(|ext| !crate::file::os_can_launch_extension(ext))
            .cloned()
            .collect();
        // Guards the loop below against passing vacuously if the two lists ever stop overlapping.
        assert!(
            !needs_interpreter.is_empty(),
            "expected the default windows_executable_extensions to include extensions the OS \
             cannot launch (ps1, vbs)"
        );
        for ext in needs_interpreter {
            let path = PathBuf::from(format!("task.{ext}"));
            assert!(
                shell_from_extension(&path).is_some(),
                "{ext} is executable per settings but the OS cannot launch it, and it has no \
                 interpreter mapping"
            );
        }
        // The console host specifically, and case-insensitively.
        for name in ["task.vbs", "task.VBS"] {
            assert_eq!(
                shell_from_extension(Path::new(name)),
                Some(vec!["cscript".to_string(), "//nologo".to_string()])
            );
        }
    }

    #[test]
    #[cfg(not(windows))]
    fn test_shell_from_extension_leaves_vbs_to_the_default_shell_off_windows() {
        // `cscript` is Windows-only, so selecting it here would swap the configured default
        // file shell for a program that does not exist. `shell_from_extension` is not
        // cfg-gated, so the arm has to be.
        assert_eq!(shell_from_extension(Path::new("task.vbs")), None);
        assert_eq!(shell_from_extension(Path::new("task.VBS")), None);
    }

    #[test]
    fn test_shell_from_extension_maps_ps1_on_every_platform() {
        // `pwsh` is cross-platform, so this arm is deliberately ungated. Case-insensitive
        // because the predicate that rejects `.ps1` for direct execution is.
        assert_eq!(
            shell_from_extension(Path::new("task.ps1")),
            Some(vec!["pwsh".to_string(), "-File".to_string()])
        );
        assert_eq!(
            shell_from_extension(Path::new("task.PS1")),
            Some(vec!["pwsh".to_string(), "-File".to_string()])
        );
        // Anything else keeps using the configured default file shell.
        assert_eq!(shell_from_extension(Path::new("task.sh")), None);
        assert_eq!(shell_from_extension(Path::new("task")), None);
    }

    #[test]
    fn test_task_shell_parts_errors_on_empty_shell() {
        let shell = Vec::new();
        let err = task_shell_parts(&shell, "inline shell").unwrap_err();
        assert!(err.to_string().contains("inline shell is empty"));
    }

    #[test]
    fn test_task_shell_parts_splits_program_and_args() {
        let shell = vec!["cmd".to_string(), "/c".to_string()];
        let (program, args) = task_shell_parts(&shell, "inline shell").unwrap();
        assert_eq!(program, "cmd");
        assert_eq!(args, &["/c"]);
    }

    #[test]
    fn test_resolve_task_sandbox_path_expands_home_before_task_base() {
        let resolved =
            resolve_task_sandbox_path(Path::new("~/sandbox-path"), Some(Path::new("/task/base")));

        assert_eq!(resolved, crate::dirs::HOME.join("sandbox-path"));
    }

    #[test]
    fn test_resolve_task_sandbox_path_uses_task_base_for_relative_paths() {
        let resolved =
            resolve_task_sandbox_path(Path::new("sandbox-path"), Some(Path::new("/task/base")));

        assert_eq!(resolved, PathBuf::from("/task/base/sandbox-path"));
    }

    #[test]
    fn test_resolve_task_sandbox_path_preserves_empty_paths_for_filtering() {
        let resolved = resolve_task_sandbox_path(Path::new(""), Some(Path::new("/task/base")));

        assert_eq!(resolved, PathBuf::new());
    }

    #[test]
    fn test_display_first_command_plain() {
        assert_eq!(display_first_command("echo hi"), "echo hi");
    }

    #[test]
    fn test_display_first_command_skips_boilerplate() {
        let script = "#!/usr/bin/env bash\nset -Eeuo pipefail\necho hi";
        assert_eq!(display_first_command(script), "echo hi");
    }

    #[test]
    fn test_display_first_command_joins_continuations() {
        let script = "echo long_command \\\n  --option1 value1 \\\n  --option2";
        assert_eq!(
            display_first_command(script),
            "echo long_command --option1 value1 --option2"
        );
    }

    #[test]
    fn test_display_first_command_joins_continuations_after_boilerplate() {
        let script = "#!/usr/bin/env bash\nset -e\necho foo \\\n  --bar";
        assert_eq!(display_first_command(script), "echo foo --bar");
    }

    #[test]
    fn test_display_first_command_keeps_literal_trailing_backslash() {
        // A trailing backslash with no following line is treated as literal data
        // (it cannot be a continuation), so it is preserved rather than dropped or
        // joined. Only genuine multi-line continuations are merged.
        assert_eq!(display_first_command("echo foo \\"), "echo foo \\");
    }

    #[test]
    fn test_display_first_command_keeps_windows_path_trailing_backslash() {
        // A Windows path ending in a backslash is data, not a line continuation,
        // and must be shown verbatim in the header.
        assert_eq!(display_first_command("echo C:\\tmp\\"), "echo C:\\tmp\\");
    }

    #[test]
    fn test_display_first_command_all_boilerplate_returns_script() {
        let script = "#!/usr/bin/env bash\nset -e";
        assert_eq!(display_first_command(script), script);
    }

    #[test]
    fn test_display_first_command_header_has_no_dangling_backslash_with_args() {
        // Reproduces #10083: the joined command plus extra args must not contain
        // the `\ ` sequence that confused the original output.
        let args = ["--extra".to_string(), "args".to_string()];
        let display_script = append_inline_args(
            "echo long_command \\\n  --option1 value1",
            &args,
            InlineArgsStyle::PosixCommandText,
        );
        let header = format!("$ {}", display_first_command(&display_script));
        assert_eq!(header, "$ echo long_command --option1 value1 --extra args");
        assert!(!header.contains("\\ "));
    }

    #[test]
    fn test_append_inline_args_uses_posix_quoting() {
        let args = ["a with space".to_string(), "second".to_string()];
        assert_eq!(
            append_inline_args(
                "echo first\necho last",
                &args,
                InlineArgsStyle::PosixCommandText
            ),
            "echo first\necho last 'a with space' second"
        );
    }

    #[test]
    fn test_append_inline_args_uses_cmd_quoting() {
        let args = ["a with space".to_string(), "a&b".to_string()];
        assert_eq!(
            append_inline_args("echo", &args, InlineArgsStyle::CmdCommandText),
            r#"echo "a with space" "a&b""#
        );
    }

    #[test]
    fn test_append_inline_args_keeps_separate_argv_off_command_text() {
        let args = ["a with space".to_string()];
        assert_eq!(
            append_inline_args("Write-Output $args", &args, InlineArgsStyle::SeparateArgv),
            "Write-Output $args"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_converts_for_bash() {
        let env = env_with_path(r"C:\Users\me\.rustup\bin;D:\tools\bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/c/Users/me/.rustup/bin:/d/tools/bin"
        );
        assert_eq!(out.get("OTHER").unwrap(), "unchanged");
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_skips_for_cmd() {
        let env = env_with_path(r"C:\Users\me\.rustup\bin;D:\tools\bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("cmd.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            r"C:\Users\me\.rustup\bin;D:\tools\bin"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_full_path_to_bash() {
        let env = env_with_path(r"C:\foo;D:\bar");
        let out =
            maybe_convert_env_for_msys_shell(Path::new(r"C:\Program Files\Git\bin\bash.exe"), &env);
        assert_eq!(out.get(&*crate::env::PATH_KEY).unwrap(), "/c/foo:/d/bar");
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_uses_cygdrive_for_cygwin_bash() {
        // A Cygwin bash (detected by the `cygwin64` path segment) needs the
        // `/cygdrive/c/...` form, not Git Bash's `/c/...`.
        let env = env_with_path(r"C:\foo;D:\bar");
        let out = maybe_convert_env_for_msys_shell(Path::new(r"C:\cygwin64\bin\bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/cygdrive/c/foo:/cygdrive/d/bar"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_honors_cygdrive_prefix_override() {
        // A non-default cygdrive mount (e.g. fstab `/mnt`) is supplied via
        // MISE_CYGDRIVE_PREFIX in the task env rather than parsed from fstab.
        let mut env = env_with_path(r"C:\foo;D:\bar");
        env.insert("MISE_CYGDRIVE_PREFIX".to_string(), "/mnt".to_string());
        let out = maybe_convert_env_for_msys_shell(Path::new(r"C:\cygwin64\bin\bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/mnt/c/foo:/mnt/d/bar"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_honors_cygdrive_prefix_for_git_bash() {
        // The cygdrive automount root is configurable in MSYS2 / Git Bash too (not just
        // Cygwin). A Git Bash user with a non-default fstab mount supplies it via
        // MISE_CYGDRIVE_PREFIX; without it the default would (wrongly) be `/c/...`.
        let mut env = env_with_path(r"C:\foo;D:\bar");
        env.insert("MISE_CYGDRIVE_PREFIX".to_string(), "/mnt".to_string());
        let out =
            maybe_convert_env_for_msys_shell(Path::new(r"C:\Program Files\Git\bin\bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/mnt/c/foo:/mnt/d/bar"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_rejects_relative_cygdrive_prefix() {
        // A prefix without a leading slash (e.g. `mnt`) would yield relative PATH
        // entries bash ignores; fall back to the shell's default instead. For the
        // Cygwin binary used here that default is `/cygdrive` (Git Bash would fall
        // back to an empty prefix, i.e. the `/c/...` form).
        let mut env = env_with_path(r"C:\foo;D:\bar");
        env.insert("MISE_CYGDRIVE_PREFIX".to_string(), "mnt".to_string());
        let out = maybe_convert_env_for_msys_shell(Path::new(r"C:\cygwin64\bin\bash.exe"), &env);
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/cygdrive/c/foo:/cygdrive/d/bar"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_cygdrive_prefix_slash_is_msys() {
        // `MISE_CYGDRIVE_PREFIX=/` trims to empty → MSYS `/c/...` form.
        let mut env = env_with_path(r"C:\foo;D:\bar");
        env.insert("MISE_CYGDRIVE_PREFIX".to_string(), "/".to_string());
        let out = maybe_convert_env_for_msys_shell(Path::new(r"C:\cygwin64\bin\bash.exe"), &env);
        assert_eq!(out.get(&*crate::env::PATH_KEY).unwrap(), "/c/foo:/d/bar");
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_borrows_when_path_already_unix() {
        // PATH already in Unix form (no `;` and no `\`) — Cow stays Borrowed,
        // env is not cloned. Common when mise runs from Git Bash itself.
        let env = env_with_path("/c/foo:/d/bar:/usr/bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
        assert_eq!(
            out.get(&*crate::env::PATH_KEY).unwrap(),
            "/c/foo:/d/bar:/usr/bin"
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_maybe_convert_env_for_msys_shell_borrows_when_path_missing() {
        // No PATH at all — also no clone.
        let mut env = BTreeMap::new();
        env.insert("OTHER".to_string(), "unchanged".to_string());
        let out = maybe_convert_env_for_msys_shell(Path::new("bash.exe"), &env);
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    #[cfg(not(windows))]
    fn test_maybe_convert_env_for_msys_shell_noop_on_unix() {
        let env = env_with_path("/usr/bin:/bin");
        let out = maybe_convert_env_for_msys_shell(Path::new("bash"), &env);
        assert_eq!(out.get(&*crate::env::PATH_KEY).unwrap(), "/usr/bin:/bin");
    }

    #[test]
    #[cfg(windows)]
    fn cmd_will_not_take_a_unc_working_directory() {
        // Measured on 2026.8.6 against \\wsl.localhost\<distro>\...: `cmd /c cd` printed
        // C:\Windows and the task still reported success, so the spawn has to be refused.
        assert!(cmd_shell_cannot_use_dir(
            "cmd.exe",
            Path::new(r"\\server\share\proj")
        ));
        // The verbatim form std hands back from canonicalize names the same directory.
        assert!(cmd_shell_cannot_use_dir(
            "cmd.exe",
            Path::new(r"\\?\UNC\server\share\proj")
        ));
    }

    #[test]
    #[cfg(windows)]
    fn another_shell_on_the_same_unc_directory_is_left_alone() {
        // Control: pwsh runs in that directory correctly, so the shell is half of the decision.
        assert!(!cmd_shell_cannot_use_dir(
            "pwsh",
            Path::new(r"\\server\share\proj")
        ));
    }

    #[test]
    #[cfg(windows)]
    fn cmd_on_an_ordinary_directory_is_left_alone() {
        // Control: the UNC shape is the other half. cmd is fine everywhere else.
        assert!(!cmd_shell_cannot_use_dir("cmd.exe", Path::new(r"C:\proj")));
    }

    #[test]
    #[cfg(windows)]
    fn the_error_names_the_directory_and_a_way_out() {
        let msg = unc_working_dir_error(Path::new(r"\\server\share\proj"));
        assert!(msg.contains(r"\\server\share\proj"), "{msg}");
        assert!(msg.contains("pwsh -c"), "{msg}");
        assert!(msg.contains("windows_default_inline_shell_args"), "{msg}");
    }
}
