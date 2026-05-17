pub use std::path::*;

use crate::dirs;

pub trait PathExt {
    /// replaces $HOME with "~"
    fn display_user(&self) -> String;
    fn mount(&self, on: &Path) -> PathBuf;
    fn is_empty(&self) -> bool;
}

impl PathExt for Path {
    fn display_user(&self) -> String {
        let home = dirs::HOME.to_string_lossy();
        let home_str: &str = home.as_ref();
        match cfg!(unix) && self.starts_with(home_str) && home != "/" {
            true => self.to_string_lossy().replacen(home_str, "~", 1),
            false => self.to_string_lossy().to_string(),
        }
    }

    fn mount(&self, on: &Path) -> PathBuf {
        if PathExt::is_empty(self) {
            on.to_path_buf()
        } else {
            on.join(self)
        }
    }

    fn is_empty(&self) -> bool {
        self.as_os_str().is_empty()
    }
}

/// Convert a Windows-style path list (`;`-separated, drive-letter prefix, `\` or `/`
/// separator) into a Git Bash / MSYS Unix-style path list (`:`-separated, `/c/...`
/// prefix, `/` separator).
///
/// Pure Rust, no subprocess. Designed for the case where mise on Windows spawns a
/// POSIX shell (`bash -c`, `sh -c`, ...) for a task — that shell uses PATH itself to
/// resolve commands, and cannot read `C:\foo;D:\bar`.
///
/// Conversion rules per entry, applied independently:
///
/// - `<drive>:[\\/]...` (canonical Windows drive path) → `/<drive lowercase>/<rest with `/` separator>`
/// - already-Unix entries (start with `/`) → pass through unchanged
/// - empty entries (e.g. trailing `;`) → preserved as empty
/// - UNC (`\\?\...`, `\\server\share\...`) → pass through unchanged. bash will fail
///   to use them, which matches what would happen without conversion.
/// - other entries (relative paths, bare names, drive-relative `C:foo`, etc.) →
///   `\` is replaced with `/` so that bash can resolve entries like
///   `node_modules\.bin` or `.\bin` injected by tools that emit Windows separators.
///
/// Out of scope (kept narrow per maintainer guidance — see PR description / `_context/`):
///
/// - Cygwin's `/etc/fstab` mount table
/// - Cygwin's `/cygdrive/c/` prefix (Git Bash uses `/c/`, which is the dominant case)
/// - Git Bash's "magic" mount of `/usr` to its install dir — `/c/Program Files/Git/usr/bin`
///   is resolved by bash to the same executable as `/usr/bin`, so no remapping is needed
///   for PATH-resolution to succeed.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn windows_path_list_to_unix(path_list: &str) -> String {
    let mut out = String::with_capacity(path_list.len());
    let mut first = true;
    for entry in path_list.split(WINDOWS_PATH_SEP) {
        if !first {
            out.push(':');
        }
        append_single_windows_path_to_unix(&mut out, entry);
        first = false;
    }
    out
}

#[cfg_attr(not(windows), allow(dead_code))]
const WINDOWS_PATH_SEP: char = ';';

#[cfg_attr(not(windows), allow(dead_code))]
fn append_single_windows_path_to_unix(out: &mut String, entry: &str) {
    if entry.is_empty() {
        return;
    }
    // Already-Unix entries and UNC paths are passed through verbatim.
    if entry.starts_with('/') || entry.starts_with("\\\\") {
        out.push_str(entry);
        return;
    }

    let bytes = entry.as_bytes();
    let is_canonical_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');

    let rest = if is_canonical_drive {
        // C:\foo → /c/foo : emit `/<drive lowercase>` then the tail with `\` → `/`.
        out.push('/');
        out.push((bytes[0] as char).to_ascii_lowercase());
        &entry[2..]
    } else {
        // Other shapes (relative paths, bare names, `C:foo`) — keep as-is but
        // still translate `\` → `/` so bash can resolve them.
        entry
    };
    for c in rest.chars() {
        out.push(if c == '\\' { '/' } else { c });
    }
}

/// Returns the lowercase stem of `program`'s basename, with any final `.exe`
/// (case-insensitive) stripped. Splits on both `/` and `\` so the result is the
/// same regardless of host `Path` separator — important since this is
/// unit-tested on Linux/macOS too. Does not stat the file — input may be a bare
/// name like `"bash"` that resolves later via the launcher's PATH search.
///
/// Returns `None` only when `program` is not valid UTF-8.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn program_stem(program: &Path) -> Option<String> {
    let s = program.to_str()?;
    let basename = s.rsplit(['/', '\\']).next().unwrap_or(s);
    let stem = match basename.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
        _ => basename,
    };
    Some(stem.to_ascii_lowercase())
}

