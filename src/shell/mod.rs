use crate::env;
use crate::hook_env;
use clap::ValueEnum;
use indoc::formatdoc;
use itertools::Itertools;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

mod bash;
mod elvish;
mod fish;
mod nushell;
mod pwsh;
mod xonsh;
mod zsh;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ShellType {
    Bash,
    Elvish,
    Fish,
    Nu,
    Xonsh,
    Zsh,
    // `powershell` is an alias, not a second shell. mise picks PowerShell 7 features at *runtime*
    // -- the emitted script checks `$PSVersionTable.PSVersion` and drops the chpwd hook below 7 --
    // so the name never carried the 5.1-vs-7 distinction, and `mise completion` already spells the
    // same shell `powershell`.
    //
    // It is not hidden, whatever `alias` suggests: `mise usage` renders aliases into
    // `mise.usage.kdl` and from there into the CLI docs, so both names are listed there.
    //
    // Deliberately not a doc comment: the derive turns those into the value's help text.
    #[value(alias = "powershell")]
    Pwsh,
}

impl ShellType {
    pub fn as_shell(&self) -> Box<dyn Shell> {
        match self {
            Self::Bash => Box::<bash::Bash>::default(),
            Self::Elvish => Box::<elvish::Elvish>::default(),
            Self::Fish => Box::<fish::Fish>::default(),
            Self::Nu => Box::<nushell::Nushell>::default(),
            Self::Xonsh => Box::<xonsh::Xonsh>::default(),
            Self::Zsh => Box::<zsh::Zsh>::default(),
            Self::Pwsh => Box::<pwsh::Pwsh>::default(),
        }
    }
}

impl Display for ShellType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bash => write!(f, "bash"),
            Self::Elvish => write!(f, "elvish"),
            Self::Fish => write!(f, "fish"),
            Self::Nu => write!(f, "nu"),
            Self::Xonsh => write!(f, "xonsh"),
            Self::Zsh => write!(f, "zsh"),
            Self::Pwsh => write!(f, "pwsh"),
        }
    }
}

impl FromStr for ShellType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.to_lowercase();
        // Take the last path component. Splitting on `/` alone only ever worked on unix: Windows
        // spells the separator `\`, and `MISE_SHELL` falls back to `SHELL`, which is `COMSPEC`
        // there -- always a full path. So every native path failed to resolve.
        let s = s.rsplit_once(['/', '\\']).map(|(_, s)| s).unwrap_or(&s);
        // `pwsh.exe` / `powershell.exe` is how Windows names them, and nothing between here and
        // `MISE_SHELL` strips that. Applied to every shell: `bash.exe` has the same problem.
        let s = s.strip_suffix(".exe").unwrap_or(s);
        match s {
            "bash" | "sh" => Ok(Self::Bash),
            "elvish" => Ok(Self::Elvish),
            "fish" => Ok(Self::Fish),
            "nu" => Ok(Self::Nu),
            "xonsh" => Ok(Self::Xonsh),
            "zsh" => Ok(Self::Zsh),
            // Both names, for the same reason `sh` maps to bash above: this parses a shell the
            // user is already in, and `powershell.exe` is what Windows PowerShell reports.
            "pwsh" | "powershell" => Ok(Self::Pwsh),
            _ => Err(format!("unsupported shell type: {s}")),
        }
    }
}

pub trait Shell: Display {
    fn activate(&self, opts: ActivateOptions) -> String;
    fn deactivate(&self) -> String;
    fn set_env(&self, k: &str, v: &str) -> String;
    fn prepend_env(&self, k: &str, v: &str) -> String;
    /// Prepend env, moving existing entries to the front if already present.
    /// Default falls back to prepend_env. Fish overrides with --move flag.
    fn move_prepend_env(&self, k: &str, v: &str) -> String {
        self.prepend_env(k, v)
    }
    /// Whether this shell natively deduplicates/reorders PATH entries.
    /// When true, activate_shims skips the is_dir_in_path guard and uses
    /// MovePrepend to ensure correct ordering on re-source.
    fn supports_move_path(&self) -> bool {
        false
    }
    fn unset_env(&self, k: &str) -> String;

    /// Set a shell alias. Returns empty string if not supported by this shell.
    fn set_alias(&self, name: &str, cmd: &str) -> String {
        // Default implementation returns empty string (unsupported)
        let _ = (name, cmd);
        String::new()
    }

    /// Unset a shell alias. Returns empty string if not supported by this shell.
    fn unset_alias(&self, name: &str) -> String {
        // Default implementation returns empty string (unsupported)
        let _ = name;
        String::new()
    }

