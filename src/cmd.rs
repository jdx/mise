use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
#[cfg(panic = "abort")]
use std::sync::TryLockError;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::redactions::Redactor;
use color_eyre::Result;
use duct::{Expression, IntoExecutablePath};
use eyre::{Context, bail};
#[cfg(not(any(test, target_os = "windows")))]
use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
#[cfg(not(any(test, target_os = "windows")))]
use signal_hook::iterator::Signals;
use std::sync::LazyLock as Lazy;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::process::Command;

use crate::config::Settings;
use crate::config::env_directive::EnvValue;
use crate::env;
use crate::env::PATH_KEY;
use crate::errors::Error::ScriptFailed;
use crate::file::display_path;
use crate::path_env::PathEnv;
use crate::ui::progress_report::SingleReport;

/// Create a command with any number of of positional arguments
///
/// may be different types (anything that implements [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of arguments.
///
/// # Example
///
/// ```
///     use std::path::Path;
///     use mise::cmd;
///
///     let arg1 = "foo";
///     let arg2 = "bar".to_owned();
///     let arg3 = Path::new("baz");
///
///     let output = cmd!("echo", arg1, arg2, arg3).read();
///
///     assert_eq!("foo bar baz", output.unwrap());
/// ```
#[macro_export]
macro_rules! cmd {
    ( $program:expr $(, $arg:expr )* $(,)? ) => {
        {
            use std::ffi::OsString;
            let args: std::vec::Vec<OsString> = std::vec![$( Into::<OsString>::into($arg) ),*];
            $crate::cmd::cmd($program, args)
        }
    };
}

/// Create a command with any number of of positional arguments, which may be
/// different types (anything that implements
/// [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of
/// arguments.
///
/// # Example
///
/// ```
///     use std::path::Path;
///     use mise::cmd;
///
///     let arg1 = "foo";
///     let arg2 = "bar".to_owned();
///     let arg3 = Path::new("baz");
///
///     let output = cmd!("echo", arg1, arg2, arg3).read();
///
///     assert_eq!("foo bar baz", output.unwrap());
/// ```
pub fn cmd<T, U>(program: T, args: U) -> Expression
where
    T: IntoExecutablePath,
    U: IntoIterator,
    U::Item: Into<OsString>,
{
    let program = program.to_executable();
    let args: Vec<OsString> = args.into_iter().map(Into::<OsString>::into).collect();

    let display_command = std::iter::once(&program)
        .chain(&args)
        .map(|s| shell_escape::escape(s.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    debug!("$ {display_command}");

    duct::cmd(program, args)
}

type OutputObserver<'a> = Box<dyn Fn(&str) + Send + 'a>;

pub struct CmdLineRunner<'a> {
    cmd: Command,
    pr: Option<&'a dyn SingleReport>,
    pr_arc: Option<Arc<Box<dyn SingleReport>>>,
    stdin: Option<String>,
    redactor: Redactor,
    raw: bool,
    pass_signals: bool,
    on_stdout: Option<Box<dyn Fn(String) + Send + 'a>>,
    on_stderr: Option<Box<dyn Fn(String) + Send + 'a>>,
    observe_stdout: Option<OutputObserver<'a>>,
    observe_stderr: Option<OutputObserver<'a>>,
    timeout: Option<Duration>,
    sandbox: Option<crate::sandbox::SandboxConfig>,
}

const GUARD_RUNNING: u8 = 0;
const GUARD_CANCELLED: u8 = 1;
const GUARD_TIMED_OUT: u8 = 2;

#[cfg(unix)]
fn signal_process_tree(pid: u32, signal: nix::sys::signal::Signal) {
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    if !should_use_pgroup() || nix::sys::signal::killpg(pid, signal).is_err() {
        let _ = nix::sys::signal::kill(pid, signal);
    }
}

#[cfg(windows)]
fn kill_process_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/F", "/T", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn wait_for_cancel_or_deadline<'a>(
    cvar: &'a Condvar,
    mut guard: MutexGuard<'a, bool>,
    deadline: std::time::Instant,
) -> (MutexGuard<'a, bool>, bool) {
    loop {
        if *guard {
            return (guard, true);
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return (guard, false);
        }
        let (g, result) = cvar.wait_timeout(guard, remaining).unwrap();
        guard = g;
        if result.timed_out() {
            return (guard, false);
        }
    }
}

struct TimeoutGuard {
    state: Arc<AtomicU8>,
    cancel: Arc<(Mutex<bool>, Condvar)>,
    timeout: Duration,
}

impl TimeoutGuard {
    fn new(timeout: Duration, pid: u32) -> Self {
        let state = Arc::new(AtomicU8::new(GUARD_RUNNING));
        let cancel = Arc::new((Mutex::new(false), Condvar::new()));
        let state_clone = state.clone();
        let cancel_clone = cancel.clone();
        thread::spawn(move || {
            let (lock, cvar) = &*cancel_clone;
            let guard = lock.lock().unwrap();
            let deadline = std::time::Instant::now() + timeout;
            let (guard, cancelled) = wait_for_cancel_or_deadline(cvar, guard, deadline);
            if cancelled {
                return;
            }
            if state_clone
                .compare_exchange(
                    GUARD_RUNNING,
                    GUARD_TIMED_OUT,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return;
            }
            #[cfg(unix)]
            {
                signal_process_tree(pid, nix::sys::signal::Signal::SIGTERM);
                drop(guard);
                let guard = lock.lock().unwrap();
                let grace_deadline = std::time::Instant::now() + Duration::from_secs(5);
                let (_guard, cancelled) = wait_for_cancel_or_deadline(cvar, guard, grace_deadline);
                if !cancelled {
                    signal_process_tree(pid, nix::sys::signal::Signal::SIGKILL);
                }
            }
            #[cfg(windows)]
            {
                drop(guard);
                // TODO: Windows lacks graceful shutdown parity with Unix.
                // Currently force-kills immediately via taskkill /F with no grace period.
                // Consider using GenerateConsoleCtrlEvent for CTRL_C_EVENT before force kill.
                kill_process_tree(pid);
            }
        });
        Self {
            state,
            cancel,
            timeout,
        }
    }

    fn cancel(&self) {
        self.state
            .compare_exchange(
                GUARD_RUNNING,
                GUARD_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .ok();
        let (lock, cvar) = &*self.cancel;
        *lock.lock().unwrap() = true;
        cvar.notify_one();
    }

    fn timed_out(&self) -> Option<Duration> {
        (self.state.load(Ordering::Acquire) == GUARD_TIMED_OUT).then_some(self.timeout)
    }
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        self.cancel();
    }
}

static OUTPUT_LOCK: Mutex<()> = Mutex::new(());
static RAW_LOCK: Lazy<tokio::sync::RwLock<()>> = Lazy::new(|| tokio::sync::RwLock::new(()));

static RUNNING_PIDS: Lazy<Mutex<HashSet<u32>>> = Lazy::new(Default::default);

#[cfg(all(panic = "abort", unix))]
fn kill_pids_immediately(pids: &HashSet<u32>) {
    let use_pgroup = should_use_pgroup();
    for pid in pids {
        let pid = nix::unistd::Pid::from_raw(*pid as i32);
        if use_pgroup {
            if nix::sys::signal::killpg(pid, nix::sys::signal::SIGKILL).is_err() {
                let _ = nix::sys::signal::kill(pid, nix::sys::signal::SIGKILL);
            }
        } else {
            let _ = nix::sys::signal::kill(pid, nix::sys::signal::SIGKILL);
        }
    }
}

