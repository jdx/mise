//! Local-only history: `mode = "track-local"` entries, saved in this
//! machine's own history store, which no setup repository ever reaches.
//!
//! The store is a complete history store of its own, under
//! `$MISE_STATE_DIR/history-local`. A command works on it by running in the
//! local scope: the same mise with [`ENV`] set, where history's [`root`] is
//! that directory. Everything else mise keeps in its state directory (trust
//! decisions, for one) stays where it is. In that scope only local entries
//! are tracked and nothing is ever synchronized.
//! Outside it local entries are never tracked
//! ([`super::tracked::TrackedSet::keep_local_out`]), so nothing local can
//! reach the shared history, its manifest, or an origin.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::Command;

use eyre::{Result, WrapErr, ensure};

/// Set in the local scope.
pub const ENV: &str = "__MISE_HISTORY_LOCAL";

/// A local operation enclosing an external command, without putting that
/// command's ordinary mise operations in the local history.
pub const OPERATION_ENV: &str = "__MISE_LOCAL_CAPTURE_PARENT";

/// Whether this process works on the local-only history.
pub fn active() -> bool {
    std::env::var_os(ENV).is_some()
}

/// The directory the local-only store lives under.
pub fn local_root() -> PathBuf {
    crate::dirs::STATE.join("history-local")
}

/// The directory this process's history lives under: the state directory,
/// or [`local_root`] in the local scope.
pub fn root() -> PathBuf {
    if active() {
        local_root()
    } else {
        crate::dirs::STATE.to_path_buf()
    }
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
        .env(ENV, "1")
        .env_remove(super::scope::ENV_VAR);
    if let Some(operation) = std::env::var_os(OPERATION_ENV) {
        command.env(super::scope::ENV_VAR, operation);
    }
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

/// A command the local scope runs on the user's behalf is outside it.
pub fn restore_env(command: &mut Command) {
    command.env_remove(ENV);
    if active() {
        if let Some(operation) = std::env::var_os(super::scope::ENV_VAR) {
            command.env(OPERATION_ENV, operation);
        }
        command.env_remove(super::scope::ENV_VAR);
    }
}
