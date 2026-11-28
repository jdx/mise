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
    /// `tool-stub`, which a stub's `#!/usr/bin/env -S mise tool-stub` line runs when the user runs
    /// the tool the stub names, from whatever directory they are in. As with a shim, the user typed
    /// the tool's name, not a mise command, so the prompt still asks.
    ToolStub,
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
///
/// Both paths are made absolute and their `..` resolved the way [`super::config_root`] does for
/// `root`: a relative `file`, such as the one `mise set --file .config/custom.toml` names, would
/// otherwise never start with `root`, and the directories between them would go unchecked.
///
/// The system resolves a `..` after a symlink from the link's target, not from the directory
/// that holds the link, so `link/../mise.toml` can open another file than the `mise.toml` its
/// lexical form names. A path whose two forms lead to different files keeps the prompt.
///
/// The directories the system passes through can differ from the written ones too, when a link
/// leads elsewhere: [`resolved_path_guarded`] checks those.
fn owned_below(root: &Path, file: &Path) -> bool {
    use path_absolutize::Absolutize;
    let (Ok(lexical_root), Ok(lexical_file)) = (root.absolutize(), file.absolutize()) else {
        return false;
    };
    let same_file = |raw: &Path, lexical: &Path| matches!((raw.canonicalize(), lexical.canonicalize()), (Ok(a), Ok(b)) if a == b);
    if !same_file(root, &lexical_root) || !same_file(file, &lexical_file) {
        return false;
    }
    let (root, file) = (lexical_root, lexical_file);
    owned_by_user(&file)
        && file
            .ancestors()
            .skip(1)
            .take_while(|dir| *dir != &*root && dir.starts_with(&root))
            .all(owned_by_user)
        && resolved_path_guarded(&root, &file)
}

/// Whether no other account can replace `file` through a directory on the path the system
/// resolves it to. A link checks itself and its target, but whoever may write a directory on the
/// way to the target can rename that file away and put their own in its place after the trust
/// is recorded. A linked `.config` leads into another directory the same way.
///
/// Inside the resolved root the directories are checked as the written ones are. A root reached
/// through a link, and a file that resolves outside its root, also pass through directories up to
/// `/`: each has to belong to the user or to root, and no other account may replace what is in
/// it. A sticky directory such as `/tmp` lets nobody but an entry's owner rename it.
fn resolved_path_guarded(root: &Path, file: &Path) -> bool {
    let (Ok(resolved_root), Ok(target)) = (root.canonicalize(), file.canonicalize()) else {
        return false;
    };
    if !links_guarded(&resolved_root, file) {
        return false;
    }
    if !target.starts_with(&resolved_root) {
        return target.ancestors().skip(1).all(entries_guarded);
    }
    target
        .ancestors()
        .skip(1)
        .take_while(|dir| *dir != resolved_root.as_path())
        .all(owned_by_user)
        && (resolved_root.as_path() == root || resolved_root.ancestors().all(entries_guarded))
}