    fn format_activate_prelude(&self, prelude: &[ActivatePrelude]) -> String {
        prelude
            .iter()
            .map(|p| match p {
                ActivatePrelude::Set(k, v) => self.set_env(k, v),
                ActivatePrelude::Prepend(k, v) => self.prepend_env(k, v),
                ActivatePrelude::MovePrepend(k, v) => self.move_prepend_env(k, v),
            })
            .join("")
    }
}

pub enum ActivatePrelude {
    Set(String, String),
    Prepend(String, String),
    /// Like Prepend but moves existing entries to the front (for fish --move).
    /// Used only by activate_shims to reorder paths on re-source.
    MovePrepend(String, String),
}

pub struct ActivateOptions {
    pub exe: PathBuf,
    pub flags: String,
    pub no_hook_env: bool,
    pub prelude: Vec<ActivatePrelude>,
}

pub fn build_deactivation_script(shell: &dyn Shell) -> String {
    if !env::is_activated() {
        return String::new();
    }

    let mut out = hook_env::clear_old_env(shell);
    out.push_str(&hook_env::clear_aliases(shell));
    out.push_str(&shell.deactivate());
    out
}

pub fn get_shell(shell: Option<ShellType>) -> Option<Box<dyn Shell>> {
    shell.or(*env::MISE_SHELL).map(|st| st.as_shell())
}

/// A shell to name in an example, chosen so the suggestion is one the reader could actually run.
///
/// The old messages said `zsh` on every platform, which on Windows points at something the reader
/// almost certainly does not have.
#[cfg(windows)]
pub const EXAMPLE_SHELL: &str = "pwsh";
#[cfg(not(windows))]
pub const EXAMPLE_SHELL: &str = "zsh";

/// The shell mise should generate for, or an error saying how to name one.
///
/// Detection is `MISE_SHELL`, then `SHELL`. PowerShell and cmd set neither, so on Windows this
/// resolves only when the session is already activated, the caller names the shell, or the user is
/// in a shell that sets `SHELL` (Git Bash, MSYS2, Cygwin) — which makes the error an ordinary
/// answer there rather than an edge case. It used to be `.expect()`, so that answer was a panic.
///
/// `how` is the caller's own instruction, because the commands differ: `mise activate` takes the
/// shell as an argument, `mise hook-env` takes `--shell`, and `mise shell` takes neither.
pub fn require_shell(shell: Option<ShellType>, how: &str) -> eyre::Result<Box<dyn Shell>> {
    get_shell(shell).ok_or_else(|| eyre::eyre!(no_shell_error(how)))
}

/// Split out from [`require_shell`] so it can be tested: the detection it fails on reads
/// `MISE_SHELL`, which is a process-wide `Lazy`, so the failing path itself cannot be constructed
/// from a test.
fn no_shell_error(how: &str) -> String {
    // Windows is the only platform where this is reachable without the user having done something
    // unusual, so it is the only one that needs to explain itself. Naming the two variables is the
    // useful part: a Git Bash user can see that mise would have taken `SHELL` and go look at why
    // theirs is not set, rather than reading this as "Windows is unsupported".
    let why = match cfg!(windows) {
        true => {
            "mise reads MISE_SHELL, then SHELL; neither names a shell it supports. PowerShell and cmd set neither.\n"
        }
        false => "",
    };
    let supported = ShellType::value_variants().iter().join(", ");
    formatdoc! {r#"
        mise could not tell which shell to generate for.
        {why}{how}
        Supported shells: {supported}"#
    }
}

