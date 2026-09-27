mod bounded;
#[cfg(windows)]
pub mod ctrl_c_group;
use std::collections::{HashSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Display, Formatter};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
#[cfg(panic = "abort")]
use std::sync::TryLockError;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::redactions::Redactor;
use duct::{Expression, IntoExecutablePath};
use eyre::Result;
use eyre::{Context, bail};
#[cfg(not(target_os = "windows"))]
use signal_hook::consts::{SIGHUP, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
#[cfg(not(target_os = "windows"))]
use signal_hook::iterator::Signals;
use std::sync::LazyLock as Lazy;
#[cfg(unix)]
use tokio::io::AsyncRead;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as TokioBufReader};
use tokio::process::Command;

use crate::env;
use crate::env::PATH_KEY;
use crate::env_value::EnvValue;
use crate::errors::ProcessError::ScriptFailed;
use crate::file::display_path;
use crate::path_env::PathEnv;
use crate::progress::SingleReport;
use mise_settings::Settings;

/// Create a command with any number of of positional arguments
///
/// may be different types (anything that implements [`Into<OsString>`](https://doc.rust-lang.org/std/convert/trait.From.html)).
/// See also the [`cmd`](fn.cmd.html) function, which takes a collection of arguments.
///
/// # Example
///
/// ```ignore
///     use std::path::Path;
///     use mise_util::cmd;
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
/// ```ignore
///     use std::path::Path;
///     use mise_util::cmd;
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

fn chain_observers<'a, F: Fn(&str) + Send + 'a>(
    previous: Option<OutputObserver<'a>>,
    next: F,
) -> OutputObserver<'a> {
    match previous {
        Some(previous) => Box::new(move |line| {
            previous(line);
            next(line);
        }),
        None => Box::new(next),
    }
}

pub struct CmdLineRunner<'a> {
    cmd: Command,
    pr: Option<&'a dyn SingleReport>,
    pr_arc: Option<Arc<Box<dyn SingleReport>>>,
    stdin: Option<String>,
    redactor: Redactor,
    raw: bool,
    /// Refuse raw mode for this one command, however it was requested.
    never_raw: bool,
    pass_signals: bool,
    on_stdout: Option<Box<dyn Fn(String) + Send + 'a>>,
    on_stderr: Option<Box<dyn Fn(String) + Send + 'a>>,
    stderr_as_stdout: bool,
    observe_stdout: Option<OutputObserver<'a>>,
    observe_stderr: Option<OutputObserver<'a>>,
    timeout: Option<Duration>,
    sandbox: Option<crate::sandbox::SandboxConfig>,
    inherit_env: bool,
    stdio: PendingStdio,
    kill_on_drop: bool,
    /// Indices of the arguments added with `raw_arg`, which a Ctrl+C group
    /// leader has to add the same way.
    #[cfg(windows)]
    raw_args: Vec<usize>,
    /// Whether the spawned process leads a Ctrl+C group (see [`ctrl_c_group`]):
    /// asked for by [`Self::interrupt_on_timeout`], kept only with a timeout.
    #[cfg(windows)]
    ctrl_c_group: bool,
}

/// Stdio for a command, applied when it is spawned. `Command` cannot hand its
/// stdio back, so it is held here for whichever command is finally spawned.
#[derive(Default)]
struct PendingStdio {
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl PendingStdio {
    /// What `CmdLineRunner::new` starts with: no stdin, both outputs piped.
    fn defaults() -> Self {
        Self {
            stdin: Some(Stdio::null()),
            stdout: Some(Stdio::piped()),
            stderr: Some(Stdio::piped()),
        }
    }

    fn apply(&mut self, cmd: &mut Command) {
        if let Some(stdin) = self.stdin.take() {
            cmd.stdin(stdin);
        }
        if let Some(stdout) = self.stdout.take() {
            cmd.stdout(stdout);
        }
        if let Some(stderr) = self.stderr.take() {
            cmd.stderr(stderr);
        }
    }
}

const GUARD_RUNNING: u8 = 0;
const GUARD_CANCELLED: u8 = 1;
const GUARD_TIMED_OUT: u8 = 2;

/// How long a timed-out command gets to exit after being asked to (SIGTERM, or
/// Ctrl+C on Windows) before it is killed.
const TIMEOUT_GRACE: Duration = Duration::from_secs(5);

#[cfg(unix)]
fn in_terminal_foreground_pgrp() -> bool {
    let pgrp = nix::unistd::getpgrp();
    nix::unistd::tcgetpgrp(std::io::stdin()) == Ok(pgrp)
        || nix::unistd::tcgetpgrp(std::io::stdout()) == Ok(pgrp)
        || nix::unistd::tcgetpgrp(std::io::stderr()) == Ok(pgrp)
}

/// Whether a SIGINT came from a process (`kill`, `sigqueue`) rather than from
/// the terminal since `kill_all` last looked. Only Linux can tell: macOS gives
/// a terminal Ctrl-C the same `si_code` and a sender pid, just like `kill`.
///
/// A SIGINT that arrives before `kill_all` has handled the previous one must
/// not hide that earlier origin, so the handler only ever sets this and
/// `kill_all` takes it.
///
/// `si_code` doesn't say whom the sender targeted: `kill -INT -<pgid>` also
/// reports SI_USER, so children in that group get the SIGINT twice. Nothing
/// tells the two apart, and a second SIGINT beats none: a child that never
/// gets one keeps mise waiting on it.
#[cfg(target_os = "linux")]
static SIGINT_FROM_PROCESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Watch SIGINT from the signal handler itself: note it for
/// [`crate::cancel::is_cancelled`], and on Linux record where it came from, so
/// `kill_all` still passes on a SIGINT that was sent to mise alone. Call this
/// before handling Ctrl-C.
#[cfg(unix)]
pub fn track_sigint() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: the actions only store to atomics, which is
        // async-signal-safe.
        #[cfg(target_os = "linux")]
        let registered = unsafe {
            signal_hook_registry::register_sigaction(nix::libc::SIGINT, |info| {
                // The terminal sends SI_KERNEL. `kill` sends SI_USER (0),
                // and `sigqueue`, `tgkill` and the like send negative codes.
                if info.si_code <= 0 {
                    SIGINT_FROM_PROCESS.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                crate::cancel::note_signal();
            })
        };
        #[cfg(not(target_os = "linux"))]
        let registered = unsafe {
            signal_hook::low_level::register(
                signal_hook::consts::SIGINT,
                crate::cancel::note_signal,
            )
        };
        if let Err(e) = registered {
            debug!("failed to track SIGINT: {e}");
        }
    });
}

