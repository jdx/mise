//! Local-only history: `mode = "track-local"` entries, saved in this
//! machine's own history store, which no setup repository ever reaches.
//!
//! The store is a complete history store of its own, under
//! `$MISE_STATE_DIR/history-local`. A command works on it by running in the
//! local scope: the same mise, with `MISE_STATE_DIR` pointed at that
//! directory and [`ENV`] holding the state directory it came from. In that
//! scope only local entries are tracked and nothing is ever synchronized.
//! Outside it local entries are never tracked
//! ([`super::tracked::TrackedSet::keep_local_out`]), so nothing local can
//! reach the shared history, its manifest, or an origin.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use eyre::{Result, WrapErr, ensure};

/// Set in the local scope, to the state directory it came from.
pub const ENV: &str = "__MISE_HISTORY_LOCAL";

/// Whether this process works on the local-only history.
pub fn active() -> bool {
    std::env::var_os(ENV).is_some()
}

/// The state directory of the local-only store, seen from outside it.
pub fn state_dir() -> PathBuf {
    crate::dirs::STATE.join("history-local")
}

/// `mise <args>` in the local scope. An operation this process records is
/// another history's, so the child never attaches to it.
pub fn command<I, S>(args: I) -> Result<Command>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(args)
        .env("MISE_STATE_DIR", state_dir())
        .env(ENV, *crate::dirs::STATE)
        .env_remove(super::scope::ENV_VAR);
    Ok(command)
}

/// Runs `mise <args>` in the local scope.
pub fn run<I, S>(args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect();
    let status = command(&args)?
        .status()
        .wrap_err("running the local-only history")?;
    ensure!(
        status.success(),
        "local-only history: `mise {}` failed",
        args.iter()
            .map(|arg| arg.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    Ok(())
}

/// A command the local scope runs on the user's behalf gets the
/// environment the scope came from.
pub fn restore_env(command: &mut Command) {
    if let Some(original) = std::env::var_os(ENV) {
        command.env("MISE_STATE_DIR", original).env_remove(ENV);
    }
}