#[cfg(all(panic = "abort", windows))]
fn kill_pids_immediately(pids: &HashSet<u32>) {
    for pid in pids {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Best-effort synchronous cleanup for the panic hook.
///
/// An aborting panic does not run destructors, and there is no time for the
/// normal TERM/grace-period/KILL sequence. Avoid blocking if the panic occurred
/// while the PID registry was locked; a deadlocked panic hook would prevent the
/// process from ever reaching abort.
#[cfg(panic = "abort")]
pub fn kill_all_on_panic() {
    let pids = match RUNNING_PIDS.try_lock() {
        Ok(pids) => pids,
        Err(TryLockError::Poisoned(err)) => err.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    kill_pids_immediately(&pids);
}

pub(crate) struct RunningPidGuard(Option<u32>);

impl RunningPidGuard {
    pub(crate) fn new(pid: Option<u32>) -> Self {
        if let Some(pid) = pid {
            RUNNING_PIDS.lock().unwrap().insert(pid);
        }
        Self(pid)
    }
}

impl Drop for RunningPidGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            RUNNING_PIDS.lock().unwrap().remove(&pid);
        }
    }
}

/// Env var set on every spawned child when this mise process is managing
/// process groups (calling setpgid + killpg). A nested mise that sees this
/// var skips its own setpgid so descendants stay in the outer pgid — that
/// way the outer mise's killpg actually reaches the leaves.
#[cfg(unix)]
const TASK_PGID_MANAGED_ENV: &str = "MISE_TASK_PGID_MANAGED";

/// True when this mise should `setpgid` spawned children and `killpg` them
/// for cleanup.
///
/// We skip pgroup management in two cases:
///
/// 1. **Nested under another mise** (env var present). The outer mise is
///    already managing pgroups; if we set our own, the outer's `killpg`
///    can't reach our descendants and either an orchestrator or the
///    user's Ctrl+C leaves orphans behind.
/// 2. **We're the session leader** — i.e. `getsid(0) == getpid()`. This
///    is what Node's `detached: true` (Playwright's `webServer`) does:
///    it calls `setsid` so the orchestrator can `kill(-pgid, SIGKILL)`
///    the whole tree later. If we then create our own pgroups, the
///    orchestrator's tree-kill stops at us and our descendants survive,
///    holding pipes open and hanging the parent.
///
/// In both cases we share whatever pgid we landed in, so the ancestor
/// that owns it can clean us up.
///
/// Cached on first access: `execute()` decides whether to `setpgid` at
/// spawn time, and `kill_all()` decides whether to `killpg` at signal
/// time. They must agree — a child placed in its own pgid by `execute()`
/// must be killed via `killpg`, or only the direct PID gets the signal
/// and grandchildren leak. Computing this once removes any chance of the
/// two callers disagreeing if the env later mutates.
#[cfg(unix)]
fn should_use_pgroup() -> bool {
    static CACHED: Lazy<bool> = Lazy::new(|| {
        if std::env::var_os(TASK_PGID_MANAGED_ENV).is_some() {
            return false;
        }
        let me = nix::unistd::getpid();
        if let Ok(sid) = nix::unistd::getsid(None)
            && sid == me
        {
            return false;
        }
        true
    });
    *CACHED
}

/// Put a non-interactive child in the process tree managed by mise.
///
/// Callers must retain a [`RunningPidGuard`] after spawning the command.
pub(crate) fn prepare_noninteractive_child(_cmd: &mut std::process::Command) {
    #[cfg(unix)]
    if should_use_pgroup() {
        _cmd.env(TASK_PGID_MANAGED_ENV, "1");
        unsafe {
            _cmd.pre_exec(|| {
                let _ = nix::unistd::setpgid(
                    nix::unistd::Pid::from_raw(0),
                    nix::unistd::Pid::from_raw(0),
                );
                Ok(())
            });
        }
    }
}

/// Grace period after a child's ExitStatus arrives during which we keep
/// reading its stdout/stderr pipes. If a grandchild inherited the pipes
/// and survived (e.g. a nested mise that escaped our pgroup, or an
/// orchestrator's SIGKILL leaving orphans), the readers would otherwise
/// block forever waiting for EOF and the parent would hang. After this
/// deadline we abandon the readers — any tail output is dropped.
const PIPE_DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

enum HashedProcessOutput {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    ReadError(&'static str, std::io::Error),
}

impl<'a> CmdLineRunner<'a> {
    pub fn new<P: AsRef<OsStr>>(program: P) -> Self {
        let mut cmd = Command::new(program);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        Self {
            cmd,
            pr: None,
            pr_arc: None,
            stdin: None,
            redactor: Default::default(),
            raw: false,
            pass_signals: false,
            on_stdout: None,
            on_stderr: None,
            observe_stdout: None,
            observe_stderr: None,
            timeout: None,
            sandbox: None,
        }
    }

    pub fn with_sandbox(mut self, sandbox: crate::sandbox::SandboxConfig) -> Self {
        if sandbox.is_active() {
            self.sandbox = Some(sandbox);
        }
        self
    }

    #[cfg(unix)]
    pub fn kill_all(signal: nix::sys::signal::Signal) {
        let use_pgroup = should_use_pgroup();
        let pids = RUNNING_PIDS.lock().unwrap();
        for pid in pids.iter() {
            let pid = *pid as i32;
            let nix_pid = nix::unistd::Pid::from_raw(pid);
            if use_pgroup {
                trace!("{signal}: pgid {pid}");
                // Each tracked PID is also the leader of its own pgid (set
                // via setpgid(0,0) in pre_exec), so killpg targets the whole
                // descendant tree. Fall back to plain kill for the rare case
                // where setpgid was skipped (TTY stdin) — still better than
                // silently dropping the signal.
                if nix::sys::signal::killpg(nix_pid, signal).is_err()
                    && let Err(e) = nix::sys::signal::kill(nix_pid, signal)
                {
                    debug!("Failed to kill cmd {pid}: {e}");
                }
            } else {
                trace!("{signal}: {pid}");
                if let Err(e) = nix::sys::signal::kill(nix_pid, signal) {
                    debug!("Failed to kill cmd {pid}: {e}");
                }
            }
        }
    }

    #[cfg(windows)]
    pub fn kill_all() {
        let pids = RUNNING_PIDS.lock().unwrap();
        for pid in pids.iter() {
            if let Err(e) = std::process::Command::new("taskkill")
                .arg("/F")
                .arg("/T")
                .arg("/PID")
                .arg(pid.to_string())
                .spawn()
            {
                warn!("Failed to kill cmd {pid}: {e}");
            }
        }
    }

    pub fn stdin<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stdin(cfg);
        self
    }

