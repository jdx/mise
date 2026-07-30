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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_requested_exit_code_through_context() {
        let err = request(42).wrap_err("context");
        assert_eq!(requested_exit_code(&err), Some(42));
    }
}
