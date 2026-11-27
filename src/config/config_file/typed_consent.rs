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

/// Who ran the parsed command, as far as trust is concerned. The command line decides it once
/// global options are consumed, so `mise -q hook-not-found` and `mise -q hook-env` count as the
/// shell's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOrigin {
    /// A command the user may have typed.
    Typed,
    /// A command the shell runs for its own integration: completion and `usage`, which answer
    /// TAB completion or print the spec it reads, and `hook-not-found`, which the shell's
    /// command-not-found handler runs with the terminal on all three descriptors when the user
    /// mistypes any command. Neither TAB nor a typo is consent.
    ShellIntegration,
    /// `hook-env`, which the prompt hook runs before every prompt: it neither trusts nor asks.
    PromptHook,
}

/// `None` until the command line is parsed.
static COMMAND_ORIGIN: Mutex<Option<CommandOrigin>> = Mutex::new(None);

/// Records who ran the parsed command.
pub fn set_command_origin(origin: CommandOrigin) {
    *COMMAND_ORIGIN.lock().unwrap() = Some(origin);
}

/// Whether the parsed command is `hook-env`, whatever global options come before it.
pub(super) fn is_prompt_hook() -> bool {
    *COMMAND_ORIGIN.lock().unwrap() == Some(CommandOrigin::PromptHook)
}

/// Whether the shell, not the user, ran this mise. Before the command line is parsed nothing
/// shows the user typed it: a tool stub, for one, loads config first.
fn run_by_shell() -> bool {
    env::__USAGE.is_some() || *COMMAND_ORIGIN.lock().unwrap() != Some(CommandOrigin::Typed)
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
        owned_by_user: root_owned_by_user(path, config_root),
    }
}

/// Whether the user owns `path`, its trust root, and every other config file the record for that
/// root admits. The record names the root, so it admits all of those files, including ones
/// loaded after `path` and ones for a `MISE_ENV` this command did not select: a
/// `.config/mise.toml` or `mise.prod.toml` another account left beside the user's own
/// `mise.toml` must keep the prompt.
fn root_owned_by_user(path: &Path, config_root: &Path) -> bool {
    let mut covered =
        crate::config::load_config_paths(&crate::config::DEFAULT_CONFIG_FILENAMES, false);
    if config_root.is_dir() {
        covered.extend(crate::config::config_files_in_dir_for_any_env(config_root));
    }
    // A `conf.d` folder has a trust root of its own, so the record for this root does not admit
    // the fragments in it.
    covered.retain(|other| super::config_trust_root(other) == config_root);
    owned_by_user(config_root)
        && std::iter::once(path)
            .chain(covered.iter().map(PathBuf::as_path))
            .all(|file| owned_below(config_root, file))
}

/// Whether the user owns `file` and each directory between `root` and it. A `.config` that
/// another account made a link to the user's own directory would otherwise pass on the file
/// it leads to.
fn owned_below(root: &Path, file: &Path) -> bool {
    owned_by_user(file)
        && file
            .ancestors()
            .skip(1)
            .take_while(|dir| *dir != root && dir.starts_with(root))
            .all(owned_by_user)
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

/// Trusts `config_root` when the command consents for the user, and says so once per root.
/// `Ok(false)` leaves the decision to the prompt.
///
/// A project whose `[settings]` fail to load once trusted keeps no record: every later command,
/// `mise untrust` too, would fail on that setting at startup, so the user could not undo it.
pub(super) fn trust_typed_command(path: &Path, config_root: &Path) -> eyre::Result<bool> {
    if !consents(read_signals(path, config_root)) {
        return Ok(false);
    }
    static NOTIFIED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);
    super::trust_and_reload_settings_or_roll_back(config_root)?;
    if NOTIFIED
        .lock()
        .unwrap()
        .get_or_insert_with(HashSet::new)
        .insert(config_root.to_path_buf())
    {
        let undo = match untrust_argument(path) {
            Some(arg) => format!("Undo with `mise untrust {arg}`."),
            None => format!(
                "Undo with `mise untrust` and the path {}, quoted for your shell.",
                path.display()
            ),
        };
        info!(
            "trusted {} because you ran a mise command there. {undo}",
            style::epath(config_root),
        );
    }
    Ok(true)
}

/// `path` as the argument of the `mise untrust` that undoes this trust, quoted for a shell.
/// `mise untrust` with no argument acts on the nearest config only, and one command can trust a
/// parent project too, so the hint names the file whose root was trusted.
#[cfg(not(windows))]
fn untrust_argument(path: &Path) -> Option<String> {
    let shown = mise_util::file::display_path(path);
    Some(match shown.strip_prefix("~/") {
        // `~` expands only when it is not quoted.
        Some(rest) => format!("~/{}", shell_words::quote(rest)),
        None => shell_words::quote(&shown).into_owned(),
    })
}