    pub fn stdout<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.cmd.stderr(cfg);
        self
    }

    pub fn redact(mut self, redactions: impl IntoIterator<Item = String>) -> Self {
        self.redactor = self.redactor.with_additional(redactions);
        self
    }

    pub fn with_on_stdout<F: Fn(String) + Send + 'a>(mut self, on_stdout: F) -> Self {
        self.on_stdout = Some(Box::new(on_stdout));
        self
    }

    pub fn with_on_stderr<F: Fn(String) + Send + 'a>(mut self, on_stderr: F) -> Self {
        self.on_stderr = Some(Box::new(on_stderr));
        self
    }

    pub(crate) fn with_stdout_observer<F: Fn(&str) + Send + 'a>(mut self, observer: F) -> Self {
        self.observe_stdout = Some(Box::new(observer));
        self
    }

    pub(crate) fn with_stderr_observer<F: Fn(&str) + Send + 'a>(mut self, observer: F) -> Self {
        self.observe_stderr = Some(Box::new(observer));
        self
    }

    pub fn current_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    pub fn env_clear(mut self) -> Self {
        self.cmd.env_clear();
        self
    }

    pub fn env<K, V>(mut self, key: K, val: V) -> Self
    where
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.env(key, val);
        self
    }

    pub fn env_remove<K>(mut self, key: K) -> Self
    where
        K: AsRef<OsStr>,
    {
        self.cmd.env_remove(key);
        self
    }

    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        self.cmd.envs(vars);
        self
    }

    pub fn env_values<I, K>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, EnvValue)>,
        K: AsRef<OsStr>,
    {
        for (key, value) in vars {
            match value.into_string() {
                Some(value) => self.cmd.env(key, value),
                None => self.cmd.env_remove(key),
            };
        }
        self
    }

    pub fn prepend_path(mut self, paths: Vec<PathBuf>) -> eyre::Result<Self> {
        let existing = self
            .get_env(&PATH_KEY)
            .map(|c| c.to_owned())
            .unwrap_or_else(|| env::var_os(&*PATH_KEY).unwrap());
        let mut path_env = PathEnv::from_iter(env::split_paths(&existing));
        for p in paths {
            path_env.add(p);
        }
        self.cmd.env(&*PATH_KEY, path_env.join());
        Ok(self)
    }

    fn get_env(&self, key: &str) -> Option<&OsStr> {
        for (k, v) in self.cmd.as_std().get_envs() {
            if k == key {
                return v;
            }
        }
        None
    }

    pub fn opt_args<S: AsRef<OsStr>>(mut self, arg: &str, values: Option<Vec<S>>) -> Self {
        if let Some(values) = values {
            for value in values {
                self.cmd.arg(arg);
                self.cmd.arg(value);
            }
        }
        self
    }

    pub fn arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        self.cmd.arg(arg.as_ref());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.cmd.args(args);
        self
    }

    /// Append a shell's `flags` followed by an inline command `body`. On Windows,
    /// when this runner's program is `cmd[.exe]` and `flags` contain `/c`|`/k`,
    /// the body is handed to cmd *verbatim* (raw args, one outer quote pair via
    /// [`crate::path::cmd_verbatim_args`]) so inner double quotes survive — the
    /// same fix the inline-task path uses. Otherwise (Unix, or a non-cmd Windows
    /// shell) this is exactly `self.args(flags).arg(body)`. See #9355.
    pub fn cmd_body_args(self, flags: &[String], body: &str) -> Self {
        #[cfg(windows)]
        {
            let program = std::path::PathBuf::from(self.cmd.as_std().get_program());
            let runs_command = flags
                .iter()
                .any(|f| f.eq_ignore_ascii_case("/c") || f.eq_ignore_ascii_case("/k"));
            if crate::path::is_cmd_shell_program(&program) && runs_command {
                let cmd_args = crate::path::cmd_verbatim_args(flags, body, &[]);
                return cmd_args.into_iter().fold(self, |r, a| r.raw_arg(a));
            }
        }
        self.args(flags).arg(body)
    }

    /// Append a single argument to the command line *verbatim*, bypassing the
    /// MSVCRT-style quoting std normally applies on Windows. Required when
    /// spawning `cmd.exe /c <script>`: cmd does not understand the `\"`
    /// escaping std would otherwise emit for inner double quotes, so the script
    /// must reach cmd unquoted. See `TaskExecutor::get_cmd_program_and_args`
    /// and discussion #9355.
    #[cfg(windows)]
    pub fn raw_arg<S: AsRef<OsStr>>(mut self, arg: S) -> Self {
        // tokio's `Command` exposes `raw_arg` as an inherent method, so the
        // `std::os::windows::process::CommandExt` trait import is unnecessary.
        self.cmd.raw_arg(arg);
        self
    }

    pub fn with_pr(mut self, pr: &'a dyn SingleReport) -> Self {
        self.pr = Some(pr);
        self
    }
    pub fn with_pr_arc(mut self, pr: Arc<Box<dyn SingleReport>>) -> Self {
        self.pr_arc = Some(pr);
        self
    }
    pub fn raw(mut self, raw: bool) -> Self {
        self.raw = raw;
        self
    }

    pub fn with_pass_signals(&mut self) -> &mut Self {
        self.pass_signals = true;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn stdin_string(mut self, input: impl Into<String>) -> Self {
        self.cmd.stdin(Stdio::piped());
        self.stdin = Some(input.into());
        self
    }

    pub fn execute(mut self) -> Result<()> {
        let read_lock = raw_read_lock_blocking();
        debug!("$ {self}");
        if Settings::get().raw || self.raw {
            drop(read_lock);
            let _write_lock = raw_write_lock_blocking();
            return self.execute_raw();
        }
        #[cfg(unix)]
        if should_use_pgroup() {
            // Mark descendants so a nested mise inherits this var and skips
            // its own setpgid — keeping the whole tree in our pgid.
            self.cmd.env(TASK_PGID_MANAGED_ENV, "1");
            unsafe {
                self.cmd.as_std_mut().pre_exec(|| {
                    // Skip setpgid when stdin is a TTY: interactive tools
                    // (e.g. Tilt) need to stay in the terminal's foreground
                    // pgid or they hang on SIGTTIN when reading stdin.
                    // Use BorrowedFd::borrow_raw rather than std::io::stdin()
                    // — pre_exec runs post-fork where OnceLock/malloc are
                    // not async-signal-safe.
                    let stdin = std::os::fd::BorrowedFd::borrow_raw(0);
                    if !std::io::IsTerminal::is_terminal(&stdin) {
                        let _ = nix::unistd::setpgid(
                            nix::unistd::Pid::from_raw(0),
                            nix::unistd::Pid::from_raw(0),
                        );
                    }
                    Ok(())
                });
            }
        }
        let mut cp = self
            .spawn_with_etxtbsy_retry()
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id();
        RUNNING_PIDS.lock().unwrap().insert(id);
        trace!("Started process: {id} for {}", self.get_program());
        let (tx, rx) = channel();
        if let Some(stdout) = cp.stdout.take() {
            thread::spawn({
                let name = self.to_string();
                let tx = tx.clone();
                move || {
                    for line in BufReader::new(stdout).lines() {
                        match line {
                            Ok(line) => {
                                let _ = tx.send(ChildProcessOutput::Stdout(line));
                            }
                            Err(e) => warn!("Failed to read stdout for {name}: {e}"),
                        }
                    }
                }
            });
        }
        if let Some(stderr) = cp.stderr.take() {
            thread::spawn({
                let name = self.to_string();
                let tx = tx.clone();
                move || {
                    for line in BufReader::new(stderr).lines() {
                        match line {
                            Ok(line) => {
                                let _ = tx.send(ChildProcessOutput::Stderr(line));
                            }
                            Err(e) => warn!("Failed to read stderr for {name}: {e}"),
                        }
                    }
                }
            });
        }
        if let Some(text) = self.stdin.take() {
            let mut stdin = cp.stdin.take().unwrap();
            thread::spawn(move || {
                stdin.write_all(text.as_bytes()).unwrap();
            });
        }
        #[cfg(not(any(test, target_os = "windows")))]
        let mut sighandle = None;
        #[cfg(not(any(test, target_os = "windows")))]
        if self.pass_signals {
            let mut signals =
                Signals::new([SIGINT, SIGTERM, SIGTERM, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2])?;
            sighandle = Some(signals.handle());
            let tx = tx.clone();
            thread::spawn(move || {
                for sig in &mut signals {
                    let _ = tx.send(ChildProcessOutput::Signal(sig));
                }
            });
        }
        thread::spawn(move || {
            let status = cp.wait().unwrap();
            #[cfg(not(any(test, target_os = "windows")))]
            if let Some(sighandle) = sighandle {
                sighandle.close();
            }
            let _ = tx.send(ChildProcessOutput::ExitStatus(status));
        });

        let timeout_guard = self.timeout.map(|t| TimeoutGuard::new(t, id));

        let mut combined_output = vec![];
        let mut status = None;
        // Once ExitStatus arrives we set a deadline and switch to recv_timeout
        // so a grandchild that inherited the pipes can't hang us forever
        // waiting for EOF. See PIPE_DRAIN_TIMEOUT.
        let mut drain_deadline: Option<Instant> = None;
        loop {
            let msg = match drain_deadline {
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        debug!("pipe drain timeout for {id}, abandoning readers");
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(m) => m,
                        Err(RecvTimeoutError::Timeout) => {
                            debug!("pipe drain timeout for {id}, abandoning readers");
                            break;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
                None => match rx.recv() {
                    Ok(m) => m,
                    Err(_) => break,
                },
            };
            match msg {
                ChildProcessOutput::Stdout(line) => {
                    let line = self.redactor.redact(&line);
                    self.on_stdout(line.clone());
                    combined_output.push((line, OutputSource::Stdout));
                }
                ChildProcessOutput::Stderr(line) => {
                    let line = self.redactor.redact(&line);
                    self.on_stderr(line.clone());
                    combined_output.push((line, OutputSource::Stderr));
                }
                ChildProcessOutput::ExitStatus(s) => {
                    status = Some(s);
                    drain_deadline = Some(Instant::now() + PIPE_DRAIN_TIMEOUT);
                }
                #[cfg(not(any(test, windows)))]
                ChildProcessOutput::Signal(sig) => {
                    let pid = nix::unistd::Pid::from_raw(id as i32);
                    let nix_sig = nix::sys::signal::Signal::try_from(sig).unwrap();
                    if should_use_pgroup() {
                        // With pgroups the child is isolated from the
                        // terminal's foreground pgid, so terminal SIGINT
                        // doesn't reach it — forward every signal we
                        // catch, including SIGINT.
                        debug!("Received signal {sig}, forwarding to pgid {id}");
                        if nix::sys::signal::killpg(pid, nix_sig).is_err() {
                            let _ = nix::sys::signal::kill(pid, nix_sig);
                        }
                    } else if sig != SIGINT {
                        // No pgroup: the child is in our pgid, so the
                        // terminal already delivered SIGINT. Forwarding
                        // it again would just be a redundant kill.
                        debug!("Received signal {sig}, forwarding to {id}");
                        let _ = nix::sys::signal::kill(pid, nix_sig);
                    }
                }
            }
        }
        // Removed after rx loop drains (not inside ExitStatus arm) so kill_all
        // can still reach this PID while output is being processed.
        RUNNING_PIDS.lock().unwrap().remove(&id);
        if let Some(g) = &timeout_guard {
            g.cancel();
        }

        let status = status.unwrap();

        if !status.success() {
            if let Some(duration) = timeout_guard.as_ref().and_then(|g| g.timed_out()) {
                bail!("timed out after {duration:?}");
            }
            self.on_error(combined_output, status)?;
        }

        Ok(())
    }

    pub async fn execute_async(self) -> Result<()> {
        self.execute_async_with_cancel_check(|| false).await
    }

    /// Execute a command while preventing cancellation from being lost between
    /// the pre-spawn check and PID registration.
    pub async fn execute_async_with_cancel_check(
        mut self,
        is_cancelled: impl Fn() -> bool + Send + Sync,
    ) -> Result<()> {
        if is_cancelled() {
            return Err(crate::errors::Error::TaskInterrupted.into());
        }
        let read_lock = RAW_LOCK.read().await;
        debug!("$ {self}");
        if Settings::get().raw || self.raw {
            drop(read_lock);
            let _write_lock = RAW_LOCK.write().await;
            return self.execute_raw_async_with_cancel_check(is_cancelled).await;
        }
        #[cfg(unix)]
        if should_use_pgroup() {
            self.cmd.env(TASK_PGID_MANAGED_ENV, "1");
            unsafe {
                self.cmd.as_std_mut().pre_exec(|| {
                    let stdin = std::os::fd::BorrowedFd::borrow_raw(0);
                    if !std::io::IsTerminal::is_terminal(&stdin) {
                        let _ = nix::unistd::setpgid(
                            nix::unistd::Pid::from_raw(0),
                            nix::unistd::Pid::from_raw(0),
                        );
                    }
                    Ok(())
                });
            }
        }
        let mut cp = self
            .spawn_async_with_etxtbsy_retry()
            .await
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id().unwrap_or_default();
        RUNNING_PIDS.lock().unwrap().insert(id);
        if is_cancelled() {
            #[cfg(unix)]
            signal_process_tree(id, nix::sys::signal::SIGINT);
            #[cfg(windows)]
            kill_process_tree(id);
        }
        trace!("Started process: {id} for {}", self.get_program());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        if let Some(stdout) = cp.stdout.take() {
            let name = self.to_string();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = TokioBufReader::new(stdout).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let _ = tx.send(ChildProcessOutput::Stdout(line));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            warn!("Failed to read stdout for {name}: {e}");
                            break;
                        }
                    }
                }
            });
        }
        if let Some(stderr) = cp.stderr.take() {
            let name = self.to_string();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = TokioBufReader::new(stderr).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => {
                            let _ = tx.send(ChildProcessOutput::Stderr(line));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            warn!("Failed to read stderr for {name}: {e}");
                            break;
                        }
                    }
                }
            });
        }
        if let Some(text) = self.stdin.take()
            && let Some(mut stdin) = cp.stdin.take()
        {
            tokio::spawn(async move {
                let _ = stdin.write_all(text.as_bytes()).await;
            });
        }
        #[cfg(not(any(test, target_os = "windows")))]
        let mut sighandle = None;
        #[cfg(not(any(test, target_os = "windows")))]
        if self.pass_signals {
            let mut signals =
                Signals::new([SIGINT, SIGTERM, SIGTERM, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2])?;
            sighandle = Some(signals.handle());
            let tx = tx.clone();
            thread::spawn(move || {
                for sig in &mut signals {
                    let _ = tx.send(ChildProcessOutput::Signal(sig));
                }
            });
        }
        drop(tx);

        let timeout_guard = self.timeout.map(|t| TimeoutGuard::new(t, id));
        let mut combined_output = vec![];
        let mut status = None;
        let mut wait = Box::pin(cp.wait());
        loop {
            tokio::select! {
                result = &mut wait, if status.is_none() => {
                    #[cfg(not(any(test, target_os = "windows")))]
                    if let Some(sighandle) = sighandle.take() {
                        sighandle.close();
                    }
                    status = Some(result?);
                    break;
                }
                msg = rx.recv() => {
                    let Some(msg) = msg else {
                        if status.is_none() {
                            #[cfg(not(any(test, target_os = "windows")))]
                            if let Some(sighandle) = sighandle.take() {
                                sighandle.close();
                            }
                            status = Some(wait.await?);
                        }
                        break;
                    };
                    match msg {
                        ChildProcessOutput::Stdout(line) => {
                            let line = self.redactor.redact(&line);
                            self.on_stdout(line.clone());
                            combined_output.push((line, OutputSource::Stdout));
                        }
                        ChildProcessOutput::Stderr(line) => {
                            let line = self.redactor.redact(&line);
                            self.on_stderr(line.clone());
                            combined_output.push((line, OutputSource::Stderr));
                        }
                        ChildProcessOutput::ExitStatus(_) => {}
                        #[cfg(not(any(test, windows)))]
                        ChildProcessOutput::Signal(sig) => {
                            let pid = nix::unistd::Pid::from_raw(id as i32);
                            let nix_sig = nix::sys::signal::Signal::try_from(sig).unwrap();
                            if should_use_pgroup() {
                                debug!("Received signal {sig}, forwarding to pgid {id}");
                                if nix::sys::signal::killpg(pid, nix_sig).is_err() {
                                    let _ = nix::sys::signal::kill(pid, nix_sig);
                                }
                            } else if sig != SIGINT {
                                debug!("Received signal {sig}, forwarding to {id}");
                                let _ = nix::sys::signal::kill(pid, nix_sig);
                            }
                        }
                    }
                }
            }
        }
        let drain_deadline = Instant::now() + PIPE_DRAIN_TIMEOUT;
        loop {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                debug!("pipe drain timeout for {id}, abandoning readers");
                break;
            }
            let msg = match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(_) => {
                    debug!("pipe drain timeout for {id}, abandoning readers");
                    break;
                }
            };
            match msg {
                ChildProcessOutput::Stdout(line) => {
                    let line = self.redactor.redact(&line);
                    self.on_stdout(line.clone());
                    combined_output.push((line, OutputSource::Stdout));
                }
                ChildProcessOutput::Stderr(line) => {
                    let line = self.redactor.redact(&line);
                    self.on_stderr(line.clone());
                    combined_output.push((line, OutputSource::Stderr));
                }
                ChildProcessOutput::ExitStatus(_) => {}
                #[cfg(not(any(test, windows)))]
                ChildProcessOutput::Signal(_) => {}
            }
        }
        RUNNING_PIDS.lock().unwrap().remove(&id);
        if let Some(g) = &timeout_guard {
            g.cancel();
        }

        let status = status.unwrap();
        if !status.success() {
            if let Some(duration) = timeout_guard.as_ref().and_then(|g| g.timed_out()) {
                bail!("timed out after {duration:?}");
            }
            self.on_error(combined_output, status)?;
        }

        Ok(())
    }

    /// Run a command while incrementally hashing its raw stdout and stderr.
    ///
    /// Unlike `read`, this never buffers the complete output in memory. The
    /// combined byte limit also prevents commands that emit indefinitely from
    /// consuming unbounded resources.
    pub async fn execute_hashes_async(self, max_output_bytes: usize) -> Result<(String, String)> {
        self.execute_hashes_async_with_drain_timeout(max_output_bytes, PIPE_DRAIN_TIMEOUT)
            .await
    }

    async fn execute_hashes_async_with_drain_timeout(
        mut self,
        max_output_bytes: usize,
        pipe_drain_timeout: Duration,
    ) -> Result<(String, String)> {
        let _read_lock = RAW_LOCK.read().await;
        debug!("$ {self}");
        self.cmd.kill_on_drop(true);
        // These commands are non-interactive probes: nothing reads stdin and
        // both output streams are piped. Detaching stdin from the terminal
        // means the child can never need the controlling TTY, so unlike
        // `execute()` we can always create a dedicated process group without
        // risking SIGTTIN. That guarantee matters here — cleanup on timeout,
        // an output-limit breach, or a stuck pipe relies on `killpg` reaching
        // descendants, not just the direct child.
        self.cmd.stdin(Stdio::null());
        #[cfg(unix)]
        if should_use_pgroup() {
            self.cmd.env(TASK_PGID_MANAGED_ENV, "1");
            unsafe {
                self.cmd.as_std_mut().pre_exec(|| {
                    let _ = nix::unistd::setpgid(
                        nix::unistd::Pid::from_raw(0),
                        nix::unistd::Pid::from_raw(0),
                    );
                    Ok(())
                });
            }
        }
        let mut cp = self
            .spawn_async_with_etxtbsy_retry()
            .await
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id().unwrap_or_default();
        let _running_pid = RunningPidGuard::new(cp.id());
        trace!("Started process: {id} for {}", self.get_program());

        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        if let Some(mut stdout) = cp.stdout.take() {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0; 8192];
                loop {
                    match stdout.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(len) => {
                            if tx
                                .send(HashedProcessOutput::Stdout(buffer[..len].to_vec()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(HashedProcessOutput::ReadError("stdout", err)).await;
                            break;
                        }
                    }
                }
            });
        }
        if let Some(mut stderr) = cp.stderr.take() {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0; 8192];
                loop {
                    match stderr.read(&mut buffer).await {
                        Ok(0) => break,
                        Ok(len) => {
                            if tx
                                .send(HashedProcessOutput::Stderr(buffer[..len].to_vec()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(err) => {
                            let _ = tx.send(HashedProcessOutput::ReadError("stderr", err)).await;
                            break;
                        }
                    }
                }
            });
        }
        drop(tx);

        let timeout_guard = self.timeout.map(|timeout| TimeoutGuard::new(timeout, id));
        let mut stdout_hasher = blake3::Hasher::new();
        let mut stderr_hasher = blake3::Hasher::new();
        let mut output_bytes = 0usize;
        let mut consume = |output: HashedProcessOutput| -> Result<()> {
            match output {
                HashedProcessOutput::Stdout(bytes) => {
                    output_bytes = output_bytes.saturating_add(bytes.len());
                    if output_bytes > max_output_bytes {
                        bail!("command output exceeded {max_output_bytes} bytes");
                    }
                    stdout_hasher.update(&bytes);
                }
                HashedProcessOutput::Stderr(bytes) => {
                    output_bytes = output_bytes.saturating_add(bytes.len());
                    if output_bytes > max_output_bytes {
                        bail!("command output exceeded {max_output_bytes} bytes");
                    }
                    stderr_hasher.update(&bytes);
                }
                HashedProcessOutput::ReadError(stream, err) => {
                    bail!("failed to read command {stream}: {err}");
                }
            }
            Ok(())
        };
        let mut status = None;
        let mut wait = Box::pin(cp.wait());
        loop {
            tokio::select! {
                result = &mut wait, if status.is_none() => {
                    status = Some(result?);
                    break;
                }
                output = rx.recv() => {
                    let Some(output) = output else {
                        status = Some(wait.await?);
                        break;
                    };
                    if let Err(err) = consume(output) {
                        #[cfg(unix)]
                        signal_process_tree(id, nix::sys::signal::Signal::SIGKILL);
                        #[cfg(windows)]
                        kill_process_tree(id);
                        let _ = wait.await;
                        return Err(err);
                    }
                }
            }
        }
        let drain_deadline = Instant::now() + pipe_drain_timeout;
        loop {
            let remaining = drain_deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                #[cfg(unix)]
                signal_process_tree(id, nix::sys::signal::Signal::SIGKILL);
                #[cfg(windows)]
                kill_process_tree(id);
                bail!("command output pipes did not close within {pipe_drain_timeout:?}");
            }
            let output = match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Some(output)) => output,
                Ok(None) => break,
                Err(_) => {
                    #[cfg(unix)]
                    signal_process_tree(id, nix::sys::signal::Signal::SIGKILL);
                    #[cfg(windows)]
                    kill_process_tree(id);
                    bail!("command output pipes did not close within {pipe_drain_timeout:?}");
                }
            };
            if let Err(err) = consume(output) {
                #[cfg(unix)]
                signal_process_tree(id, nix::sys::signal::Signal::SIGKILL);
                #[cfg(windows)]
                kill_process_tree(id);
                return Err(err);
            }
        }

        if let Some(guard) = &timeout_guard {
            guard.cancel();
        }
        let status = status.expect("command wait must complete");
        if !status.success() {
            if let Some(timeout) = timeout_guard.as_ref().and_then(|guard| guard.timed_out()) {
                bail!("timed out after {timeout:?}");
            }
            bail!("exited with non-zero status: {status}");
        }
        Ok((
            stdout_hasher.finalize().to_hex().to_string(),
            stderr_hasher.finalize().to_hex().to_string(),
        ))
    }

    /// Run the command and return stdout, even when raw mode is enabled.
    pub async fn read(mut self) -> Result<String> {
        let _read_lock = RAW_LOCK.read().await;
        debug!("$ {self}");
        self.cmd.kill_on_drop(true);
        #[cfg(unix)]
        if should_use_pgroup() {
            self.cmd.env(TASK_PGID_MANAGED_ENV, "1");
            unsafe {
                self.cmd.as_std_mut().pre_exec(|| {
                    let stdin = std::os::fd::BorrowedFd::borrow_raw(0);
                    if !std::io::IsTerminal::is_terminal(&stdin) {
                        let _ = nix::unistd::setpgid(
                            nix::unistd::Pid::from_raw(0),
                            nix::unistd::Pid::from_raw(0),
                        );
                    }
                    Ok(())
                });
            }
        }
        let mut cp = self
            .spawn_async_with_etxtbsy_retry()
            .await
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id();
        let _running_pid = RunningPidGuard::new(id);
        trace!(
            "Started process: {} for {}",
            id.unwrap_or_default(),
            self.get_program()
        );
        if let Some(text) = self.stdin.take()
            && let Some(mut stdin) = cp.stdin.take()
        {
            tokio::spawn(async move {
                let _ = stdin.write_all(text.as_bytes()).await;
            });
        }

        let wait = cp.wait_with_output();
        let output = match self.timeout {
            Some(timeout) => match tokio::time::timeout(timeout, wait).await {
                Ok(output) => output?,
                Err(_) => bail!("timed out after {timeout:?}"),
            },
            None => wait.await?,
        };

        if !output.status.success() {
            let combined_output = captured_output_lines(&self, &output);
            self.replay_captured_stderr(&combined_output);
            self.on_error(combined_output, output.status)?;
        }

        let stdout = String::from_utf8(output.stdout)
            .wrap_err_with(|| format!("{} produced invalid UTF-8 output", self.get_program()))?;
        Ok(stdout.trim_end().to_string())
    }

    fn execute_raw(mut self) -> Result<()> {
        // In raw mode, inherit stdio so the child can interact with the terminal
        // directly. Piped stdout/stderr would deadlock if the child produces >64KB
        // of output since nobody reads the pipes.
        if self.stdin.is_none() {
            self.cmd.stdin(Stdio::inherit());
        }
        self.cmd.stdout(Stdio::inherit());
        self.cmd.stderr(Stdio::inherit());
        let mut cp = self.spawn_with_etxtbsy_retry()?;
        let timeout_guard = self.timeout.map(|t| TimeoutGuard::new(t, cp.id()));
        let status = cp.wait()?;
        if let Some(g) = &timeout_guard {
            g.cancel();
        }
        if !status.success() {
            if let Some(duration) = timeout_guard.as_ref().and_then(|g| g.timed_out()) {
                bail!("timed out after {duration:?}");
            }
            return self.on_error(vec![], status);
        }
        Ok(())
    }

    async fn execute_raw_async_with_cancel_check(
        mut self,
        is_cancelled: impl Fn() -> bool + Send + Sync,
    ) -> Result<()> {
        if self.stdin.is_none() {
            self.cmd.stdin(Stdio::inherit());
        }
        self.cmd.stdout(Stdio::inherit());
        self.cmd.stderr(Stdio::inherit());
        let mut cp = self.spawn_async_with_etxtbsy_retry().await?;
        let id = cp.id().unwrap_or_default();
        if is_cancelled() {
            #[cfg(unix)]
            signal_process_tree(id, nix::sys::signal::SIGINT);
            #[cfg(windows)]
            kill_process_tree(id);
        }
        let timeout_guard = self.timeout.map(|t| TimeoutGuard::new(t, id));
        let status = cp.wait().await?;
        if let Some(g) = &timeout_guard {
            g.cancel();
        }
        if !status.success() {
            if let Some(duration) = timeout_guard.as_ref().and_then(|g| g.timed_out()) {
                bail!("timed out after {duration:?}");
            }
            return self.on_error(vec![], status);
        }
        Ok(())
    }

    /// Retry spawning a process if it fails with ETXTBSY (Text file busy).
    /// This can happen on Linux when executing a binary that was just written/extracted,
    /// as the file descriptor may not be fully closed yet.
    fn spawn_with_etxtbsy_retry(&mut self) -> std::io::Result<std::process::Child> {
        let mut attempt = 0;
        loop {
            match self.cmd.as_std_mut().spawn() {
                Ok(child) => return Ok(child),
                Err(err) if Self::is_etxtbsy(&err) && attempt < 3 => {
                    attempt += 1;
                    trace!("retrying spawn after ETXTBSY (attempt {}/3)", attempt);
                    // Exponential backoff: 50ms, 100ms, 200ms
                    std::thread::sleep(std::time::Duration::from_millis(50 * (1 << (attempt - 1))));
                }
                Err(err) => return Err(err),
            }
        }
    }

    async fn spawn_async_with_etxtbsy_retry(&mut self) -> std::io::Result<tokio::process::Child> {
        let mut attempt = 0;
        loop {
            match self.cmd.spawn() {
                Ok(child) => return Ok(child),
                Err(err) if Self::is_etxtbsy(&err) && attempt < 3 => {
                    attempt += 1;
                    trace!("retrying spawn after ETXTBSY (attempt {}/3)", attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(50 * (1 << (attempt - 1))))
                        .await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Prepare sandbox restrictions on the command. Must be called before execute()
    /// when sandbox is configured. This is async because macOS DNS resolution is async.
    pub async fn apply_sandbox(&mut self) -> eyre::Result<()> {
        let Some(sandbox) = self.sandbox.take() else {
            return Ok(());
        };
        if !sandbox.is_active() {
            return Ok(());
        }

        // Fail early on Linux if per-host network filtering is requested
        #[cfg(target_os = "linux")]
        if !sandbox.allow_net.is_empty() {
            eyre::bail!(
                "per-host network filtering (--allow-net=<host>) is not supported on Linux. \
                 Use --deny-net to block all network, or remove --allow-net."
            );
        }

        #[cfg(target_os = "linux")]
        {
            // On Linux, clear inherited env before pre_exec so child only sees filtered vars.
            // env_clear() also wipes envs explicitly set via .envs(), so save and restore them.
            if sandbox.effective_deny_env() {
                let saved: Vec<(std::ffi::OsString, std::ffi::OsString)> = self
                    .cmd
                    .as_std()
                    .get_envs()
                    .filter_map(|(k, v)| v.map(|v| (k.to_os_string(), v.to_os_string())))
                    .collect();
                self.cmd.env_clear();
                for (k, v) in saved {
                    self.cmd.env(k, v);
                }
            }
            // Use pre_exec to apply Landlock/seccomp in the child process
            // before it execs the target program. This avoids restricting the mise process.
            let sandbox = sandbox.clone();
            unsafe {
                self.cmd.as_std_mut().pre_exec(move || {
                    if sandbox.effective_deny_read() || sandbox.effective_deny_write() {
                        crate::sandbox::landlock_apply(&sandbox)
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                    }
                    if sandbox.effective_deny_net() {
                        crate::sandbox::seccomp_apply()
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                    }
                    Ok(())
                });
            }
        }

        #[cfg(target_os = "macos")]
        {
            // On macOS, rewrite the command to go through sandbox-exec.
            // Build a new Command that wraps the original through sandbox-exec,
            // preserving stdio, cwd, and env from the original.
            let program = self.cmd.as_std().get_program().to_os_string();
            let args: Vec<String> = self
                .cmd
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let profile = crate::sandbox::macos_generate_profile(&sandbox).await;

            let mut new_cmd = Command::new("sandbox-exec");
            new_cmd.arg("-p").arg(&profile).arg("--").arg(&program);
            for arg in &args {
                new_cmd.arg(arg);
            }
            // Match CmdLineRunner::new() defaults for stdio.
            // execute() reads from piped stdout/stderr; execute_raw() overrides to inherit.
            new_cmd.stdin(Stdio::null());
            new_cmd.stdout(Stdio::piped());
            new_cmd.stderr(Stdio::piped());
            if let Some(dir) = self.cmd.as_std().get_current_dir() {
                new_cmd.current_dir(dir);
            }
            if sandbox.effective_deny_env() {
                new_cmd.env_clear();
            }
            for (k, v) in self.cmd.as_std().get_envs() {
                match v {
                    Some(v) => new_cmd.env(k, v),
                    None => new_cmd.env_remove(k),
                };
            }
            self.cmd = new_cmd;
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = sandbox;
            warn!("sandbox is not supported on this platform, running unsandboxed");
        }
        Ok(())
    }

    #[cfg(unix)]
    fn is_etxtbsy(err: &std::io::Error) -> bool {
        err.raw_os_error() == Some(nix::errno::Errno::ETXTBSY as i32)
    }

    #[cfg(not(unix))]
    fn is_etxtbsy(_err: &std::io::Error) -> bool {
        false
    }

    fn on_stdout(&self, line: String) {
        let _lock = OUTPUT_LOCK.lock().unwrap();
        if let Some(observer) = &self.observe_stdout {
            observer(&line);
        }
        if let Some(on_stdout) = &self.on_stdout {
            on_stdout(line);
            return;
        }
        if let Some(pr) = self
            .pr
            .or(self.pr_arc.as_ref().map(|arc| arc.as_ref().as_ref()))
        {
            if !line.trim().is_empty() {
                pr.set_message(line)
            }
        } else {
            let mut stdout = std::io::stdout().lock();
            let _ = if console::colors_enabled() {
                writeln!(stdout, "{line}\x1b[0m")
            } else {
                writeln!(stdout, "{line}")
            };
        }
    }

    fn on_stderr(&self, line: String) {
        let _lock = OUTPUT_LOCK.lock().unwrap();
        if let Some(observer) = &self.observe_stderr {
            observer(&line);
        }
        if let Some(on_stderr) = &self.on_stderr {
            on_stderr(line);
            return;
        }
        match self
            .pr
            .or(self.pr_arc.as_ref().map(|arc| arc.as_ref().as_ref()))
        {
            Some(pr) => {
                if !line.trim().is_empty() {
                    pr.println(line)
                }
            }
            None => {
                let mut stderr = std::io::stderr().lock();
                let _ = if console::colors_enabled_stderr() {
                    writeln!(stderr, "{line}\x1b[0m")
                } else {
                    writeln!(stderr, "{line}")
                };
            }
        }
    }

    fn on_error(&self, output: Vec<(String, OutputSource)>, status: ExitStatus) -> Result<()> {
        match self
            .pr
            .or(self.pr_arc.as_ref().map(|arc| arc.as_ref().as_ref()))
        {
            Some(pr) => {
                error!("{} failed", self.get_program());
                if self.on_stdout.is_none() {
                    // Stdout was hidden behind the progress indicator
                    // (pr.set_message) so replay it on failure. Only replay
                    // stdout — stderr was already printed during execution
                    // via pr.println.
                    let stdout_only: String = output
                        .into_iter()
                        .filter(|(_, source)| matches!(source, OutputSource::Stdout))
                        .map(|(line, _)| line)
                        .collect::<Vec<_>>()
                        .join("\n");
                    if !stdout_only.trim().is_empty() {
                        pr.println(stdout_only);
                    }
                }
            }
            None => {
                // eprintln!("{}", output);
            }
        }
        Err(ScriptFailed(self.get_program(), Some(status)))?
    }

    fn replay_captured_stderr(&self, output: &[(String, OutputSource)]) {
        for (line, source) in output {
            if matches!(source, OutputSource::Stderr) {
                self.on_stderr(line.clone());
            }
        }
    }

    fn get_program(&self) -> String {
        display_path(PathBuf::from(self.cmd.as_std().get_program()))
    }

    fn get_args(&self) -> Vec<String> {
        self.cmd
            .as_std()
            .get_args()
            .map(|s| s.to_string_lossy().to_string())
            .collect::<Vec<_>>()
    }
}

fn raw_read_lock_blocking() -> tokio::sync::RwLockReadGuard<'static, ()> {
    loop {
        if let Ok(guard) = RAW_LOCK.try_read() {
            return guard;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn raw_write_lock_blocking() -> tokio::sync::RwLockWriteGuard<'static, ()> {
    loop {
        if let Ok(guard) = RAW_LOCK.try_write() {
            return guard;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

impl Display for CmdLineRunner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let args = self.get_args().join(" ");
        write!(f, "{} {args}", self.get_program())
    }
}

impl Debug for CmdLineRunner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let args = self.get_args().join(" ");
        write!(f, "{} {args}", self.get_program())
    }
}

/// Tracks whether an output line came from stdout or stderr,
/// so on_error can decide which lines need replaying.
enum OutputSource {
    Stdout,
    Stderr,
}

fn captured_output_lines(
    cmd: &CmdLineRunner<'_>,
    output: &std::process::Output,
) -> Vec<(String, OutputSource)> {
    let mut combined = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        combined.push((cmd.redactor.redact(line), OutputSource::Stdout));
    }
    for line in String::from_utf8_lossy(&output.stderr).lines() {
        combined.push((cmd.redactor.redact(line), OutputSource::Stderr));
    }
    combined
}

enum ChildProcessOutput {
    Stdout(String),
    Stderr(String),
    ExitStatus(ExitStatus),
    #[cfg(not(any(test, target_os = "windows")))]
    Signal(i32),
}

/// Run a command asynchronously with `kill_on_drop(true)` so that timeouts
/// (via `tokio::time::timeout`) actually terminate the subprocess.
///
/// This variant **clears** the environment and sets only the provided `env` —
/// use it for backends that pass a full env from `dependency_env()`.
/// `program` is `AsRef<OsStr>` rather than `&str` so callers can pass a resolved path
/// straight through — `Backend::spawn_program` returns an `OsString`, and forcing it
/// through `to_string_lossy()` here would mangle a Windows path that is not valid UTF-8.
pub async fn cmd_read_async<P, I, K, V>(program: P, args: &[&str], env: I) -> Result<String>
where
    P: AsRef<OsStr>,
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let program = program.as_ref();
    let display_program = program.to_string_lossy();
    let display_args = args.join(" ");
    debug!("$ {display_program} {display_args}");

    let output = tokio::process::Command::new(program)
        .args(args)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .wrap_err_with(|| format!("failed to execute command: {display_program} {display_args}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{display_program} {display_args} failed: exit code {}\n{}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8(output.stdout)
        .wrap_err_with(|| format!("{display_program} produced invalid UTF-8 output"))?;
    Ok(stdout.trim_end().to_string())
}

/// Like [`cmd_read_async`] but **inherits** the current process environment,
/// only adding the provided extra variables on top.
///
/// Use this for core plugins that need the ambient PATH / locale / etc.
pub async fn cmd_read_async_inherited_env<I, K, V>(
    program: &str,
    args: &[&str],
    extra_env: I,
) -> Result<String>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let display_args = args.join(" ");
    debug!("$ {program} {display_args}");

    let output = tokio::process::Command::new(program)
        .args(args)
        .envs(extra_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .wrap_err_with(|| format!("failed to execute command: {program} {display_args}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{program} {display_args} failed: exit code {}\n{}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8(output.stdout)
        .wrap_err_with(|| format!("{program} produced invalid UTF-8 output"))?;
    Ok(stdout.trim_end().to_string())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use pretty_assertions::assert_eq;

    use crate::config::Config;

    #[tokio::test]
    async fn test_cmd() {
        let _config = Config::get().await.unwrap();
        let output = cmd!("echo", "foo", "bar").read().unwrap();
        assert_eq!("foo bar", output);
    }

    #[tokio::test]
    async fn test_cmd_line_runner_execute_async() {
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let observed_stdout = Arc::new(Mutex::new(Vec::new()));
        let observed_stderr = Arc::new(Mutex::new(Vec::new()));
        let stdout_clone = stdout.clone();
        let stderr_clone = stderr.clone();
        let observed_stdout_clone = observed_stdout.clone();
        let observed_stderr_clone = observed_stderr.clone();
        super::CmdLineRunner::new("sh")
            .args(["-c", "printf out; printf err >&2"])
            .with_on_stdout(move |line| stdout_clone.lock().unwrap().push(line))
            .with_on_stderr(move |line| stderr_clone.lock().unwrap().push(line))
            .with_stdout_observer(move |line| {
                observed_stdout_clone.lock().unwrap().push(line.to_string());
            })
            .with_stderr_observer(move |line| {
                observed_stderr_clone.lock().unwrap().push(line.to_string());
            })
            .execute_async()
            .await
            .unwrap();
        assert_eq!(stdout.lock().unwrap().as_slice(), ["out"]);
        assert_eq!(stderr.lock().unwrap().as_slice(), ["err"]);
        assert_eq!(observed_stdout.lock().unwrap().as_slice(), ["out"]);
        assert_eq!(observed_stderr.lock().unwrap().as_slice(), ["err"]);
    }

    #[tokio::test]
    async fn test_execute_async_skips_pre_cancelled_command() {
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "exit 0"])
            .execute_async_with_cancel_check(|| true)
            .await
            .unwrap_err();

        assert!(crate::errors::Error::is_task_interrupted(&err));
    }

    #[tokio::test]
    async fn test_execute_async_catches_cancellation_after_spawn() {
        let checks = Arc::new(AtomicUsize::new(0));
        let checks_c = checks.clone();
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "sleep 30"])
            .execute_async_with_cancel_check(move || checks_c.fetch_add(1, Ordering::SeqCst) > 0)
            .await
            .unwrap_err();

        assert!(crate::errors::Error::is_sigint(&err));
        assert!(checks.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn test_execute_raw_async_catches_cancellation_after_spawn() {
        let checks = Arc::new(AtomicUsize::new(0));
        let checks_c = checks.clone();
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "sleep 30"])
            .raw(true)
            .execute_async_with_cancel_check(move || checks_c.fetch_add(1, Ordering::SeqCst) > 0)
            .await
            .unwrap_err();

        assert!(crate::errors::Error::is_sigint(&err));
        assert!(checks.load(Ordering::SeqCst) >= 2);
    }

    #[tokio::test]
    async fn test_cmd_line_runner_read_ignores_raw_mode() {
        let output = super::CmdLineRunner::new("sh")
            .args(["-c", "printf out"])
            .raw(true)
            .read()
            .await
            .unwrap();
        assert_eq!(output, "out");
    }

    #[tokio::test]
    async fn test_cmd_line_runner_read_replays_stderr_on_failure() {
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let stderr_clone = stderr.clone();
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "printf err >&2; exit 1"])
            .with_on_stderr(move |line| stderr_clone.lock().unwrap().push(line))
            .read()
            .await
            .unwrap_err();
        assert!(err.to_string().contains("exited with non-zero status"));
        assert_eq!(stderr.lock().unwrap().as_slice(), ["err"]);
    }

    #[tokio::test]
    async fn test_cmd_line_runner_execute_hashes_async() {
        let (stdout_hash, stderr_hash) = super::CmdLineRunner::new("sh")
            .args(["-c", "printf stdout; printf stderr >&2"])
            .execute_hashes_async(1024)
            .await
            .unwrap();
        assert_eq!(stdout_hash, blake3::hash(b"stdout").to_hex().to_string());
        assert_eq!(stderr_hash, blake3::hash(b"stderr").to_hex().to_string());
    }

    #[tokio::test]
    async fn test_cmd_line_runner_execute_hashes_async_limits_output() {
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "printf 12345"])
            .execute_hashes_async(4)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("output exceeded 4 bytes"));
    }

    #[tokio::test]
    async fn test_cmd_line_runner_execute_hashes_async_times_out() {
        let err = super::CmdLineRunner::new("sh")
            // Replace the shell so there is no descendant holding the pipes
            // after the timed-out process is terminated.
            .args(["-c", "exec sleep 60"])
            .with_timeout(std::time::Duration::from_millis(10))
            .execute_hashes_async(1024)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn test_cmd_line_runner_execute_hashes_async_rejects_undrained_pipes() {
        let err = super::CmdLineRunner::new("sh")
            .args(["-c", "sleep 60 &"])
            .execute_hashes_async_with_drain_timeout(1024, std::time::Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("command output pipes did not close")
        );
    }

    /// A descendant that outlives the shell must not survive the drain
    /// deadline — cleanup goes through the process group, so it reaches the
    /// leaves even though only the shell is a direct child.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_cmd_line_runner_execute_hashes_async_kills_descendants() {
        if !super::should_use_pgroup() {
            // No pgroup of our own to killpg; an ancestor owns cleanup.
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("descendant.pid");
        let err = super::CmdLineRunner::new("sh")
            .args([
                "-c",
                &format!("sleep 60 & printf %s \"$!\" >{}", pid_file.display()),
            ])
            .execute_hashes_async_with_drain_timeout(1024, std::time::Duration::from_millis(20))
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("command output pipes did not close")
        );
        let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().parse().unwrap();
        let pid = nix::unistd::Pid::from_raw(pid);
        for _ in 0..100 {
            if nix::sys::signal::kill(pid, None).is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("descendant {pid} survived cleanup");
    }

    #[test]
    fn test_env_values_treats_false_as_removal() {
        use std::ffi::OsStr;

        let runner = super::CmdLineRunner::new("true")
            .env("REMOVE", "inherited")
            .env_values([
                (
                    "KEEP",
                    crate::config::env_directive::EnvValue::from("value"),
                ),
                (
                    "REMOVE",
                    crate::config::env_directive::EnvValue::from(false),
                ),
            ]);

        let env = runner.cmd.as_std().get_envs().collect::<Vec<_>>();
        assert!(env.iter().any(|(key, value)| {
            *key == OsStr::new("KEEP") && value == &Some(OsStr::new("value"))
        }));
        assert!(
            env.iter()
                .any(|(key, value)| *key == OsStr::new("REMOVE") && value.is_none())
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn test_macos_sandbox_preserves_env_removals() {
        use std::ffi::OsStr;

        let mut runner = super::CmdLineRunner::new("true")
            .env("KEEP", "value")
            .env_remove("DROP")
            .with_sandbox(crate::sandbox::SandboxConfig {
                deny_read: true,
                ..Default::default()
            });

        runner.apply_sandbox().await.unwrap();

        let env = runner.cmd.as_std().get_envs().collect::<Vec<_>>();
        assert!(env.iter().any(|(key, value)| {
            *key == OsStr::new("KEEP") && value == &Some(OsStr::new("value"))
        }));
        assert!(
            env.iter()
                .any(|(key, value)| *key == OsStr::new("DROP") && value.is_none())
        );
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn test_sandbox_private_temp_blocks_external_write_and_env_leak() {
        let root = tempfile::tempdir().unwrap();
        let allowed = root.path().join("allowed");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let script = format!(
            "env > {}/env; echo inside > {}/inside; echo outside > {}/escaped",
            shell_escape::escape(allowed.to_string_lossy()),
            shell_escape::escape(allowed.to_string_lossy()),
            shell_escape::escape(outside.to_string_lossy()),
        );
        let mut sandbox = crate::sandbox::SandboxConfig {
            deny_write: true,
            deny_net: true,
            deny_env: true,
            allow_write: vec![allowed.clone()],
            deny_system_temp_write: true,
            ..Default::default()
        };
        sandbox.resolve_paths();
        let mut runner = super::CmdLineRunner::new("/bin/sh")
            .args(["-c", &script])
            .env("SECRET_THAT_MUST_NOT_LEAK", "secret")
            .env_clear()
            .env("DOCUMENTED", "yes")
            .with_sandbox(sandbox);
        runner.apply_sandbox().await.unwrap();
        let _error = runner.execute_async().await.unwrap_err();

        assert!(allowed.join("inside").is_file());
        assert!(!outside.join("escaped").exists());
        let child_env = std::fs::read_to_string(allowed.join("env")).unwrap();
        assert!(child_env.contains("DOCUMENTED=yes"));
        assert!(!child_env.contains("SECRET_THAT_MUST_NOT_LEAK"));
    }

    #[test]
    fn test_running_pid_guard_removes_pid() {
        let pid = 424_242;
        assert!(!super::RUNNING_PIDS.lock().unwrap().contains(&pid));
        {
            let _guard = super::RunningPidGuard::new(Some(pid));
            assert!(super::RUNNING_PIDS.lock().unwrap().contains(&pid));
        }
        assert!(!super::RUNNING_PIDS.lock().unwrap().contains(&pid));
    }

    #[test]
    fn test_cmd_body_args_unix_fallthrough() {
        // On Unix `cmd_body_args` must be exactly `args(flags).arg(body)` — the
        // non-regression contract shared by every CmdLineRunner call site.
        let r = super::CmdLineRunner::new("bash").cmd_body_args(&["-c".to_string()], "echo hi");
        assert_eq!(r.get_args(), vec!["-c".to_string(), "echo hi".to_string()]);
    }
}

#[cfg(test)]
#[cfg(windows)]
mod windows_tests {
    #[test]
    fn test_cmd_body_args_cmd_verbatim() {
        // cmd /c <body>: the verbatim branch produces the cmd_verbatim_args output
        // (`/s /c "<body>"`) rather than the fall-through [/c, <body>] layout.
        let r =
            super::CmdLineRunner::new("cmd").cmd_body_args(&["/c".to_string()], r#"echo "a b""#);
        assert!(r.get_program().to_lowercase().contains("cmd"));
        assert_eq!(
            r.get_args(),
            vec![
                "/s".to_string(),
                "/c".to_string(),
                r#""echo "a b"""#.to_string()
            ]
        );
    }

    #[test]
    fn test_cmd_body_args_non_cmd_fallthrough() {
        // A non-cmd Windows shell keeps the plain args(flags).arg(body) layout.
        let r = super::CmdLineRunner::new("pwsh")
            .cmd_body_args(&["-Command".to_string()], r#"echo "a b""#);
        assert_eq!(
            r.get_args(),
            vec!["-Command".to_string(), r#"echo "a b""#.to_string()]
        );
    }
}