/// Whether each link the system follows from `file` to the file it opens can be changed only by
/// the user or root. `canonicalize` goes straight to the end, so a link on the way, or a directory
/// that is a link, would go unchecked, and whoever may write the directory holding it, or owns it
/// in a sticky one, could point it at their own file after the trust is recorded.
///
/// The path is followed one component at a time, as the system does. Each link has to belong to
/// the user or root, and so does the directory that holds it, which no other account may write
/// unless it is sticky; so does each directory entered after that, except the ones above the
/// resolved root, which are not checked for a root reached without a link either.
#[cfg(unix)]
fn links_guarded(resolved_root: &Path, file: &Path) -> bool {
    use std::collections::VecDeque;
    use std::ffi::OsString;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    enum Step {
        Root,
        Up,
        Name(OsString),
    }
    let steps = |path: &Path| -> Vec<Step> {
        path.components()
            .filter_map(|c| match c {
                Component::RootDir | Component::Prefix(_) => Some(Step::Root),
                Component::ParentDir => Some(Step::Up),
                Component::CurDir => None,
                Component::Normal(name) => Some(Step::Name(name.to_os_string())),
            })
            .collect()
    };
    // The kernel gives up with `ELOOP` after this many links.
    const MAX_LINKS: usize = 40;
    let uid = nix::unistd::geteuid().as_raw();
    let mut pending: VecDeque<Step> = steps(file).into();
    let mut current = PathBuf::from("/");
    let mut links = 0;
    while let Some(step) = pending.pop_front() {
        let name = match step {
            Step::Root => {
                current = PathBuf::from("/");
                continue;
            }
            // `..` is an entry of `current` too, read like any other, so `current` is checked
            // the same way before it is left: every directory the lookup reads an entry from
            // has to be guarded, also one a link's target only passes through.
            Step::Up => {
                let above_root = current != resolved_root && resolved_root.starts_with(&current);
                if !above_root && !entries_guarded(&current) {
                    return false;
                }
                current.pop();
                continue;
            }
            Step::Name(name) => name,
        };
        let next = current.join(&name);
        let Ok(meta) = next.symlink_metadata() else {
            return false;
        };
        if meta.file_type().is_symlink() {
            links += 1;
            let Ok(target) = next.read_link() else {
                return false;
            };
            if links > MAX_LINKS
                || (meta.uid() != uid && meta.uid() != 0)
                || !entries_guarded(&current)
            {
                return false;
            }
            for step in steps(&target).into_iter().rev() {
                pending.push_front(step);
            }
        } else {
            let above_root = current != resolved_root && resolved_root.starts_with(&current);
            if !above_root && !entries_guarded(&current) {
                return false;
            }
            current = next;
        }
    }
    true
}

/// See the Unix [`owned_by_user`]: ownership does not gate this on Windows.
#[cfg(not(unix))]
fn links_guarded(_resolved_root: &Path, _file: &Path) -> bool {
    true
}

/// Whether only the user, or root, can replace an entry of directory `dir`: it belongs to one
/// of them, and is sticky or writable by nobody else.
#[cfg(unix)]
fn entries_guarded(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = nix::unistd::geteuid();
    let Ok(meta) = dir.metadata() else {
        return false;
    };
    (meta.uid() == uid.as_raw() || meta.uid() == 0)
        && (meta.mode() & 0o1000 != 0
            || !writable_by_others(meta.mode(), meta.gid(), private_group(uid)))
}

/// See the Unix [`owned_by_user`]: ownership does not gate this on Windows.
#[cfg(not(unix))]
fn entries_guarded(_dir: &Path) -> bool {
    true
}

/// Whether `path` and, when it is a symlink, what it points at both belong to the effective user,
/// and no other account can write to what it points at.
///
/// The link is checked so a link another account planted to the user's own file does not pass,
/// and the target so a link the user made to another account's file does not either. A file or
/// directory the user owns but that another account may write is shared: that account could
/// edit the config, or replace it through the directory, after the trust is recorded.
#[cfg(unix)]
fn owned_by_user(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let uid = nix::unistd::geteuid();
    let (Ok(link), Ok(target)) = (path.symlink_metadata(), path.metadata()) else {
        return false;
    };
    link.uid() == uid.as_raw()
        && target.uid() == uid.as_raw()
        && !writable_by_others(target.mode(), target.gid(), private_group(uid))
}

/// Whether a file or directory with permission bits `mode` and group `gid` may be written by an
/// account other than its owner. World-writable always may. Group-writable may too, unless the
/// group is `private_gid`, the user's own private group, which holds nobody else: with that
/// convention, a umask of `002` makes every file group-writable.
#[cfg(unix)]
fn writable_by_others(mode: u32, gid: u32, private_gid: Option<u32>) -> bool {
    mode & 0o002 != 0 || (mode & 0o020 != 0 && private_gid != Some(gid))
}