/// Returns true if `program` is the path or basename of a POSIX-style shell that
/// expects a Unix-style PATH. Used on Windows to decide whether to convert the
/// child's PATH before spawning.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn is_posix_shell_program(program: &Path) -> bool {
    const POSIX_SHELLS: &[&str] = &["bash", "sh", "zsh", "fish", "ksh", "dash"];
    let Some(stem) = program_stem(program) else {
        return false;
    };
    POSIX_SHELLS.iter().any(|name| *name == stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_path_list_to_unix_basic() {
        assert_eq!(windows_path_list_to_unix(r"C:\foo;D:\bar"), "/c/foo:/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_forward_slash() {
        assert_eq!(windows_path_list_to_unix("C:/foo;D:/bar"), "/c/foo:/d/bar");
    }

    #[test]
    fn test_windows_path_list_to_unix_mixed_separators() {
        assert_eq!(
            windows_path_list_to_unix(r"C:\foo\bar;D:/baz/qux"),
            "/c/foo/bar:/d/baz/qux"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_passthrough_unix_entries() {
        assert_eq!(
            windows_path_list_to_unix("/usr/bin;C:\\foo;/c/bar"),
            "/usr/bin:/c/foo:/c/bar"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_passthrough_unc() {
        // UNC entries are passed through verbatim (they contain `:` themselves,
        // so we cannot split the result on `:` to inspect entries — bash receives
        // the whole string and will fail to use the UNC entry, which matches what
        // would happen without conversion).
        assert_eq!(
            windows_path_list_to_unix(r"\\?\C:\foo;C:\bar"),
            r"\\?\C:\foo:/c/bar"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_empty_entries() {
        assert_eq!(windows_path_list_to_unix("C:\\foo;"), "/c/foo:");
        assert_eq!(windows_path_list_to_unix(";C:\\foo"), ":/c/foo");
        assert_eq!(windows_path_list_to_unix(""), "");
    }

    #[test]
    fn test_windows_path_list_to_unix_drive_letter_case() {
        assert_eq!(windows_path_list_to_unix(r"C:\foo"), "/c/foo");
        assert_eq!(windows_path_list_to_unix(r"c:\foo"), "/c/foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_program_files_with_spaces() {
        assert_eq!(
            windows_path_list_to_unix(r"C:\Program Files\Git\bin"),
            "/c/Program Files/Git/bin"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_bare_drive_letter_passthrough() {
        // Bare "C:" or "C:foo" (relative-to-drive) is unrecognized — pass through.
        assert_eq!(windows_path_list_to_unix("C:"), "C:");
        assert_eq!(windows_path_list_to_unix("C:foo"), "C:foo");
    }

    #[test]
    fn test_windows_path_list_to_unix_relative_paths_with_backslashes() {
        // mise can inject relative entries via `[env] _.path = ["./node_modules/.bin"]`,
        // and tools that emit Windows separators may produce backslash forms. bash
        // does not treat `\` as a separator, so we translate `\` → `/` for non-UNC,
        // non-canonical-drive entries too.
        assert_eq!(
            windows_path_list_to_unix(r"node_modules\.bin"),
            "node_modules/.bin"
        );
        assert_eq!(windows_path_list_to_unix(r".\bin"), "./bin");
        assert_eq!(
            windows_path_list_to_unix(r"node_modules\.bin;C:\tools\bin"),
            "node_modules/.bin:/c/tools/bin"
        );
    }

    #[test]
    fn test_windows_path_list_to_unix_single_entry() {
        assert_eq!(windows_path_list_to_unix(r"C:\foo"), "/c/foo");
    }

    #[test]
    fn test_is_posix_shell_program() {
        assert!(is_posix_shell_program(Path::new("bash")));
        assert!(is_posix_shell_program(Path::new("bash.exe")));
        assert!(is_posix_shell_program(Path::new("BASH.EXE")));
        assert!(is_posix_shell_program(Path::new(
            r"C:\Program Files\Git\bin\bash.exe"
        )));
        assert!(is_posix_shell_program(Path::new("/usr/bin/bash")));
        assert!(is_posix_shell_program(Path::new("sh")));
        assert!(is_posix_shell_program(Path::new("zsh")));
        assert!(is_posix_shell_program(Path::new("fish")));

        assert!(!is_posix_shell_program(Path::new("cmd")));
        assert!(!is_posix_shell_program(Path::new("cmd.exe")));
        assert!(!is_posix_shell_program(Path::new("powershell")));
        assert!(!is_posix_shell_program(Path::new("pwsh.exe")));
        assert!(!is_posix_shell_program(Path::new("rustc")));
        assert!(!is_posix_shell_program(Path::new("")));
    }
}
