//! Hand-off of OTLP log export between nested `mise run` invocations.
//!
//! A task's stdout/stderr is piped through this process, so when that task
//! shells out to `mise run`, the inner run's output flows up through our pipe
//! too. Both processes would then export the same lines: the inner run
//! attributed to its own task spans, and this one attributed to the outer
//! task's span.
//!
//! The inner run is the more precise reporter, so it wins. Before spawning a
//! task we hand it a claim path; a nested `mise run` that exports its own task
//! logs creates that file for as long as it lives, and we skip forwarding
//! while it exists. Sequential nested runs hand the stream back and forth, so
//! output the outer task writes itself is still exported by us:
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

/// Env var naming the claim file a nested `mise run` creates while it is
/// exporting its own task logs. Set on task subprocesses only when this
/// process is actually capturing their output.
pub const LOG_CLAIM_ENV: &str = "MISE_TASK_OTEL_LOG_CLAIM";

/// Parent side: the claim file handed to a task's subprocess.
#[derive(Clone, Debug)]
pub struct LogClaimWatcher {
    path: Arc<PathBuf>,
}

impl LogClaimWatcher {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path: Arc::new(path),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a live nested `mise run` currently owns the stream.
    ///
    /// Checked per forwarded line, which is what lets the outer task resume
    /// exporting the moment the nested run finishes. When no claim exists —
    /// the common case — this costs one failed `open`, cheap next to building
    /// and queueing an OTLP record.
    ///
    /// A claim whose owner is gone is treated as released and cleaned up.
    /// A nested run killed with `SIGKILL` never runs its destructor, so
    /// without this the outer task would stop exporting for the rest of the
    /// command rather than for the rest of the nested run.
    pub fn claimed(&self) -> bool {
        let Ok(owner) = std::fs::read_to_string(self.path()) else {
            return false;
        };
        match owner.trim().parse::<u32>() {
            Ok(pid) if process_is_alive(pid) => true,
            Ok(pid) => {
                trace!("otel: reclaiming log stream from dead pid {pid}");
                let _ = std::fs::remove_file(self.path());
                false
            }
            // Unparseable claim: treat the stream as ours. Erring this way
            // risks duplicating a line; erring the other way risks dropping
            // every remaining line.
            Err(_) => false,
        }
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
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: OpenProcess/CloseHandle are called with a valid pid and the
    // handle is closed exactly once on the success path.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            // ERROR_ACCESS_DENIED means it exists but isn't ours to open.
            return windows_sys::Win32::Foundation::GetLastError()
                == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
        }
        CloseHandle(handle);
        true
    }
}

/// Child side: registers this run as the owner of the inherited stream for as
/// long as it lives. Released on drop.
#[derive(Debug)]
pub struct LogClaim {
    path: PathBuf,
    pid: String,
}

impl LogClaim {
    /// Claim the stream if an ancestor `mise` handed us a claim path.
    ///
    /// Only call this when we will actually export our own task logs —
    /// claiming without exporting would drop the lines on both sides.
    pub fn acquire() -> Option<Self> {
        let path = PathBuf::from(std::env::var_os(LOG_CLAIM_ENV)?);
        let pid = std::process::id().to_string();
        // Write-then-rename so the ancestor reading this concurrently sees
        // either no claim or a complete one, never a half-written pid.
        let tmp = path.with_extension(format!("tmp.{pid}"));
        if let Err(err) = std::fs::write(&tmp, &pid).and_then(|()| std::fs::rename(&tmp, &path)) {
            // Not fatal: without the claim the ancestor keeps forwarding, so
            // the lines are duplicated rather than lost.
            debug!(
                "otel: failed to claim log stream at {}: {err}",
                path.display()
            );
            let _ = std::fs::remove_file(&tmp);
            return None;
        }
        trace!("otel: claimed log stream at {}", path.display());
        Some(Self { path, pid })
    }
}

impl Drop for LogClaim {
    fn drop(&mut self) {
        // Only release if we're still the owner. Two nested runs sharing one
        // parent task (`mise run a & mise run b &`) both write the file; the
        // first to exit must not hand the stream back while the other is
        // still reporting.
        match std::fs::read_to_string(&self.path) {
            Ok(owner) if owner.trim() == self.pid => {
                let _ = std::fs::remove_file(&self.path);
            }
            _ => {}
        }
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

    #[test]
    fn watcher_reports_claim_file_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().join("claim"));
        assert!(!watcher.claimed(), "unclaimed before anyone writes");

        std::fs::write(watcher.path(), live_pid().to_string()).unwrap();
        assert!(watcher.claimed());

        std::fs::remove_file(watcher.path()).unwrap();
        assert!(!watcher.claimed(), "released once the file is gone");
    }

    #[test]
    fn claim_releases_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claim");
        let watcher = LogClaimWatcher::new(path.clone());

        let pid = live_pid();
        let claim = LogClaim {
            path: path.clone(),
            pid: pid.to_string(),
        };
        std::fs::write(&path, pid.to_string()).unwrap();
        assert!(watcher.claimed());

        drop(claim);
        assert!(!watcher.claimed());
    }

    #[test]
    fn claim_does_not_release_a_stream_another_run_took_over() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claim");
        let watcher = LogClaimWatcher::new(path.clone());

        let first = LogClaim {
            path: path.clone(),
            pid: dead_pid().to_string(),
        };
        // A concurrent nested run claims the same stream after us.
        std::fs::write(&path, live_pid().to_string()).unwrap();

        drop(first);
        assert!(
            watcher.claimed(),
            "the run that still owns the stream keeps it"
        );
    }

    #[test]
    fn stale_claim_from_a_dead_process_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().join("claim"));
        // A nested run killed with SIGKILL never runs its destructor, so its
        // claim file outlives it.
        std::fs::write(watcher.path(), dead_pid().to_string()).unwrap();

        assert!(
            !watcher.claimed(),
            "a claim whose owner is gone must not suppress the outer task"
        );
        assert!(
            !watcher.path().exists(),
            "the stale claim should be cleaned up so later checks stay cheap"
        );
    }

    #[test]
    fn unreadable_claim_does_not_suppress_the_outer_task() {
        let dir = tempfile::tempdir().unwrap();
        let watcher = LogClaimWatcher::new(dir.path().join("claim"));
        std::fs::write(watcher.path(), "not-a-pid").unwrap();
        assert!(!watcher.claimed());
    }

    #[test]
    fn acquire_publishes_a_complete_claim() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claim");
        // SAFETY: single-threaded test; the var is removed before returning.
        unsafe { std::env::set_var(LOG_CLAIM_ENV, &path) };
        let claim = LogClaim::acquire().expect("claim should be acquired");
        unsafe { std::env::remove_var(LOG_CLAIM_ENV) };

        let watcher = LogClaimWatcher::new(path.clone());
        assert!(watcher.claimed());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            std::process::id().to_string()
        );
        // No temp file left behind by the atomic publish.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "claim")
            .collect();
        assert!(leftovers.is_empty(), "unexpected leftovers: {leftovers:?}");

        drop(claim);
        assert!(!watcher.claimed());
    }
}