/// The user's private group: their primary group when it has the user's name and no other
/// member. `None` when there is no such group or it cannot be read, so a group-writable file
/// keeps the prompt.
#[cfg(unix)]
fn private_group(uid: nix::unistd::Uid) -> Option<u32> {
    use nix::unistd::{Group, User};
    static PRIVATE_GROUP: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *PRIVATE_GROUP.get_or_init(|| {
        let user = User::from_uid(uid).ok().flatten()?;
        let group = Group::from_gid(user.gid).ok().flatten()?;
        (group.name == user.name && group.mem.iter().all(|member| *member == user.name))
            .then_some(group.gid.as_raw())
    })
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
        // `mise untrust` reads a file it is given as `mise trust` does. With only paranoid
        // `env_files` on, a file that does not look like mise config, such as `.node-version`,
        // is then an env file with a record of its own, so the hint names the root instead.
        let named = if super::trust_root_for_cli(path) == config_root {
            path
        } else {
            config_root
        };
        let undo = match untrust_argument(named) {
            Some(arg) => format!("Undo with `mise untrust {arg}`."),
            None => format!(
                "Undo with `mise untrust` and the path {}, quoted for your shell.",
                named.display()
            ),
        };
        info!(
            "trusted {} because you ran a mise command there. {undo}",
            style::epath(config_root),
        );
    }
    Ok(true)
}

/// `path` as the argument of the `mise untrust` that undoes this trust, quoted for the shell
/// `MISE_SHELL` names, or `None` when no quoting is known to be safe there.
///
/// Only `MISE_SHELL`, which `mise activate` sets for the shell it runs in, names that shell.
/// `SHELL` is the login shell, and the user may have typed the command in another one, such as
/// PowerShell started from bash, which cannot read bash's quoting.
///
/// `mise untrust` with no argument acts on the nearest config only, and one command can trust a
/// parent project too, so the hint names the file whose root was trusted, or that root.
fn untrust_argument(path: &Path) -> Option<String> {
    let activated = env::var("MISE_SHELL").ok().and_then(|s| s.parse().ok());
    quote_untrust_path(path, activated, cfg!(windows))
}

