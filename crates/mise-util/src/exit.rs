//! Exit statuses requested from deep inside a command, applied once it has unwound.

use crate::cmd::CmdLineRunner;
use eyre::Report;
#[cfg(unix)]
use nix::sys::signal::SIGTERM;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug)]
struct ExitRequest {
    code: i32,
}

impl Display for ExitRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "exit with status {}", self.code)
    }
}

impl Error for ExitRequest {}

/// Request a process status without terminating before callers can unwind.
pub fn request(code: i32) -> Report {
    Report::new(ExitRequest { code })
}

pub fn requested_exit_code(err: &Report) -> Option<i32> {
    err.downcast_ref::<ExitRequest>().map(|exit| exit.code)
}

/// Whether `err` is the error [`request`] makes. An eyre hook can skip its work
/// for one: it carries a status, not a failure, and is never printed.
pub fn is_exit_request(err: &(dyn Error + 'static)) -> bool {
    err.is::<ExitRequest>()
}

pub fn kill_all() {
    #[cfg(unix)]
    CmdLineRunner::kill_all(SIGTERM);

    #[cfg(windows)]
    CmdLineRunner::kill_all();
}

/// Convert a requested process status after command scopes have unwound.
pub fn status(code: i32) -> std::process::ExitCode {
    debug!("exiting with code: {code}");
    std::process::ExitCode::from(code as u8)
}

/// End the process with a whole 32-bit Windows status, for a process that only
/// relays another's, such as a service launcher. [`status`] narrows to a byte, and
/// the stable `ExitCode` has no wider constructor yet (`ExitCodeExt::from_raw` is
/// unstable, rust-lang/rust#111688), so this is the Win32 call itself. Like
/// `std::process::exit` it skips destructors: call it only with nothing to unwind.
#[cfg(windows)]
pub fn exit_with_windows_status(code: u32) -> ! {
    // `std::process::exit` flushes stdout before its own `ExitProcess`.
    let _ = std::io::Write::flush(&mut std::io::stdout());
    unsafe { windows_sys::Win32::System::Threading::ExitProcess(code) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_requested_exit_code_through_context() {
        let err = request(42).wrap_err("context");
        assert_eq!(requested_exit_code(&err), Some(42));
    }

    #[test]
    fn tells_exit_requests_from_other_errors() {
        let exit = ExitRequest { code: 1 };
        assert!(is_exit_request(&exit));
        let other = std::io::Error::other("failure");
        assert!(!is_exit_request(&other));
    }
}