/// Whether the SIGINT `kill_all` is about to pass on has already reached the
/// children that share mise's process group.
#[cfg(unix)]
fn sigint_reached_own_pgrp() -> bool {
    // Take the flag even when it isn't needed, so it never outlives its SIGINT.
    #[cfg(target_os = "linux")]
    let from_process = SIGINT_FROM_PROCESS.swap(false, std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(target_os = "linux"))]
    let from_process = false;
    // An outer mise that owns our process group signals all of it with
    // `killpg`, so the children we share it with got that SIGINT directly,
    // however it looks to us. Without this, a nested mise would pass it on a
    // second time. The cost is a `kill -INT` sent to this nested mise alone,
    // which its children never see; the outer mise, whose Ctrl-C reaches us
    // here, is the realistic sender, and nothing in a SIGINT tells the two
    // apart.
    if std::env::var_os(TASK_PGID_MANAGED_ENV).is_some() {
        return true;
    }
    !from_process && in_terminal_foreground_pgrp()
}

#[cfg(unix)]
fn signal_process_tree(pid: u32, signal: nix::sys::signal::Signal) {
    let pid = nix::unistd::Pid::from_raw(pid as i32);
    if !should_use_pgroup() || nix::sys::signal::killpg(pid, signal).is_err() {
        let _ = nix::sys::signal::kill(pid, signal);
    }
}

/// Those of `pids` for which anything that [`signal_process_tree`] would reach
/// is still running.
#[cfg(unix)]
fn alive_process_trees(pids: &HashSet<u32>) -> HashSet<u32> {
    #[cfg(target_os = "linux")]
    if let Some(alive) = alive_process_trees_procfs(pids) {
        return alive;
    }
    pids.iter()
        .copied()
        .filter(|&pid| {
            let pid = nix::unistd::Pid::from_raw(pid as i32);
            (should_use_pgroup() && nix::sys::signal::killpg(pid, None).is_ok())
                || nix::sys::signal::kill(pid, None).is_ok()
        })
        .collect()
}

/// Signal 0 also reaches zombies, which an init that never reaps keeps around,
/// so read process states instead. `None` when /proc cannot be read.
#[cfg(target_os = "linux")]
fn alive_process_trees_procfs(pids: &HashSet<u32>) -> Option<HashSet<u32>> {
    let use_pgroup = should_use_pgroup();
    let entries = std::fs::read_dir("/proc").ok()?;
    let mut alive = HashSet::new();
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse().ok()) else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // "pid (comm) state ppid pgrp ..."; comm may itself contain ") ".
        let Some(fields) = stat.rfind(')').map(|i| &stat[i + 1..]) else {
            continue;
        };
        let mut fields = fields.split_whitespace();
        let (Some(state), Some(_ppid), Some(pgrp)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if state == "Z" {
            continue;
        }
        if pids.contains(&pid) {
            alive.insert(pid);
        }
        if use_pgroup
            && let Ok(pgrp) = pgrp.parse::<u32>()
            && pids.contains(&pgrp)
        {
            alive.insert(pgrp);
        }
    }
    Some(alive)
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
    /// `interruptible` is whether `pid` can be asked to stop before it is killed:
    /// always on Unix, and on Windows when it leads a Ctrl+C group.
    fn new(timeout: Duration, pid: u32, interruptible: bool) -> Self {
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
                debug_assert!(interruptible, "Unix can always signal a command");
                signal_process_tree(pid, nix::sys::signal::Signal::SIGTERM);
                drop(guard);
                let guard = lock.lock().unwrap();
                let grace_deadline = std::time::Instant::now() + TIMEOUT_GRACE;
                let (_guard, cancelled) = wait_for_cancel_or_deadline(cvar, guard, grace_deadline);
                if !cancelled {
                    signal_process_tree(pid, nix::sys::signal::Signal::SIGKILL);
                }
            }
            #[cfg(windows)]
            {
                drop(guard);
                // Without a console to raise Ctrl+C on, there is nothing to wait for.
                if interruptible && ctrl_c_group::interrupt(pid) {
                    let guard = lock.lock().unwrap();
                    let grace_deadline = std::time::Instant::now() + TIMEOUT_GRACE;
                    let (_guard, cancelled) =
                        wait_for_cancel_or_deadline(cvar, guard, grace_deadline);
                    if cancelled {
                        return;
                    }
                }
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

    /// How long the command ran before this guard stopped it, if it did. A command
    /// can exit cleanly on SIGTERM or Ctrl+C, so its exit status cannot tell.
    fn timed_out(&self) -> Option<Duration> {
        (self.state.load(Ordering::Acquire) == GUARD_TIMED_OUT).then_some(self.timeout)
    }
}

/// Whether the timeout had fired by the time the command exited. Taken at exit,
/// not after draining its pipes, which a background process can hold open past
/// the deadline.
fn timed_out_at_exit(guard: Option<&TimeoutGuard>) -> Option<Duration> {
    guard.and_then(TimeoutGuard::timed_out)
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        self.cancel();
    }
}

static OUTPUT_LOCK: Mutex<()> = Mutex::new(());
static RAW_LOCK: Lazy<tokio::sync::RwLock<()>> = Lazy::new(|| tokio::sync::RwLock::new(()));

static RUNNING_PIDS: Lazy<Mutex<HashSet<u32>>> = Lazy::new(Default::default);
/// Set by [`CmdLineRunner::terminate_all`]; nothing started afterwards may keep running.
static TERMINATING: AtomicBool = AtomicBool::new(false);

/// Track a started command for `kill_all`. One that started after
/// `terminate_all` began missed its signals, so it is killed here instead.
fn register_running_pid(pid: u32) {
    RUNNING_PIDS.lock().unwrap().insert(pid);
    if TERMINATING.load(Ordering::SeqCst) {
        #[cfg(unix)]
        signal_process_tree(pid, nix::sys::signal::SIGKILL);
        #[cfg(windows)]
        kill_process_tree(pid);
    }
}

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

