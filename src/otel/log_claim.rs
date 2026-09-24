//! Hand-off of OTLP log export between nested `mise run` invocations.
//!
//! A task's stdout/stderr is piped through this process, so when that task
//! shells out to `mise run`, the inner run's output flows up through our pipe
//! too. Both processes would then export the same lines: the inner run
//! attributed to its own task spans, and this one attributed to the outer
//! task's span.
//!
//! The inner run is the more precise reporter, so it wins. Before spawning a
//! task we hand it a claim directory; a nested `mise run` that exports its own
//! task logs creates a file named after its pid there for as long as it lives,
//! and we skip forwarding while any such file belongs to a live process. Each
//! nested run owns its own file, so concurrent nested runs
//! (`mise run a & mise run b &`) keep the stream claimed until the last one
//! exits. Sequential nested runs hand the stream back and forth, so output the
//! outer task writes itself is still exported by us:
//!
//! ```text
//! run = "echo building; mise run inner; echo done"
//!         └─ outer span    └─ inner span   └─ outer span
//! ```
//!
//! Terminal output is untouched — this only gates the OTLP hooks, which are
//! separate from the callbacks that print each line.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Env var naming the claim directory a nested `mise run` registers in while
/// it is exporting its own task logs. Set on task subprocesses only when this
/// process is actually capturing their output.
pub(crate) const LOG_CLAIM_ENV: &str = "MISE_TASK_OTEL_LOG_CLAIM";

/// Parent side: the claim directory handed to a task's subprocess.
#[derive(Clone, Debug)]
pub(crate) struct LogClaimWatcher {
    dir: Arc<PathBuf>,
}

impl LogClaimWatcher {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir: Arc::new(dir) }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.dir
    }

    /// Whether a live nested `mise run` currently owns the stream.
    ///
    /// Checked per forwarded line, which is what lets the outer task resume
    /// exporting the moment the nested run finishes. When no claim exists —
    /// the common case — this costs one read of an empty directory, cheap
    /// next to building and queueing an OTLP record.
    ///
    /// A claim whose owner is gone is treated as released and cleaned up.
    /// A nested run killed with `SIGKILL` never runs its destructor, so
    /// without this the outer task would stop exporting for the rest of the
    /// command rather than for the rest of the nested run. Entries that
    /// aren't pids are ignored: erring this way risks duplicating a line;
    /// erring the other way risks dropping every remaining line.
    pub(crate) fn claimed(&self) -> bool {
        let Ok(entries) = std::fs::read_dir(self.path()) else {
            return false;
        };
        let mut claimed = false;
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if process_is_alive(pid) {
                claimed = true;
            } else {
                trace!("otel: reclaiming log stream from dead pid {pid}");
                let _ = std::fs::remove_file(entry.path());
            }
        }
        claimed
    }
}

/// Whether a process is still running.
///
/// Only ever asked about a claim written by a descendant of this process, so
/// PID reuse would need a wrap-around inside one task's lifetime.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    // Signal 0 checks for existence without delivering anything. `EPERM`
    // means the process is there but not ours to signal — still alive.
    if pid == 0 {
        // kill(0, ...) addresses our own process group, which would always
        // report alive. A real claim never contains 0.
        return false;
    }
    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None) {
        Ok(()) => true,
        Err(nix::errno::Errno::EPERM) => true,
        Err(_) => false,
    }
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess/GetExitCodeProcess/CloseHandle are called with a
    // valid pid and handle, and the handle is closed exactly once on the
    // success path.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            // ERROR_ACCESS_DENIED means it exists but isn't ours to open.
            return windows_sys::Win32::Foundation::GetLastError()
                == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        }
        // An exited process stays openable while anything holds a handle to
        // it, so opening it proves nothing; its exit code says whether it
        // is still running.
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok == 0 || code == STILL_ACTIVE as u32
    }
}

/// Child side: registers this run as an owner of the inherited stream for as
/// long as it lives. Released on drop.
#[derive(Debug)]
pub(crate) struct LogClaim {
    path: PathBuf,
}

impl LogClaim {
    /// Claim the stream if an ancestor `mise` handed us a claim directory.
    ///
    /// Only call this when we will actually export our own task logs —
    /// claiming without exporting would drop the lines on both sides.
    pub(crate) fn acquire() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os(LOG_CLAIM_ENV)?);
        let path = dir.join(std::process::id().to_string());
        // The file's existence is the claim, so creating it empty is atomic
        // from the ancestor's point of view.
        if let Err(err) = std::fs::File::create(&path) {
            // Not fatal: without the claim the ancestor keeps forwarding, so
            // the lines are duplicated rather than lost.
            debug!(
                "otel: failed to claim log stream at {}: {err}",
                path.display()
            );
            return None;
        }
        trace!("otel: claimed log stream at {}", path.display());
        Some(Self { path })
    }
}

impl Drop for LogClaim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pid that is definitely gone: spawn a process and reap it.
    fn dead_pid() -> u32 {
        #[cfg(unix)]
        let mut child = std::process::Command::new("true").spawn().unwrap();
        #[cfg(windows)]
        let mut child = std::process::Command::new("cmd")
            .args(["/c", "exit"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    fn live_pid() -> u32 {
        std::process::id()
    }

    fn claim_as(dir: &Path, pid: u32) -> LogClaim {
        let path = dir.join(pid.to_string());
        std::fs::File::create(&path).unwrap();
        LogClaim { path }
    }

    #[test]
    fn watcher_reports_claim_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().to_path_buf());
        assert!(!watcher.claimed(), "unclaimed before anyone registers");

        let claim = claim_as(dir.path(), live_pid());
        assert!(watcher.claimed());

        drop(claim);
        assert!(
            !watcher.claimed(),
            "released once the owner drops its claim"
        );
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_claims_hold_the_stream_until_the_last_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().to_path_buf());
        // `mise run a & mise run b &`: two live nested runs under one task.
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let a = claim_as(dir.path(), live_pid());
        let b = claim_as(dir.path(), other.id());

        drop(a);
        assert!(watcher.claimed(), "b is still exporting its own lines");
        drop(b);
        assert!(!watcher.claimed());
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[test]
    fn stale_claim_from_a_dead_process_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().to_path_buf());
        // A nested run killed with SIGKILL never runs its destructor, so its
        // claim outlives it.
        let stale = dir.path().join(dead_pid().to_string());
        std::fs::File::create(&stale).unwrap();

        assert!(
            !watcher.claimed(),
            "a claim whose owner is gone must not suppress the outer task"
        );
        assert!(
            !stale.exists(),
            "the stale claim should be cleaned up so later checks stay cheap"
        );
    }

    #[test]
    fn unrecognised_entries_do_not_suppress_the_outer_task() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().to_path_buf());
        std::fs::write(dir.path().join("not-a-pid"), "").unwrap();
        assert!(!watcher.claimed());
    }

    #[test]
    fn acquire_registers_this_process() {
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: single-threaded test; the var is removed before returning.
        unsafe { std::env::set_var(LOG_CLAIM_ENV, dir.path()) };
        let claim = LogClaim::acquire();
        unsafe { std::env::remove_var(LOG_CLAIM_ENV) };
        let claim = claim.expect("claim should be acquired");

        let watcher = LogClaimWatcher::new(dir.path().to_path_buf());
        assert!(watcher.claimed());
        assert!(dir.path().join(std::process::id().to_string()).exists());

        drop(claim);
        assert!(!watcher.claimed());
    }
}
