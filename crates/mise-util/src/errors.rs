//! Errors from running external processes.

use std::process::ExitStatus;

use eyre::Report;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("{} exited with non-zero status: {}{}", .0, render_exit_status(.1), render_stderr_tail(.2))]
    ScriptFailed(String, Option<ExitStatus>, Option<String>),
    #[error("task interrupted before process start")]
    TaskInterrupted,
}

fn render_exit_status(exit_status: &Option<ExitStatus>) -> String {
    if let Some(code) = exit_status.and_then(|s| s.code()) {
        return format!("exit code {code}");
    }
    // No code means the process was signalled, and the signal is right there.
    // Reporting "no exit status" threw it away and left nothing to act on.
    #[cfg(unix)]
    if let Some(signal) = exit_status.and_then(|s| {
        use std::os::unix::process::ExitStatusExt;
        s.signal()
    }) {
        return match nix::sys::signal::Signal::try_from(signal) {
            Ok(signal) => format!("killed by {signal}"),
            Err(_) => format!("killed by signal {signal}"),
        };
    }
    "no exit status".into()
}

/// The child's own last word, appended to the bare exit status.
///
/// A command that fails during an install has already written the reason to
/// stderr — `error while loading shared libraries: libncurses.so.6` — and the
/// progress reporter prints it as it arrives. But the error that ends the run
/// carried only `exit code 127`, and under `--quiet` the live output never
/// appeared at all, so the one line that explains the failure was gone by the
/// time anyone read it.
///
/// Kept to a single line so every consumer that renders an error on one row —
/// the install summary's `✗ … · failed: …` — stays on one row.
fn render_stderr_tail(tail: &Option<String>) -> String {
    match tail {
        Some(tail) if !tail.trim().is_empty() => format!("; last stderr: {tail}"),
        _ => String::new(),
    }
}

impl ProcessError {
    pub fn get_exit_status(err: &Report) -> Option<i32> {
        if let Some(ProcessError::ScriptFailed(_, Some(status), _)) =
            err.downcast_ref::<ProcessError>()
        {
            status.code()
        } else {
            None
        }
    }

    /// Whether the command was ended by the SIGTERM mise sends to a failed
    /// task's siblings, rather than exiting or crashing on its own.
    #[cfg(unix)]
    pub fn is_killed_by_signal(err: &Report) -> bool {
        use std::os::unix::process::ExitStatusExt;

        err.downcast_ref::<ProcessError>().is_some_and(|err| {
            matches!(
                err,
                ProcessError::ScriptFailed(_, Some(status), _)
                    if status.signal() == Some(nix::sys::signal::SIGTERM as i32)
            )
        })
    }

    /// Windows reports a terminated process as an ordinary exit code, so a
    /// sibling mise stopped can't be told apart from one that failed.
    #[cfg(windows)]
    pub fn is_killed_by_signal(_err: &Report) -> bool {
        true
    }

    #[cfg(unix)]
    pub fn is_sigint(err: &Report) -> bool {
        use std::os::unix::process::ExitStatusExt;

        err.downcast_ref::<ProcessError>().is_some_and(|err| {
            matches!(
                err,
                ProcessError::ScriptFailed(_, Some(status), _)
                    if status.signal() == Some(nix::sys::signal::SIGINT as i32)
            )
        })
    }

    /// Windows has no signals. A process ended by a console control event
    /// exits with `STATUS_CONTROL_C_EXIT`, which reports what
    /// `signal() == SIGINT` reports on Unix: the terminal interrupted this
    /// child, so its task stops without that counting as a failure.
    #[cfg(windows)]
    pub fn is_sigint(err: &Report) -> bool {
        use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;

        err.downcast_ref::<ProcessError>().is_some_and(|err| {
            matches!(
                err,
                ProcessError::ScriptFailed(_, Some(status), _)
                    if status.code() == Some(STATUS_CONTROL_C_EXIT)
            )
        })
    }

    #[cfg(not(any(unix, windows)))]
    pub fn is_sigint(_err: &Report) -> bool {
        false
    }

    pub fn is_task_interrupted_before_start(err: &Report) -> bool {
        matches!(
            err.downcast_ref::<ProcessError>(),
            Some(ProcessError::TaskInterrupted)
        )
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::os::windows::process::ExitStatusExt;
    use windows_sys::Win32::Foundation::STATUS_CONTROL_C_EXIT;

    #[test]
    fn detects_a_console_interrupt() {
        let status = ExitStatus::from_raw(STATUS_CONTROL_C_EXIT as u32);
        let err = Report::new(ProcessError::ScriptFailed("cmd".into(), Some(status), None));

        assert!(ProcessError::is_sigint(&err));
    }

    #[test]
    fn does_not_treat_an_ordinary_failure_as_an_interrupt() {
        let status = ExitStatus::from_raw(1);
        let err = Report::new(ProcessError::ScriptFailed("cmd".into(), Some(status), None));

        assert!(!ProcessError::is_sigint(&err));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    #[test]
    fn detects_sigint_script_failure() {
        let status = ExitStatus::from_raw(nix::sys::signal::SIGINT as i32);
        let err = Report::new(ProcessError::ScriptFailed("sh".into(), Some(status), None));

        assert!(ProcessError::is_sigint(&err));
    }

    #[test]
    fn does_not_treat_exit_code_as_sigint() {
        let status = ExitStatus::from_raw(2 << 8);
        let err = Report::new(ProcessError::ScriptFailed("sh".into(), Some(status), None));

        assert!(!ProcessError::is_sigint(&err));
    }

    #[test]
    fn renders_the_signal_that_killed_the_process() {
        // "no exit status" threw away the one fact that explains the failure.
        let status = ExitStatus::from_raw(nix::sys::signal::SIGINT as i32);
        assert_eq!(render_exit_status(&Some(status)), "killed by SIGINT");

        let status = ExitStatus::from_raw(nix::sys::signal::SIGTERM as i32);
        assert_eq!(render_exit_status(&Some(status)), "killed by SIGTERM");
    }

    #[test]
    fn renders_an_exit_code_unchanged() {
        let status = ExitStatus::from_raw(2 << 8);
        assert_eq!(render_exit_status(&Some(status)), "exit code 2");
        assert_eq!(render_exit_status(&None), "no exit status");
    }

    #[test]
    fn detects_interruption_before_process_start() {
        let err = Report::new(ProcessError::TaskInterrupted);

        assert!(ProcessError::is_task_interrupted_before_start(&err));
    }
}
