//! A command the user types on an interactive terminal in an untrusted project trusts that
//! project without the "Trust them?" prompt.
//!
//! Only [`super::trust_check`] reaches this, and only where it would otherwise prompt: safe mode,
//! `hook-env`, strict trust, explicit-trust callers and ignored paths have already been decided.
//! What is left to decide here is whether a person typed this command in this project, which
//! everything other than a terminal on all three descriptors, run by the owner of the config,
//! fails to show.

use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config::{Settings, settings};
use crate::env;
use crate::ui::style;

/// What [`consents`] decides from, read by [`read_signals`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Signals {
    pub(super) stdin_tty: bool,
    pub(super) stdout_tty: bool,
    pub(super) stderr_tty: bool,
    /// Paranoid mode's `config` category, or settings that could not be read.
    pub(super) paranoid_config: bool,
    /// mise runs as a shim, or as the `mise x` a shim starts.
    pub(super) shim: bool,
    /// The shell ran mise for its own integration, not because the user typed a mise command:
    /// TAB completion and the usage spec it reads, or the command-not-found handler after a
    /// mistyped command.
    pub(super) run_by_shell: bool,
    /// The config file and its trust root belong to the user running mise.
    pub(super) owned_by_user: bool,
}

/// Whether the command counts as the user's consent to trust the project.
///
/// stdout is a terminal too, not only stdin and stderr: `$(mise ...)` in a prompt segment and
/// `eval "$(mise env)"` in an rc file run with the terminal on stdin and stderr, but nobody typed
/// them in this project.
pub(super) fn consents(signals: Signals) -> bool {
    signals.stdin_tty
        && signals.stdout_tty
        && signals.stderr_tty
        && !signals.paranoid_config
        && !signals.shim
        && !signals.run_by_shell
        && signals.owned_by_user
}

/// The commands a shell integration runs without the user typing a mise command: those that
/// answer TAB completion or print the spec it reads, and `hook-not-found`, which the shell's
/// command-not-found handler runs with the terminal on all three descriptors when the user
/// mistypes any command. Neither TAB nor a typo is consent. `hook-env`, which the prompt hook
/// runs, is not listed because `trust_check` never asks for it and so never reaches this.
const SHELL_RUN_COMMANDS: &[&str] = &[
    "__complete_word__",
    "__usage_complete_word",
    "complete",
    "completion",
    "completions",
    "hook-not-found",
    "usage",
];

/// Whether the shell ran mise for its own integration rather than for a command the user entered:
/// completion, including the `mise tasks --usage` it starts with `__USAGE` set, or one of
/// [`SHELL_RUN_COMMANDS`]. A shim's arguments belong to its tool, so they never name one.
pub(super) fn run_by_shell() -> bool {
    if env::__USAGE.is_some() {
        return true;
    }
    if *env::IS_RUNNING_AS_SHIM {
        return false;
    }
    env::ARGS
        .read()
        .unwrap()
        .get(1)
        .is_some_and(|command| SHELL_RUN_COMMANDS.contains(&command.as_str()))
}

fn read_signals(path: &Path, config_root: &Path) -> Signals {
    Signals {
        stdin_tty: std::io::stdin().is_terminal(),
        stdout_tty: console::user_attended(),
        stderr_tty: console::user_attended_stderr(),
        // Settings that are not loaded yet cannot say paranoid mode is off.
        paranoid_config: !settings::is_loaded()
            || !Settings::try_get().is_ok_and(|settings| !settings.paranoid.config()),
        // A shim's arguments belong to its tool, and a shim that is not mise itself runs
        // `mise x` with `__MISE_SHIM_PATH` naming itself: the user typed the tool's name.
        shim: *env::IS_RUNNING_AS_SHIM || env::MISE_SHIM_PATH.read().unwrap().is_some(),
        run_by_shell: run_by_shell(),
        owned_by_user: owned_by_user(path) && owned_by_user(config_root),
    }
}