/// [`untrust_argument`] for a given shell and OS. The shell decides, not the OS: PowerShell runs
/// on Unix too, and Git Bash on Windows.
///
/// - PowerShell gets its own single quotes, which keep everything literal but the single quotes
///   it doubles, typographic ones included.
/// - Bash and zsh, and fish for a path with no backslash (fish reads `\\` in single quotes), get
///   POSIX single quotes, which also keep a Windows path's backslashes. On Unix a path in the home
///   directory starts with an unquoted `~`, which expands only when it is not quoted.
/// - Any other shell, or none named, gets only a full path with nothing any shell reads in it.
fn quote_untrust_path(
    path: &Path,
    shell: Option<crate::shell::ShellType>,
    windows: bool,
) -> Option<String> {
    use crate::shell::ShellType;
    let full = path.display().to_string();
    let bare = |backslash: bool| {
        let plain =
            |c: char| c.is_ascii_alphanumeric() || "/:._-".contains(c) || (backslash && c == '\\');
        (!full.is_empty() && full.chars().all(plain)).then(|| full.clone())
    };
    let posix = || {
        if windows {
            return shell_words::quote(&full).into_owned();
        }
        let shown = mise_util::file::display_path(path);
        // The home directory itself: `'~'` would name a directory called `~`.
        if shown == "~" {
            return shown;
        }
        match shown.strip_prefix("~/") {
            Some(rest) => format!("~/{}", shell_words::quote(rest)),
            None => shell_words::quote(&shown).into_owned(),
        }
    };
    match shell {
        Some(ShellType::Pwsh) => {
            bare(true).or_else(|| Some(format!("'{}'", crate::shell::pwsh_escape_sq(&full))))
        }
        Some(ShellType::Bash | ShellType::Zsh) => Some(posix()),
        Some(ShellType::Fish) if !full.contains('\\') => Some(posix()),
        _ => bare(false),
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

    // Unix only: there the `~` form comes from `display_path`, which spells separators as the
    // host does.
    #[cfg(unix)]
    #[test]
    fn undo_hint_quotes_for_the_shell_on_unix() {
        use crate::shell::ShellType;
        let quote = |shown: &str, shell| quote_untrust_path(Path::new(shown), shell, false);
        // Bash, zsh and fish get POSIX single quotes.
        for shell in [
            Some(ShellType::Bash),
            Some(ShellType::Zsh),
            Some(ShellType::Fish),
        ] {
            assert_eq!(
                quote("/tmp/Jo's app/mise.toml", shell).as_deref(),
                Some(r"'/tmp/Jo'\''s app/mise.toml'"),
                "{shell:?}"
            );
        }
        // PowerShell on Unix gets its own quotes, typographic apostrophes included.
        assert_eq!(
            quote("/tmp/Jo's/app/mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some("'/tmp/Jo''s/app/mise.toml'")
        );
        assert_eq!(
            quote("/tmp/Jo\u{2019}s $x/mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some("'/tmp/Jo\u{2019}\u{2019}s $x/mise.toml'")
        );
        assert_eq!(
            quote("/tmp/app/mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some("/tmp/app/mise.toml")
        );
        // Fish reads `\\` in single quotes, and other shells get only a path with nothing to quote.
        assert_eq!(quote(r"/tmp/a\b/mise.toml", Some(ShellType::Fish)), None);
        assert_eq!(quote("/tmp/a b/mise.toml", Some(ShellType::Nu)), None);
        assert_eq!(
            quote("/tmp/app/mise.toml", Some(ShellType::Nu)).as_deref(),
            Some("/tmp/app/mise.toml")
        );
        // With no shell named the user may be in any of them, PowerShell started from bash too,
        // so only a path with nothing to quote is given.
        assert_eq!(quote("/tmp/Jo's app/mise.toml", None), None);
        assert_eq!(quote("/tmp/a$b/mise.toml", None), None);
        assert_eq!(
            quote("/tmp/app/mise.toml", None).as_deref(),
            Some("/tmp/app/mise.toml")
        );
    }

    // A trusted root in the home directory, such as the one a `.node-version` there names, is
    // shown as `~`, which the shell expands only when it is not quoted.
    #[cfg(unix)]
    #[test]
    fn undo_hint_leaves_the_home_directory_unquoted() {
        use crate::shell::ShellType;
        let home = &*crate::dirs::HOME;
        for shell in [ShellType::Bash, ShellType::Zsh, ShellType::Fish] {
            assert_eq!(
                quote_untrust_path(home, Some(shell), false).as_deref(),
                Some("~"),
                "{shell:?}"
            );
            assert_eq!(
                quote_untrust_path(&home.join("Jo's app"), Some(shell), false).as_deref(),
                Some(r"~/'Jo'\''s app'"),
                "{shell:?}"
            );
        }
    }

    #[test]
    fn undo_hint_quotes_for_the_shell_on_windows() {
        use crate::shell::ShellType;
        let quote = |shown: &str, shell| quote_untrust_path(Path::new(shown), shell, true);
        assert_eq!(
            quote(r"C:\work\app\mise.toml", Some(ShellType::Pwsh)).as_deref(),
            Some(r"C:\work\app\mise.toml")
        );
        // Git Bash and MSYS2 read a bare backslash as an escape, so they get POSIX single quotes.
        for shell in [Some(ShellType::Bash), Some(ShellType::Zsh)] {
            assert_eq!(
                quote(r"C:\work\app\mise.toml", shell).as_deref(),
                Some(r"'C:\work\app\mise.toml'"),
                "{shell:?}"
            );
            assert_eq!(
                quote(r"C:\Users\Jo's $x\mise.toml", shell).as_deref(),
                Some(r"'C:\Users\Jo'\''s $x\mise.toml'"),
                "{shell:?}"
            );
        }
        assert_eq!(quote(r"C:\work\mise.toml", Some(ShellType::Fish)), None);
        // With no shell named the user may be in `cmd.exe`, PowerShell or a Git Bash that is not
        // activated, which reads a bare backslash as an escape, so the path is left for the user
        // to quote.
        for shown in [
            r"C:\work\app\mise.toml",
            r"C:\Users\Jo Smith\app\mise.toml",
            r"C:\Jo's\mise.toml",
            r"C:\work\$(rm x)\mise.toml",
        ] {
            assert_eq!(quote(shown, None), None, "{shown}");
        }
        assert_eq!(
            quote("C:/work/app/mise.toml", None).as_deref(),
            Some("C:/work/app/mise.toml")
        );
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
            quote(r"C:\Users\Jo Smith\app\mise.toml", Some(ShellType::Nu)),
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
    fn ownership_checks_the_directories_below_a_relative_or_dotted_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let config = root.join(".config");
        std::fs::create_dir(&config).unwrap();
        let own = config.join("custom.toml");
        std::fs::write(&own, "").unwrap();
        // The system resolves `x/..` only when `x` exists.
        std::fs::create_dir(root.join("x")).unwrap();
        // `.config` is shared, so a path through it must fail however it is written.
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o777)).unwrap();
        let dotted = root
            .join("x")
            .join("..")
            .join(".config")
            .join("custom.toml");
        // Up from the working directory to `/`, then down to the file.
        let cwd = std::env::current_dir().unwrap();
        let relative = (1..cwd.components().count())
            .map(|_| Path::new(".."))
            .collect::<PathBuf>()
            .join(own.strip_prefix("/").unwrap());
        assert!(relative.is_relative());
        for file in [&own, &dotted, &relative] {
            assert!(!owned_below(&root, file), "{}", file.display());
        }
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o755)).unwrap();
        for file in [&own, &dotted, &relative] {
            assert!(owned_below(&root, file), "{}", file.display());
        }
    }

    #[cfg(unix)]
    #[test]
    fn ownership_checks_the_file_a_dotdot_after_a_symlink_opens() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("elsewhere").join("sub")).unwrap();
        std::fs::write(root.join("mise.toml"), "").unwrap();
        std::fs::write(root.join("elsewhere").join("mise.toml"), "").unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere").join("sub"), root.join("link")).unwrap();
        // Read lexically this is `root/mise.toml`; the system opens `elsewhere/mise.toml`.
        let through_link = root.join("link").join("..").join("mise.toml");
        assert_eq!(
            std::fs::canonicalize(&through_link).unwrap(),
            root.join("elsewhere").join("mise.toml")
        );
        assert!(owned_below(&root, &root.join("mise.toml")));
        assert!(!owned_below(&root, &through_link));
        // A root written through the link is refused the same way.
        assert!(!owned_below(
            &root.join("link").join(".."),
            &root.join("mise.toml")
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_file_another_account_can_write_is_not_the_users() {
        // World-writable always is shared.
        assert!(writable_by_others(0o100666, 1000, Some(1000)));
        assert!(writable_by_others(0o040777, 1000, None));
        // Group-writable is shared unless the group is the user's private group.
        assert!(writable_by_others(0o100664, 100, Some(1000)));
        assert!(writable_by_others(0o100664, 1000, None));
        assert!(!writable_by_others(0o100664, 1000, Some(1000)));
        assert!(!writable_by_others(0o100644, 100, None));

        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("mise.toml");
        std::fs::write(&own, "").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(owned_by_user(&own));
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o646)).unwrap();
        assert!(!owned_by_user(&own));
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

    #[cfg(unix)]
    #[test]
    fn ownership_checks_the_directories_a_link_leads_through() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (root, shared) = (base.join("proj"), base.join("shared"));
        std::fs::create_dir_all(root.join("inside").join("sub")).unwrap();
        std::fs::create_dir_all(shared.join("sub")).unwrap();
        let (own, nested) = (
            shared.join("mise.toml"),
            shared.join("sub").join("mise.toml"),
        );
        std::fs::write(&own, "").unwrap();
        std::fs::write(&nested, "").unwrap();
        mode(&own, 0o644);
        let link = root.join("mise.toml");
        std::os::unix::fs::symlink(&own, &link).unwrap();
        // A linked `.config` leads through `shared` too.
        let linked_dir = root.join(".config");
        std::os::unix::fs::symlink(shared.join("sub"), &linked_dir).unwrap();
        let through_dir = linked_dir.join("mise.toml");

        mode(&shared, 0o755);
        assert!(owned_below(&root, &link));
        assert!(owned_below(&root, &through_dir));
        // The user owns the link and the file, but anyone may replace the file in `shared`.
        mode(&shared, 0o777);
        assert!(owned_by_user(&link) && owned_by_user(&own));
        assert!(!owned_below(&root, &link));
        assert!(!owned_below(&root, &through_dir));
        // Sticky, only the file's owner may rename or remove it.
        mode(&shared, 0o1777);
        assert!(owned_below(&root, &link));
        mode(&shared, 0o755);

        // A link to a file elsewhere in the root is checked from the root down, as written.
        let inner = root.join("inside").join("sub").join("mise.toml");
        std::fs::write(&inner, "").unwrap();
        let inner_link = root.join("config.toml");
        std::os::unix::fs::symlink(&inner, &inner_link).unwrap();
        assert!(owned_below(&root, &inner_link));
        mode(&root.join("inside"), 0o777);
        assert!(!owned_below(&root, &inner_link));
        mode(&root.join("inside"), 0o755);

        // A root reached through a link passes through the directories its target is in.
        let linked_root = base.join("linked");
        std::os::unix::fs::symlink(&root, &linked_root).unwrap();
        let plain = root.join("inside").join("mise.toml");
        std::fs::write(&plain, "").unwrap();
        assert!(owned_below(
            &linked_root,
            &linked_root.join("inside").join("mise.toml")
        ));
        mode(&base, 0o777);
        assert!(!owned_below(
            &linked_root,
            &linked_root.join("inside").join("mise.toml")
        ));
        mode(&base, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn ownership_checks_each_link_on_the_way() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let mode = |path: &Path, mode| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
        };
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let (root, shared, hop) = (base.join("proj"), base.join("shared"), base.join("hop"));
        for dir in [&root, &shared, &hop] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let own = shared.join("mise.toml");
        std::fs::write(&own, "").unwrap();
        // `proj/mise.toml` -> `hop/link` -> `shared/mise.toml`: `canonicalize` skips `hop`.
        symlink(&own, hop.join("link")).unwrap();
        let via_link = root.join("mise.toml");
        symlink(hop.join("link"), &via_link).unwrap();
        // `proj/.config` -> `hop/dir/sub`, where `hop/dir` is a link to `shared`'s parent.
        std::fs::create_dir_all(shared.join("sub")).unwrap();
        std::fs::write(shared.join("sub").join("mise.toml"), "").unwrap();
        symlink(&shared, hop.join("dir")).unwrap();
        symlink(hop.join("dir").join("sub"), root.join(".config")).unwrap();
        let via_dir = root.join(".config").join("mise.toml");
        // A relative link whose `..` is resolved from where it points.
        symlink("../hop/link", root.join("config.toml")).unwrap();
        let relative = root.join("config.toml");
        // A link whose target enters `hop` and leaves it again with `..`.
        symlink("../hop/../shared/mise.toml", root.join("mise.local.toml")).unwrap();
        let through = root.join("mise.local.toml");
        let files = [&via_link, &via_dir, &relative, &through];

        mode(&hop, 0o755);
        for file in files {
            assert!(owned_below(&root, file), "{}", file.display());
        }
        // Anyone may replace the links in `hop`, though none is on the final path.
        mode(&hop, 0o777);
        for file in files {
            assert!(!owned_below(&root, file), "{}", file.display());
        }
        // Sticky: only the user, who owns the links, may replace them.
        mode(&hop, 0o1777);
        for file in files {
            assert!(owned_below(&root, file), "{}", file.display());
        }
        mode(&hop, 0o755);

        // A loop is refused, not followed forever.
        symlink(base.join("loop"), base.join("loop")).unwrap();
        assert!(!links_guarded(&root, &base.join("loop")));
    }
}