pub struct RunningPidGuard(Option<u32>);

impl RunningPidGuard {
    pub fn new(pid: Option<u32>) -> Self {
        if let Some(pid) = pid {
            register_running_pid(pid);
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
/// process groups (calling setpgid/setsid + killpg). A nested mise that sees this
/// var skips its own setpgid so descendants stay in the outer pgid — that
/// way the outer mise's killpg actually reaches the leaves.
#[cfg(unix)]
const TASK_PGID_MANAGED_ENV: &str = "MISE_TASK_PGID_MANAGED";

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildProcessIsolation {
    Inherit,
    ProcessGroup,
    Session,
}

#[cfg(unix)]
fn child_process_isolation(
    child_stdin_is_terminal: bool,
    parent_has_terminal: bool,
    is_macos: bool,
) -> ChildProcessIsolation {
    if child_stdin_is_terminal {
        ChildProcessIsolation::Inherit
    } else if is_macos && parent_has_terminal {
        ChildProcessIsolation::Session
    } else {
        ChildProcessIsolation::ProcessGroup
    }
}

#[cfg(unix)]
fn parent_has_terminal() -> bool {
    use std::io::IsTerminal;

    std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
}

/// Put an ordinary non-raw command in the process tree managed by mise.
///
/// On macOS, a non-interactive zsh child in its own process group can be
/// stopped by job control when it uses process substitution under a
/// controlling terminal (for example, inside tmux). A separate session keeps
/// the child detached from that terminal while preserving the invariant that
/// its PID is also its process group ID for killpg-based cleanup.
#[cfg(unix)]
fn prepare_execute_child(cmd: &mut std::process::Command) {
    if !should_use_pgroup() {
        return;
    }

    cmd.env(TASK_PGID_MANAGED_ENV, "1");
    let parent_has_terminal = parent_has_terminal();
    unsafe {
        cmd.pre_exec(move || {
            // Use BorrowedFd::borrow_raw rather than std::io::stdin() —
            // pre_exec runs post-fork where OnceLock/malloc are not
            // async-signal-safe.
            let stdin = std::os::fd::BorrowedFd::borrow_raw(0);
            let child_stdin_is_terminal = std::io::IsTerminal::is_terminal(&stdin);
            match child_process_isolation(
                child_stdin_is_terminal,
                parent_has_terminal,
                cfg!(target_os = "macos"),
            ) {
                ChildProcessIsolation::Inherit => Ok(()),
                ChildProcessIsolation::ProcessGroup => {
                    let _ = nix::unistd::setpgid(
                        nix::unistd::Pid::from_raw(0),
                        nix::unistd::Pid::from_raw(0),
                    );
                    Ok(())
                }
                ChildProcessIsolation::Session => {
                    nix::unistd::setsid().map(|_| ()).map_err(Into::into)
                }
            }
        });
    }
}

/// True when this mise should isolate spawned children into process groups and
/// `killpg` them for cleanup.
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
/// Cached on first access: `execute()` decides whether to create a managed
/// process group or session at spawn time, and `kill_all()` decides whether to
/// `killpg` at signal time. They must agree — a child placed in its own pgid by
/// `execute()` must be killed via `killpg`, or only the direct PID gets the
/// signal and grandchildren leak. Computing this once removes any chance of
/// the two callers disagreeing if the env later mutates.
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
pub fn prepare_noninteractive_child(_cmd: &mut std::process::Command) {
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

/// Maximum amount of stdout retained for commands whose output is hidden
/// behind a progress indicator. The tail is replayed if the command fails.
const FAILURE_OUTPUT_TAIL_BYTES: usize = 64 * 1024;
/// How much of the child's final stderr line the failure carries. Long enough
/// for a loader or compiler diagnostic, short enough to stay on one row.
const STDERR_TAIL_MAX_CHARS: usize = 300;
const FAILURE_OUTPUT_TRUNCATED_NOTICE: &str = "[output truncated; showing last 64 KiB]";

#[derive(Default)]
struct FailureOutputTail {
    lines: VecDeque<String>,
    bytes: usize,
    truncated: bool,
}

impl FailureOutputTail {
    fn push(&mut self, mut line: String) {
        let max_line_bytes = FAILURE_OUTPUT_TAIL_BYTES.saturating_sub(1);
        if line.len() > max_line_bytes {
            let mut start = line.len() - max_line_bytes;
            while !line.is_char_boundary(start) {
                start += 1;
            }
            line = line[start..].to_string();
            self.lines.clear();
            self.bytes = 0;
            self.truncated = true;
        }

        self.bytes = self.bytes.saturating_add(line.len().saturating_add(1));
        self.lines.push_back(line);
        while self.bytes > FAILURE_OUTPUT_TAIL_BYTES {
            if let Some(line) = self.lines.pop_front() {
                self.bytes = self.bytes.saturating_sub(line.len().saturating_add(1));
                self.truncated = true;
            } else {
                break;
            }
        }
    }

    fn into_output(mut self) -> Vec<(String, OutputSource)> {
        if self.truncated {
            self.lines
                .push_front(FAILURE_OUTPUT_TRUNCATED_NOTICE.to_string());
        }
        self.lines
            .into_iter()
            .map(|line| (line, OutputSource::Stdout))
            .collect()
    }
}

enum HashedProcessOutput {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    ReadError(&'static str, std::io::Error),
}

#[cfg(unix)]
async fn read_capped<R: AsyncRead + Unpin>(
    mut reader: R,
    max_bytes: usize,
) -> std::io::Result<(Vec<u8>, usize)> {
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut buffer = [0; 8192];
    loop {
        let len = reader.read(&mut buffer).await?;
        if len == 0 {
            break;
        }
        total = total.saturating_add(len);
        let remaining = max_bytes.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..len.min(remaining)]);
    }
    Ok((kept, total))
}

impl<'a> CmdLineRunner<'a> {
    fn failure_output_tail(&self) -> Option<FailureOutputTail> {
        if self.on_stdout.is_none() && (self.pr.is_some() || self.pr_arc.is_some()) {
            Some(FailureOutputTail::default())
        } else {
            None
        }
    }

    pub fn new<P: AsRef<OsStr>>(program: P) -> Self {
        Self {
            cmd: Command::new(program),
            pr: None,
            pr_arc: None,
            stdin: None,
            redactor: Default::default(),
            raw: false,
            never_raw: false,
            pass_signals: false,
            on_stdout: None,
            on_stderr: None,
            stderr_as_stdout: false,
            observe_stdout: None,
            observe_stderr: None,
            timeout: None,
            sandbox: None,
            inherit_env: true,
            stdio: PendingStdio::defaults(),
            kill_on_drop: false,
            #[cfg(windows)]
            raw_args: Vec::new(),
            #[cfg(windows)]
            ctrl_c_group: false,
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
        let already_delivered = signal == nix::sys::signal::SIGINT && sigint_reached_own_pgrp();
        let own_pgid = nix::unistd::getpgrp();
        let pids = RUNNING_PIDS.lock().unwrap();
        for pid in pids.iter() {
            let pid = *pid as i32;
            let nix_pid = nix::unistd::Pid::from_raw(pid);
            // A terminal Ctrl-C, or an outer mise's `killpg`, already reached
            // children in our pgid. A second SIGINT makes some of them, such
            // as a nested mise or `docker compose`, force-quit instead of
            // shutting down.
            if already_delivered && nix::unistd::getpgid(Some(nix_pid)) == Ok(own_pgid) {
                trace!("{signal}: {pid} already signalled");
                continue;
            }
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
        let killers: Vec<_> = {
            let pids = RUNNING_PIDS.lock().unwrap();
            pids.iter()
                .filter_map(|pid| {
                    std::process::Command::new("taskkill")
                        .arg("/F")
                        .arg("/T")
                        .arg("/PID")
                        .arg(pid.to_string())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .inspect_err(|e| warn!("Failed to kill cmd {pid}: {e}"))
                        .ok()
                })
                .collect()
        };
        // Started together and waited on afterwards: a caller that exits as soon
        // as this returns — the Ctrl-C handler does — must not leave a tree it
        // is taking down still standing, and the trees come down concurrently
        // rather than one `taskkill` at a time.
        for mut killer in killers {
            let _ = killer.wait();
        }
    }

    /// Stop every running command the way a per-command timeout does: SIGTERM,
    /// then SIGKILL for whatever has not exited after the grace period. Windows
    /// has no graceful stop yet, so it force-kills the trees at once. Commands
    /// started from here on are killed as they start, for the rest of the process.
    pub async fn terminate_all() {
        TERMINATING.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        {
            // Kept past the leaders' exit: a shell that dies on SIGTERM leaves
            // RUNNING_PIDS while a child ignoring it keeps the group alive.
            let mut pids = RUNNING_PIDS.lock().unwrap().clone();
            for &pid in &pids {
                signal_process_tree(pid, nix::sys::signal::SIGTERM);
            }
            let deadline = Instant::now() + TIMEOUT_GRACE;
            loop {
                // A tree once seen gone is never signalled again: its ID could
                // by now belong to an unrelated process.
                pids = alive_process_trees(&pids);
                if pids.is_empty() || Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            for &pid in &pids {
                signal_process_tree(pid, nix::sys::signal::SIGKILL);
            }
        }
        #[cfg(windows)]
        Self::kill_all();
    }

    pub fn stdin<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.stdio.stdin = Some(cfg.into());
        self
    }

    pub fn stdout<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.stdio.stdout = Some(cfg.into());
        self
    }

    pub fn stderr<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.stdio.stderr = Some(cfg.into());
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

    pub fn stderr_as_stdout(mut self) -> Self {
        self.stderr_as_stdout = true;
        self
    }

    /// Add an observer for each stdout line. Observers compose: one added
    /// later runs after the earlier ones instead of replacing them, so the
    /// task cache and log export can both watch the same stream.
    pub fn with_stdout_observer<F: Fn(&str) + Send + 'a>(mut self, observer: F) -> Self {
        self.observe_stdout = Some(chain_observers(self.observe_stdout.take(), observer));
        self
    }

    /// Add an observer for each stderr line. See [`Self::with_stdout_observer`].
    pub fn with_stderr_observer<F: Fn(&str) + Send + 'a>(mut self, observer: F) -> Self {
        self.observe_stderr = Some(chain_observers(self.observe_stderr.take(), observer));
        self
    }

    /// Whether a stdout observer is attached. Used to decide whether stdout
    /// must stay piped in output modes that would otherwise inherit it.
    pub fn has_stdout_observer(&self) -> bool {
        self.observe_stdout.is_some()
    }

    /// Whether a stderr observer is attached. See [`Self::has_stdout_observer`].
    pub fn has_stderr_observer(&self) -> bool {
        self.observe_stderr.is_some()
    }

    pub fn current_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.cmd.current_dir(dir);
        self
    }

    pub fn env_clear(mut self) -> Self {
        self.cmd.env_clear();
        self.inherit_env = false;
        self
    }

    /// Must run after environment/cwd configuration and before custom stdio or
    /// pre-exec setup. Sandboxed commands retain their shell and policy target.
    pub fn optimize_inline(mut self, body: &str, forwarded: &[String], enabled: bool) -> Self {
        if self.sandbox.is_none()
            && let Some(command) = crate::inline_command::direct_command(
                self.cmd.as_std(),
                self.inherit_env,
                body,
                forwarded,
                enabled,
            )
        {
            self.cmd = command.into();
            self.stdio = PendingStdio::defaults();
            self.inherit_env = false;
            #[cfg(windows)]
            self.raw_args.clear();
        }
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

    pub fn get_env(&self, key: &str) -> Option<&OsStr> {
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
        self.raw_args.push(self.cmd.as_std().get_args().len());
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

    /// Never run this command in raw mode, even when the setting asks for it.
    ///
    /// Raw mode hands the child mise's own stdout and stderr, which is the
    /// point of it, and also means nothing passes through the redactor. For a
    /// command whose arguments carry a credential that is not a trade the
    /// `raw` setting can reasonably be making on the user's behalf: they asked
    /// for unfiltered output, not for a token in their scrollback.
    ///
    /// Deliberately per-command rather than "any runner with redactions".
    /// Tasks register redactions too, and silently moving an interactive task
    /// off raw mode would change what it can do.
    pub fn never_raw(mut self) -> Self {
        self.never_raw = true;
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
        self.stdio.stdin = Some(Stdio::piped());
        self.stdin = Some(input.into());
        self
    }

    pub fn execute(mut self) -> Result<()> {
        let read_lock = raw_read_lock_blocking();
        debug!("$ {self}");
        if (Settings::get().raw || self.raw) && !self.never_raw {
            drop(read_lock);
            let _write_lock = raw_write_lock_blocking();
            return self.execute_raw();
        }
        #[cfg(unix)]
        prepare_execute_child(self.cmd.as_std_mut());
        self.interrupt_on_timeout();
        let mut cp = self
            .spawn_with_etxtbsy_retry()
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id();
        register_running_pid(id);
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
        #[cfg(not(target_os = "windows"))]
        let mut sighandle = None;
        #[cfg(not(target_os = "windows"))]
        if self.pass_signals && !crate::testing::active() {
            // SIGINT is left to mise's Ctrl-C handler, whose `kill_all`
            // already signals every running command. Forwarding it here too
            // delivered each Ctrl-C twice, which a nested mise (and tools like
            // `docker compose`) take as a second Ctrl-C and force-quit.
            let mut signals = Signals::new([SIGTERM, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2])?;
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
            #[cfg(not(target_os = "windows"))]
            if let Some(sighandle) = sighandle {
                sighandle.close();
            }
            let _ = tx.send(ChildProcessOutput::ExitStatus(status));
        });

        let timeout_guard = self
            .timeout
            .map(|t| TimeoutGuard::new(t, id, self.interruptible()));

        let mut failure_output = self.failure_output_tail();
        // The child's last word, kept for the error itself. The live output is
        // long gone by the time anyone reads `exit code 127`, and under
        // `--quiet` it was never printed at all.
        let mut last_stderr: Option<String> = None;
        let mut status = None;
        let mut timed_out_by_exit = None;
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
                    if let Some(output) = &mut failure_output {
                        self.on_stdout(line.clone());
                        output.push(line);
                    } else {
                        self.on_stdout(line);
                    }
                }
                ChildProcessOutput::Stderr(line) => {
                    let line = self.redactor.redact(&line);
                    if !line.trim().is_empty() {
                        last_stderr = Some(line.clone());
                    }
                    if self.stderr_as_stdout
                        && self.on_stderr.is_none()
                        && let Some(output) = &mut failure_output
                    {
                        self.on_stderr(line.clone());
                        output.push(line);
                    } else {
                        self.on_stderr(line);
                    }
                }
                ChildProcessOutput::ExitStatus(s) => {
                    status = Some(s);
                    timed_out_by_exit = timed_out_at_exit(timeout_guard.as_ref());
                    drain_deadline = Some(Instant::now() + PIPE_DRAIN_TIMEOUT);
                }
                #[cfg(not(windows))]
                ChildProcessOutput::Signal(sig) => {
                    let pid = nix::unistd::Pid::from_raw(id as i32);
                    let nix_sig = nix::sys::signal::Signal::try_from(sig).unwrap();
                    if should_use_pgroup() {
                        debug!("Received signal {sig}, forwarding to pgid {id}");
                        if nix::sys::signal::killpg(pid, nix_sig).is_err() {
                            let _ = nix::sys::signal::kill(pid, nix_sig);
                        }
                    } else {
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

        if let Some(duration) = timed_out_by_exit {
            bail!("timed out after {duration:?}");
        }
        if !status.success() {
            let mut output = failure_output.map_or_else(Vec::new, FailureOutputTail::into_output);
            if let Some(line) = last_stderr {
                output.push((line, OutputSource::Stderr));
            }
            self.on_error(output, status)?;
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
            return Err(crate::errors::ProcessError::TaskInterrupted.into());
        }
        let read_lock = raw_read_lock().await;
        debug!("$ {self}");
        if (Settings::get().raw || self.raw) && !self.never_raw {
            drop(read_lock);
            let _write_lock = raw_write_lock().await;
            return self.execute_raw_async_with_cancel_check(is_cancelled).await;
        }
        #[cfg(unix)]
        prepare_execute_child(self.cmd.as_std_mut());
        self.interrupt_on_timeout();
        let mut cp = self
            .spawn_async_with_etxtbsy_retry()
            .await
            .wrap_err_with(|| format!("failed to execute command: {self}"))?;
        let id = cp.id().unwrap_or_default();
        register_running_pid(id);
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
        #[cfg(not(target_os = "windows"))]
        let mut sighandle = None;
        #[cfg(not(target_os = "windows"))]
        if self.pass_signals && !crate::testing::active() {
            // SIGINT is left to mise's Ctrl-C handler; see `execute`.
            let mut signals = Signals::new([SIGTERM, SIGHUP, SIGQUIT, SIGUSR1, SIGUSR2])?;
            sighandle = Some(signals.handle());
            let tx = tx.clone();
            thread::spawn(move || {
                for sig in &mut signals {
                    let _ = tx.send(ChildProcessOutput::Signal(sig));
                }
            });
        }
        drop(tx);

        let timeout_guard = self
            .timeout
            .map(|t| TimeoutGuard::new(t, id, self.interruptible()));
        let mut failure_output = self.failure_output_tail();
        // The child's last word, kept for the error itself. The live output is
        // long gone by the time anyone reads `exit code 127`, and under
        // `--quiet` it was never printed at all.
        let mut last_stderr: Option<String> = None;
        let mut status = None;
        let mut wait = Box::pin(cp.wait());
        loop {
            tokio::select! {
                result = &mut wait, if status.is_none() => {
                    #[cfg(not(target_os = "windows"))]
                    if let Some(sighandle) = sighandle.take() {
                        sighandle.close();
                    }
                    status = Some(result?);
                    break;
                }
                msg = rx.recv() => {
                    let Some(msg) = msg else {
                        if status.is_none() {
                            #[cfg(not(target_os = "windows"))]
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
                            if let Some(output) = &mut failure_output {
                                self.on_stdout(line.clone());
                                output.push(line);
                            } else {
                                self.on_stdout(line);
                            }
                        }
                        ChildProcessOutput::Stderr(line) => {
                            let line = self.redactor.redact(&line);
                            if !line.trim().is_empty() {
                                last_stderr = Some(line.clone());
                            }
                            if self.stderr_as_stdout
                                && self.on_stderr.is_none()
                                && let Some(output) = &mut failure_output
                            {
                                self.on_stderr(line.clone());
                                output.push(line);
                            } else {
                                self.on_stderr(line);
                            }
                        }
                        ChildProcessOutput::ExitStatus(_) => {}
                        #[cfg(not(windows))]
                        ChildProcessOutput::Signal(sig) => {
                            let pid = nix::unistd::Pid::from_raw(id as i32);
                            let nix_sig = nix::sys::signal::Signal::try_from(sig).unwrap();
                            if should_use_pgroup() {
                                debug!("Received signal {sig}, forwarding to pgid {id}");
                                if nix::sys::signal::killpg(pid, nix_sig).is_err() {
                                    let _ = nix::sys::signal::kill(pid, nix_sig);
                                }
                            } else {
                                debug!("Received signal {sig}, forwarding to {id}");
                                let _ = nix::sys::signal::kill(pid, nix_sig);
                            }
                        }
                    }
                }
            }
        }
        let timed_out_by_exit = timed_out_at_exit(timeout_guard.as_ref());
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
                    if let Some(output) = &mut failure_output {
                        self.on_stdout(line.clone());
                        output.push(line);
                    } else {
                        self.on_stdout(line);
                    }
                }
                ChildProcessOutput::Stderr(line) => {
                    let line = self.redactor.redact(&line);
                    if !line.trim().is_empty() {
                        last_stderr = Some(line.clone());
                    }
                    if self.stderr_as_stdout
                        && self.on_stderr.is_none()
                        && let Some(output) = &mut failure_output
                    {
                        self.on_stderr(line.clone());
                        output.push(line);
                    } else {
                        self.on_stderr(line);
                    }
                }
                ChildProcessOutput::ExitStatus(_) => {}
                #[cfg(not(windows))]
                ChildProcessOutput::Signal(_) => {}
            }
        }
        RUNNING_PIDS.lock().unwrap().remove(&id);
        if let Some(g) = &timeout_guard {
            g.cancel();
        }

        let status = status.unwrap();
        if let Some(duration) = timed_out_by_exit {
            bail!("timed out after {duration:?}");
        }
        if !status.success() {
            let mut output = failure_output.map_or_else(Vec::new, FailureOutputTail::into_output);
            if let Some(line) = last_stderr {
                output.push((line, OutputSource::Stderr));
            }
            self.on_error(output, status)?;
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
        let _read_lock = raw_read_lock().await;
        debug!("$ {self}");
        self.kill_on_drop = true;
        // These commands are non-interactive probes: nothing reads stdin and
        // both output streams are piped. Detaching stdin from the terminal
        // means the child can never need the controlling TTY, so unlike
        // `execute()` we can always create a dedicated process group without
        // risking SIGTTIN. That guarantee matters here — cleanup on timeout,
        // an output-limit breach, or a stuck pipe relies on `killpg` reaching
        // descendants, not just the direct child.
        self.stdio.stdin = Some(Stdio::null());
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
        self.interrupt_on_timeout();
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

        let timeout_guard = self
            .timeout
            .map(|timeout| TimeoutGuard::new(timeout, id, self.interruptible()));
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
        let timed_out_by_exit = timed_out_at_exit(timeout_guard.as_ref());
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
        if let Some(timeout) = timed_out_by_exit {
            bail!("timed out after {timeout:?}");
        }
        if !status.success() {
            bail!("exited with non-zero status: {status}");
        }
        Ok((
            stdout_hasher.finalize().to_hex().to_string(),
            stderr_hasher.finalize().to_hex().to_string(),
        ))
    }

    /// Run the command and return stdout, even when raw mode is enabled.
    pub async fn read(mut self) -> Result<String> {
        let _read_lock = raw_read_lock().await;
        debug!("$ {self}");
        self.kill_on_drop = true;
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

    #[cfg(unix)]
    pub async fn read_bounded(mut self, max_output_bytes: usize) -> Result<String> {
        let _read_lock = raw_read_lock().await;
        debug!("$ {self}");
        self.kill_on_drop = true;
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
        if let Some(text) = self.stdin.take()
            && let Some(mut stdin) = cp.stdin.take()
        {
            tokio::spawn(async move {
                let _ = stdin.write_all(text.as_bytes()).await;
            });
        }
        let stdout = cp.stdout.take().expect("stdout must be piped");
        let stderr = cp.stderr.take().expect("stderr must be piped");
        let stdout_task = tokio::spawn(read_capped(stdout, max_output_bytes));
        let stderr_task = tokio::spawn(read_capped(stderr, max_output_bytes));
        let status = match self.timeout {
            Some(timeout) => match tokio::time::timeout(timeout, cp.wait()).await {
                Ok(status) => status?,
                Err(_) => {
                    #[cfg(unix)]
                    signal_process_tree(id, nix::sys::signal::Signal::SIGKILL);
                    #[cfg(windows)]
                    kill_process_tree(id);
                    let _ = cp.wait().await;
                    bail!("timed out after {timeout:?}");
                }
            },
            None => cp.wait().await?,
        };
        let (stdout, stdout_len) = stdout_task.await??;
        let (stderr, stderr_len) = stderr_task.await??;
        if stdout_len.saturating_add(stderr_len) > max_output_bytes {
            bail!("command output exceeded {max_output_bytes} bytes");
        }
        if !status.success() {
            let output = std::process::Output {
                status,
                stdout: stdout.clone(),
                stderr,
            };
            let combined_output = captured_output_lines(&self, &output);
            self.replay_captured_stderr(&combined_output);
            self.on_error(combined_output, output.status)?;
        }
        let stdout = String::from_utf8(stdout)
            .wrap_err_with(|| format!("{} produced invalid UTF-8 output", self.get_program()))?;
        Ok(stdout.trim_end().to_string())
    }

    fn execute_raw(mut self) -> Result<()> {
        // In raw mode, inherit stdio so the child can interact with the terminal
        // directly. Piped stdout/stderr would deadlock if the child produces >64KB
        // of output since nobody reads the pipes.
        if self.stdin.is_none() {
            self.stdio.stdin = Some(Stdio::inherit());
        }
        self.stdio.stdout = Some(Stdio::inherit());
        self.stdio.stderr = Some(Stdio::inherit());
        self.interrupt_on_timeout();
        let mut cp = self.spawn_with_etxtbsy_retry()?;
        let timeout_guard = self
            .timeout
            .map(|t| TimeoutGuard::new(t, cp.id(), self.interruptible()));
        let status = cp.wait()?;
        let timed_out_by_exit = timed_out_at_exit(timeout_guard.as_ref());
        if let Some(g) = &timeout_guard {
            g.cancel();
        }
        if let Some(duration) = timed_out_by_exit {
            bail!("timed out after {duration:?}");
        }
        if !status.success() {
            return self.on_error(vec![], status);
        }
        Ok(())
    }

    async fn execute_raw_async_with_cancel_check(
        mut self,
        is_cancelled: impl Fn() -> bool + Send + Sync,
    ) -> Result<()> {
        if self.stdin.is_none() {
            self.stdio.stdin = Some(Stdio::inherit());
        }
        self.stdio.stdout = Some(Stdio::inherit());
        self.stdio.stderr = Some(Stdio::inherit());
        self.interrupt_on_timeout();
        let mut cp = self.spawn_async_with_etxtbsy_retry().await?;
        let id = cp.id().unwrap_or_default();
        if is_cancelled() {
            #[cfg(unix)]
            signal_process_tree(id, nix::sys::signal::SIGINT);
            #[cfg(windows)]
            kill_process_tree(id);
        }
        let timeout_guard = self
            .timeout
            .map(|t| TimeoutGuard::new(t, id, self.interruptible()));
        let status = cp.wait().await?;
        let timed_out_by_exit = timed_out_at_exit(timeout_guard.as_ref());
        if let Some(g) = &timeout_guard {
            g.cancel();
        }
        if let Some(duration) = timed_out_by_exit {
            bail!("timed out after {duration:?}");
        }
        if !status.success() {
            return self.on_error(vec![], status);
        }
        Ok(())
    }

    /// Ask for a Ctrl+C group leader, which only the paths whose [`TimeoutGuard`]
    /// interrupts a timed-out command need.
    #[cfg(windows)]
    fn interrupt_on_timeout(&mut self) {
        self.ctrl_c_group = true;
    }

    #[cfg(unix)]
    fn interrupt_on_timeout(&mut self) {}

    /// Hand the stdio and drop behaviour held on this runner to the command about
    /// to spawn. On Windows a command with a timeout that asked to be interrupted is
    /// spawned through a Ctrl+C group leader, returned here; `self.cmd` stays as is
    /// for error messages.
    fn prepare_spawn(&mut self) -> Option<Command> {
        #[cfg(windows)]
        {
            self.ctrl_c_group &= self.timeout.is_some();
            if self.ctrl_c_group {
                let mut leader = ctrl_c_group::wrap(&self.cmd, self.inherit_env, &self.raw_args);
                self.stdio.apply(&mut leader);
                leader.kill_on_drop(self.kill_on_drop);
                return Some(leader);
            }
        }
        self.stdio.apply(&mut self.cmd);
        self.cmd.kill_on_drop(self.kill_on_drop);
        None
    }

    #[cfg(windows)]
    fn interruptible(&self) -> bool {
        self.ctrl_c_group
    }

    #[cfg(unix)]
    fn interruptible(&self) -> bool {
        true
    }

    /// Retry spawning a process if it fails with ETXTBSY (Text file busy).
    /// This can happen on Linux when executing a binary that was just written/extracted,
    /// as the file descriptor may not be fully closed yet.
    fn spawn_with_etxtbsy_retry(&mut self) -> std::io::Result<std::process::Child> {
        let mut leader = self.prepare_spawn();
        let mut attempt = 0;
        loop {
            let cmd = leader.as_mut().unwrap_or(&mut self.cmd);
            match cmd.as_std_mut().spawn() {
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
        let mut leader = self.prepare_spawn();
        let mut attempt = 0;
        loop {
            let cmd = leader.as_mut().unwrap_or(&mut self.cmd);
            match cmd.spawn() {
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
            let initial_program = std::path::PathBuf::from(self.cmd.as_std().get_program());
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
            // Rules naming a path that does not exist yet get dropped, and the
            // task is then denied. Say so here: pre_exec runs post-fork, where
            // the logger is not available.
            if sandbox.effective_deny_read() || sandbox.effective_deny_write() {
                sandbox.warn_missing_allow_paths();
            }
            // Use pre_exec to apply Landlock/seccomp in the child process
            // before it execs the target program. This avoids restricting the mise process.
            let sandbox = sandbox.clone();
            unsafe {
                self.cmd.as_std_mut().pre_exec(move || {
                    if sandbox.effective_deny_read()
                        || sandbox.effective_deny_write()
                        || sandbox.deny_process
                    {
                        crate::sandbox::landlock_apply(&sandbox, &initial_program)
                            .map_err(|e| std::io::Error::other(e.to_string()))?;
                    }
                    if sandbox.effective_deny_net() || sandbox.deny_process {
                        crate::sandbox::seccomp_apply(
                            sandbox.effective_deny_net(),
                            sandbox.deny_process,
                        )
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
            let profile =
                crate::sandbox::macos_generate_profile(&sandbox, std::path::Path::new(&program))
                    .await;

            let mut new_cmd = Command::new("sandbox-exec");
            new_cmd.arg("-p").arg(&profile).arg("--").arg(&program);
            for arg in &args {
                new_cmd.arg(arg);
            }
            // Match CmdLineRunner::new() defaults for stdio.
            // execute() reads from piped stdout/stderr; execute_raw() overrides to inherit.
            self.stdio = PendingStdio::defaults();
            if self.stdin.is_some() {
                self.stdio.stdin = Some(Stdio::piped());
            }
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
                pr.set_process_output(line)
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
        if self.stderr_as_stdout {
            if let Some(pr) = self
                .pr
                .or(self.pr_arc.as_ref().map(|arc| arc.as_ref().as_ref()))
            {
                if !line.trim().is_empty() {
                    pr.set_process_output(line);
                }
            } else {
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{line}");
            }
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
                if self.on_stdout.is_none() && !pr.shows_process_output() {
                    // Stdout was hidden behind the progress indicator
                    // (pr.set_process_output) so replay it on failure. Only replay
                    // stdout — stderr was already printed during execution
                    // via pr.println. Reporters that already showed stdout as it
                    // arrived would only duplicate it here.
                    let stdout_only: String = output
                        .iter()
                        .filter(|(_, source)| matches!(source, OutputSource::Stdout))
                        .map(|(line, _)| line.as_str())
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
        Err(ScriptFailed(
            self.get_program(),
            Some(status),
            stderr_tail_for_error(&output),
        ))?
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

/// Number of threads spinning in [`raw_write_lock_blocking`].
///
/// These helpers poll `try_write`/`try_read` rather than awaiting, because their
/// callers are sync. `try_write` never registers as a waiting writer, so without
/// this counter a steady stream of readers starves them: `try_read` succeeds
/// whenever no writer *currently holds* the lock, which is every time the writer
/// is between polls. Readers check this first and yield to a pending writer, which
/// is what makes an exclusive acquisition finish in bounded time.
static RAW_WRITERS_WAITING: AtomicUsize = AtomicUsize::new(0);

/// Decrements [`RAW_WRITERS_WAITING`] however the writer leaves its loop.
struct RawWriterWaiting;

impl RawWriterWaiting {
    fn new() -> Self {
        RAW_WRITERS_WAITING.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for RawWriterWaiting {
    fn drop(&mut self) {
        RAW_WRITERS_WAITING.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Async counterpart of [`raw_read_lock_blocking`].
///
/// Awaiting `RAW_LOCK` directly would queue behind an *async* writer but not a sync
/// one, whose `try_write` polls tokio never sees. Consulting the counter makes both
/// kinds of writer visible to both kinds of reader.
///
/// Acquires first and re-checks, rather than checking then acquiring: a writer that
/// registers during the gap between those two steps would otherwise be bypassed by a
/// reader that had already passed the check. Releasing the guard on that path leaves
/// only the case where the reader held the lock before the writer arrived, which no
/// amount of gating can avoid — an `RwLock` writer always waits for current readers.
pub async fn raw_read_lock() -> tokio::sync::RwLockReadGuard<'static, ()> {
    loop {
        let guard = RAW_LOCK.read().await;
        if RAW_WRITERS_WAITING.load(Ordering::Acquire) == 0 {
            return guard;
        }
        drop(guard);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Async counterpart of [`raw_write_lock_blocking`]. Registers as a waiting writer so
/// sync readers yield to it, then queues on the lock as usual.
pub async fn raw_write_lock() -> tokio::sync::RwLockWriteGuard<'static, ()> {
    let _waiting = RawWriterWaiting::new();
    RAW_LOCK.write().await
}

/// Take the shared side of [`RAW_LOCK`]. Held while an ordinary command runs, so a
/// `--raw` command (or vfox's `cmd.stream`) waits for it before taking the terminal.
///
/// Yields to a writer already waiting, so exclusive acquisition cannot be starved.
pub fn raw_read_lock_blocking() -> tokio::sync::RwLockReadGuard<'static, ()> {
    loop {
        if let Ok(guard) = RAW_LOCK.try_read() {
            // Acquire-then-verify, for the reason given on `raw_read_lock`.
            if RAW_WRITERS_WAITING.load(Ordering::Acquire) == 0 {
                return guard;
            }
            drop(guard);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Take the exclusive side of [`RAW_LOCK`], blocking until no other command holds
/// it. Used by `--raw` commands and by vfox's `cmd.stream`, which needs the same
/// exclusivity so an interactive plugin child owns the terminal. (#13254)
pub fn raw_write_lock_blocking() -> tokio::sync::RwLockWriteGuard<'static, ()> {
    let _waiting = RawWriterWaiting::new();
    loop {
        if let Ok(guard) = RAW_LOCK.try_write() {
            return guard;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// A command line with registered secrets removed.
///
/// This rendering is not only for logs. `execute` debug-logs it, but it is also
/// formatted into `failed to execute command: {self}`, and that error is
/// printed by the top-level handler, which applies no redaction of its own. An
/// argument can legitimately carry a credential (a gem `source` authenticates
/// as basic-auth userinfo on the URL), so redacting where the string is built
/// covers every consumer instead of asking each one to remember.
fn display_command(runner: &CmdLineRunner<'_>) -> String {
    let args = runner.get_args().join(" ");
    crate::redactions::redact_global(&format!("{} {args}", runner.get_program()))
}

impl Display for CmdLineRunner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", display_command(self))
    }
}

impl Debug for CmdLineRunner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", display_command(self))
    }
}

/// Tracks whether an output line came from stdout or stderr,
/// so on_error can decide which lines need replaying.
enum OutputSource {
    Stdout,
    Stderr,
}

/// The last thing the child said on stderr, for the error that ends the run.
///
/// One line: an error is rendered on a single row in places like the install
/// summary, and the whole stream was already streamed to the reporter. Callers
/// that route stderr to stdout (`stderr_as_stdout`) replay their output in full
/// instead, so nothing here is the only copy.
fn stderr_tail_for_error(output: &[(String, OutputSource)]) -> Option<String> {
    let line = output
        .iter()
        .rev()
        .find(|(line, source)| matches!(source, OutputSource::Stderr) && !line.trim().is_empty())
        .map(|(line, _)| line.trim())?;
    // By character, not by byte: a diagnostic in a non-ASCII locale would
    // otherwise lose two thirds of its length to UTF-8 encoding.
    let end = line
        .char_indices()
        .nth(STDERR_TAIL_MAX_CHARS)
        .map_or(line.len(), |(index, _)| index);
    if end < line.len() {
        Some(format!("{}…", &line[..end]))
    } else {
        Some(line.to_string())
    }
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
    #[cfg(not(target_os = "windows"))]
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

#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, windows))]
mod windows_tests;