/// Deliberately not `#[cfg(unix)]`: this is string handling, and Windows is the platform whose
/// paths were not being parsed.
#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;
    use std::str::FromStr;

    #[test]
    fn a_windows_path_resolves_to_its_shell() {
        // `MISE_SHELL`/`SHELL` hold a full path on Windows, and `\` is the separator there.
        for (input, expected) in [
            (r"C:\Program Files\PowerShell\7\pwsh.exe", ShellType::Pwsh),
            (r"C:\msys64\usr\bin\bash.exe", ShellType::Bash),
            // The value is lowercased before the split, so casing does not matter.
            (r"C:\Program Files\Git\bin\PWSH.EXE", ShellType::Pwsh),
            ("pwsh.exe", ShellType::Pwsh),
        ] {
            // Qualified because `ValueEnum` is in scope here and also defines `from_str`.
            assert_eq!(
                <ShellType as FromStr>::from_str(input),
                Ok(expected),
                "{input:?}"
            );
        }
    }

    #[test]
    fn unix_paths_and_bare_names_are_unchanged() {
        // The regression guard: `/` splitting already worked, and nothing here may change.
        for (input, expected) in [
            ("/usr/bin/bash", ShellType::Bash),
            ("/bin/zsh", ShellType::Zsh),
            ("bash", ShellType::Bash),
            ("sh", ShellType::Bash),
            ("pwsh", ShellType::Pwsh),
            ("fish", ShellType::Fish),
        ] {
            assert_eq!(
                <ShellType as FromStr>::from_str(input),
                Ok(expected),
                "{input:?}"
            );
        }
    }

    #[test]
    fn an_unsupported_shell_still_fails_but_names_itself() {
        // mise has no `cmd` activate script, so this stays an error -- the control for reading
        // the change as "more shells are supported" rather than "paths now resolve". What is new
        // is that the message names `cmd` instead of repeating the whole path back.
        for input in [r"C:\WINDOWS\system32\cmd.exe", "cmd.exe"] {
            assert_eq!(
                <ShellType as FromStr>::from_str(input),
                Err("unsupported shell type: cmd".to_string()),
                "{input:?}"
            );
        }
    }

    #[test]
    fn powershell_is_accepted_as_pwsh() {
        // Two paths reach this enum: clap for `mise activate <SHELL>`, and FromStr for detecting
        // the shell the user is already in. Both have to take the alias or the fix is half done.
        // Qualified because both traits in scope define `from_str`.
        assert_eq!(
            <ShellType as FromStr>::from_str("powershell"),
            Ok(ShellType::Pwsh),
            "FromStr"
        );
        assert_eq!(
            <ShellType as ValueEnum>::from_str("powershell", true),
            Ok(ShellType::Pwsh),
            "clap"
        );
        assert_eq!(
            <ShellType as ValueEnum>::from_str("pwsh", true),
            Ok(ShellType::Pwsh)
        );
    }

    #[test]
    fn the_exe_suffix_is_stripped() {
        // `MISE_SHELL` is parsed straight through `FromStr` with nothing removing the suffix
        // first, so a Windows shell name has to be matched with it.
        for name in ["powershell.exe", "pwsh.exe", "PowerShell.exe"] {
            assert_eq!(
                <ShellType as FromStr>::from_str(name),
                Ok(ShellType::Pwsh),
                "{name}"
            );
        }
        assert_eq!(
            <ShellType as FromStr>::from_str("bash.exe"),
            Ok(ShellType::Bash)
        );
    }

    #[test]
    fn the_primary_names_are_unchanged() {
        // Only the *names*. The alias is deliberately not asserted absent here: `mise usage`
        // renders aliases into `mise.usage.kdl` and the CLI docs, so "hidden" would be false.
        // What this pins is that adding an alias did not rename or reorder an existing value.
        let listed: Vec<String> = ShellType::value_variants()
            .iter()
            .filter_map(|v| v.to_possible_value())
            .map(|pv| pv.get_name().to_string())
            .collect();
        assert_eq!(
            listed,
            ["bash", "elvish", "fish", "nu", "xonsh", "zsh", "pwsh"]
        );
    }

    /// The message replaced a `.expect()`, so it is the only thing the user gets. It has to carry
    /// the caller's instruction and every shell that would have been accepted — the list is built
    /// from `value_variants()` so that adding a shell cannot leave it stale.
    #[test]
    fn the_no_shell_message_says_what_to_do_and_what_is_accepted() {
        let msg = no_shell_error("Name the shell: `mise activate zsh`.");

        assert!(
            msg.contains("Name the shell: `mise activate zsh`."),
            "{msg}"
        );
        for shell in ShellType::value_variants() {
            assert!(msg.contains(&shell.to_string()), "{shell} missing:\n{msg}");
        }
    }

    /// On Windows the message is the ordinary answer rather than an edge case, so it also has to
    /// say which variables were consulted. Elsewhere that line would be noise.
    #[test]
    fn only_windows_explains_why_detection_failed() {
        let msg = no_shell_error("Name the shell.");
        // Both names, because naming only `MISE_SHELL` would read as "activate first" to someone
        // in Git Bash, where setting `SHELL` is the actual answer.
        assert_eq!(msg.contains("MISE_SHELL"), cfg!(windows), "{msg}");
        assert_eq!(msg.contains("SHELL;"), cfg!(windows), "{msg}");
    }
}