/// [`untrust_argument`] on Windows, where `~` does not expand: the full path, quoted for the
/// shell when mise knows it, or `None` when no quoting is safe in every shell it may be.
#[cfg(windows)]
fn untrust_argument(path: &Path) -> Option<String> {
    quote_for_windows(&path.display().to_string(), *env::MISE_SHELL)
}

/// A path is bare when it holds nothing any shell reads. PowerShell, which `mise activate pwsh`
/// names in `MISE_SHELL`, gets its own single quotes, which keep everything literal but the
/// single quotes it doubles. Otherwise the shell may be `cmd.exe`, which takes no single quotes,
/// or a PowerShell that is not activated, and double quotes keep a path literal in both unless it
/// holds `$`, a backtick or a double quote (curly ones included), which PowerShell reads in them,
/// or `%` or `!`, which `cmd.exe` does. Such a path, or one under another known shell, is left for
/// the user to quote.
#[cfg(any(windows, test))]
fn quote_for_windows(shown: &str, shell: Option<crate::shell::ShellType>) -> Option<String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || r"\/:._-".contains(c);
    const READ_IN_DOUBLE_QUOTES: [char; 8] =
        ['$', '`', '%', '!', '"', '\u{201c}', '\u{201d}', '\u{201e}'];
    if !shown.is_empty() && shown.chars().all(plain) {
        Some(shown.to_string())
    } else if shell == Some(crate::shell::ShellType::Pwsh) {
        Some(format!("'{}'", crate::shell::pwsh_escape_sq(shown)))
    } else if shell.is_none() && !shown.contains(READ_IN_DOUBLE_QUOTES) {
        Some(format!("\"{shown}\""))
    } else {
        None
    }
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

    #[test]
    fn windows_hint_quotes_only_paths_that_need_it() {
        use crate::shell::ShellType;
        let quote = |shown: &str, shell| quote_for_windows(shown, shell);
        for shell in [None, Some(ShellType::Pwsh), Some(ShellType::Bash)] {
            assert_eq!(
                quote(r"C:\work\app\mise.toml", shell).as_deref(),
                Some(r"C:\work\app\mise.toml"),
                "{shell:?}"
            );
        }
        // An unknown shell may be `cmd.exe` or PowerShell: double quotes keep these literal in both.
        assert_eq!(
            quote(r"C:\Users\Jo Smith\app\mise.toml", None).as_deref(),
            Some(r#""C:\Users\Jo Smith\app\mise.toml""#)
        );
        assert_eq!(
            quote(r"C:\a&b\mise.toml", None).as_deref(),
            Some(r#""C:\a&b\mise.toml""#)
        );
        assert_eq!(
            quote(r"C:\Jo's\mise.toml", None).as_deref(),
            Some(r#""C:\Jo's\mise.toml""#)
        );
        // No quoting keeps these literal in both, so the path is left for the user to quote.
        for shown in [
            r"C:\work\$demo\mise.toml",
            r"C:\work\$(rm x)\mise.toml",
            r"C:\work\a`b\mise.toml",
            r"C:\100%\mise.toml",
            r"C:\wow!\mise.toml",
            "C:\\a\u{201c}b\\mise.toml",
        ] {
            assert_eq!(quote(shown, None), None, "{shown}");
        }
        // PowerShell's single quotes keep everything literal but the single quotes it doubles,
        // typographic ones included.
        assert_eq!(
            quote(r"C:\work\$(rm x)\mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some(r"'C:\work\$(rm x)\mise.toml'")
        );
        assert_eq!(
            quote(r"C:\Jo's $x\mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some(r"'C:\Jo''s $x\mise.toml'")
        );
        assert_eq!(
            quote("C:\\Jo\u{2019}s $x\\mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some("'C:\\Jo\u{2019}\u{2019}s $x\\mise.toml'")
        );
        // Another known shell is left for the user to quote.
        assert_eq!(
            quote(r"C:\Users\Jo Smith\app\mise.toml", Some(ShellType::Bash)),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn ownership_checks_each_directory_below_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".config")).unwrap();
        let own = dir.path().join(".config").join("mise.toml");
        std::fs::write(&own, "").unwrap();
        assert!(owned_below(dir.path(), &own));
        // Measured from `/`, the path passes through a directory such as `/tmp` or `/home` that
        // belongs to root, so it fails there although the user owns the file and its parent.
        if !nix::unistd::geteuid().is_root() {
            assert!(owned_by_user(&own) && owned_by_user(own.parent().unwrap()));
            assert!(!owned_below(Path::new("/"), &own));
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
