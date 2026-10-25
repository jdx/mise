//! What to tell a user whose mise is out of date: run `mise self-update`, or
//! follow the instructions the packager shipped.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::env;

/// Whether `mise self-update` can update this install. Packagers turn it off
/// with a marker file, an instructions file or `MISE_SELF_UPDATE_AVAILABLE`,
/// and a build without the `self_update` feature never has it.
#[cfg(feature = "self_update")]
pub fn self_update_available() -> bool {
    if let Some(b) = *env::MISE_SELF_UPDATE_AVAILABLE {
        return b;
    }
    let has_disable = env::MISE_SELF_UPDATE_DISABLED_PATH.is_some();
    let has_instructions = env::MISE_SELF_UPDATE_INSTRUCTIONS.is_some();
    !(has_disable || has_instructions || installed_by_winget())
}

#[cfg(not(feature = "self_update"))]
pub fn self_update_available() -> bool {
    false
}

/// Shown for a WinGet install, which has no instructions file to read.
#[cfg(feature = "self_update")]
const WINGET_UPGRADE_HINT: &str =
    "mise was installed by WinGet, update it with:\n\n  winget upgrade jdx.mise\n";

/// Whether `exe` lives in WinGet's package or link directories. Updating such a
/// binary in place makes WinGet refuse to manage the package afterwards
/// ("Unable to remove Portable package as it has been modified").
#[cfg(feature = "self_update")]
fn is_winget_path(exe: &std::path::Path) -> bool {
    let path = exe.to_string_lossy().replace('\\', "/").to_lowercase();
    path.contains("/microsoft/winget/packages/") || path.contains("/microsoft/winget/links/")
}

#[cfg(feature = "self_update")]
fn installed_by_winget() -> bool {
    is_winget_path(&env::MISE_BIN)
        || std::fs::canonicalize(&*env::MISE_BIN).is_ok_and(|p| is_winget_path(&p))
}

#[derive(Debug, Default, serde::Deserialize)]
struct InstructionsToml {
    message: Option<String>,
    #[serde(flatten)]
    commands: BTreeMap<String, String>,
}

fn read_instructions_file(path: &PathBuf) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let parsed: InstructionsToml = toml::from_str(&body).ok()?;
    if let Some(msg) = parsed.message {
        return Some(msg);
    }
    if let Some((_k, v)) = parsed.commands.into_iter().next() {
        return Some(v);
    }
    None
}

pub fn upgrade_instructions_text() -> Option<String> {
    if let Some(path) = &*env::MISE_SELF_UPDATE_INSTRUCTIONS
        && let Some(msg) = read_instructions_file(path)
    {
        return Some(msg);
    }
    #[cfg(feature = "self_update")]
    if installed_by_winget() && *env::MISE_SELF_UPDATE_AVAILABLE != Some(true) {
        return Some(WINGET_UPGRADE_HINT.to_string());
    }
    None
}

/// Shown when mise cannot update itself and the packager shipped no instructions
/// file. Without it, telling the user their mise is out of date is a dead end on
/// every install that disables self-update: a marker file (Homebrew, the AUR
/// `mise-bin` package), a build without the `self_update` feature (Arch, or a
/// local `--no-default-features` build), or `MISE_SELF_UPDATE_AVAILABLE=false`.
/// The wording stays neutral about which of those applies — being unable to
/// self-update is not by itself proof that a package manager owns the install.
pub(crate) const SELF_UPDATE_DISABLED_HINT: &str =
    "self-update is disabled for this install, update mise the same way you installed it";

/// How to update mise when `mise self-update` is not available: the packager's
/// instructions when they shipped some, otherwise the generic hint.
pub fn upgrade_instructions_or_hint() -> String {
    upgrade_instructions_text().unwrap_or_else(|| SELF_UPDATE_DISABLED_HINT.to_string())
}

/// Appends self-update guidance and packaging instructions (if any) to a message.
pub(crate) fn append_self_update_instructions(mut message: String) -> String {
    if self_update_available() {
        message.push_str("\nRun `mise self-update` to update mise");
    }
    if let Some(instructions) = upgrade_instructions_text() {
        message.push('\n');
        message.push_str(&instructions);
    } else if !self_update_available() {
        message.push('\n');
        message.push_str(SELF_UPDATE_DISABLED_HINT);
    }
    message
}

#[cfg(all(test, feature = "self_update"))]
mod tests {
    use super::is_winget_path;
    use std::path::Path;

    #[test]
    fn detects_winget_package_and_link_paths() {
        for p in [
            r"C:\Users\me\AppData\Local\Microsoft\WinGet\Packages\jdx.mise_Microsoft.Winget.Source_8wekyb3d8bbwe\mise\bin\mise.exe",
            r"C:\Users\me\AppData\Local\Microsoft\WinGet\Links\mise.exe",
            "c:/users/me/appdata/local/microsoft/winget/packages/jdx.mise/mise.exe",
        ] {
            assert!(is_winget_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn ignores_other_paths() {
        for p in [
            r"C:\Users\me\.local\bin\mise.exe",
            "/usr/local/bin/mise",
            r"C:\Users\me\AppData\Local\mise\bin\mise.exe",
        ] {
            assert!(!is_winget_path(Path::new(p)), "{p}");
        }
    }
}