/// Whether `path` and, when it is a symlink, what it points at both belong to the effective user.
/// The link is checked so a link another account planted to the user's own file does not pass,
/// and the target so a link the user made to another account's file does not either.
#[cfg(unix)]
fn owned_by_user(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = nix::unistd::geteuid().as_raw();
    let owned = |meta: std::io::Result<std::fs::Metadata>| meta.is_ok_and(|m| m.uid() == uid);
    owned(path.symlink_metadata()) && owned(path.metadata())
}

/// Windows has no owner check as cheap as a uid comparison, so ownership does not gate this there.
#[cfg(not(unix))]
fn owned_by_user(_path: &Path) -> bool {
    true
}

/// The config roots this process trusted because the command was typed, resolved.
static TRUSTED_HERE: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Whether this process trusted `config_root` because the command was typed, rather than
/// finding trust that existed before it started. Nobody looked at the project before that
/// trust, so it does not vouch for where the project's links lead.
pub(super) fn trusted_by_this_command(config_root: &Path) -> bool {
    TRUSTED_HERE
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|roots| roots.contains(&resolved(config_root)))
}

/// Trusts `config_root` when the command consents for the user, and says so once per root.
/// `Ok(false)` leaves the decision to the prompt.
pub(super) fn trust_typed_command(path: &Path, config_root: &Path) -> eyre::Result<bool> {
    if !consents(read_signals(path, config_root)) {
        return Ok(false);
    }
    super::trust_and_reload_settings(config_root)?;
    if TRUSTED_HERE
        .lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(resolved(config_root))
    {
        info!(
            "trusted {} because you ran a mise command there. Undo with `mise untrust`.",
            style::epath(config_root)
        );
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONSENTING: Signals = Signals {
        stdin_tty: true,
        stdout_tty: true,
        stderr_tty: true,
        paranoid_config: false,
        shim: false,
        run_by_shell: false,
        owned_by_user: true,
    };

    #[test]
    fn a_command_typed_on_a_terminal_by_the_owner_consents() {
        assert!(consents(CONSENTING));
    }

    #[test]
    fn each_condition_alone_keeps_the_prompt() {
        let failing = [
            (
                "stdin piped",
                Signals {
                    stdin_tty: false,
                    ..CONSENTING
                },
            ),
            (
                "stdout captured",
                Signals {
                    stdout_tty: false,
                    ..CONSENTING
                },
            ),
            (
                "stderr piped",
                Signals {
                    stderr_tty: false,
                    ..CONSENTING
                },
            ),
            (
                "paranoid config",
                Signals {
                    paranoid_config: true,
                    ..CONSENTING
                },
            ),
            (
                "shim",
                Signals {
                    shim: true,
                    ..CONSENTING
                },
            ),
            (
                "completion or the command-not-found handler",
                Signals {
                    run_by_shell: true,
                    ..CONSENTING
                },
            ),
            (
                "another owner",
                Signals {
                    owned_by_user: false,
                    ..CONSENTING
                },
            ),
        ];
        for (name, signals) in failing {
            assert!(!consents(signals), "{name} must not consent");
        }
    }

    #[cfg(unix)]
    #[test]
    fn ownership_checks_the_link_and_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("mise.toml");
        std::fs::write(&own, "").unwrap();
        assert!(owned_by_user(&own));
        assert!(owned_by_user(dir.path()));
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&own, &link).unwrap();
        assert!(owned_by_user(&link));
        // A dangling link has no target the user owns.
        let dangling = dir.path().join("dangling.toml");
        std::os::unix::fs::symlink(dir.path().join("missing"), &dangling).unwrap();
        assert!(!owned_by_user(&dangling));
        assert!(!owned_by_user(&dir.path().join("missing")));
        // `/` belongs to root, so it is another account's for anyone else.
        if !nix::unistd::geteuid().is_root() {
            assert!(!owned_by_user(Path::new("/")));
        }
    }
}
