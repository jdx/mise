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
        match cfg!(unix) && self.starts_with(home.as_ref()) && home != "/" {
            true => self.to_string_lossy().replacen(home.as_ref(), "~", 1),
            false => self.to_string_lossy().to_string(),
        }
    }

    fn mount(&self, on: &Path) -> PathBuf {
        if self.is_empty() {
            on.to_path_buf()
        } else {
            on.join(self)
        }
    }

    fn is_empty(&self) -> bool {
        self.as_os_str().is_empty()
    }
}

pub(crate) enum PathEscape {
    Unix,
    EscapeBackslash,
}

pub(crate) fn to_path_list(escapes: &[PathEscape], path: &str) -> String {
    let mut out = path.to_string();
    for escape in escapes {
        match escape {
            PathEscape::Unix => {
                #[cfg(windows)]
                {
                    if windows_path::should_use_unix_path() {
                        out = windows_path::to_unix_path_list(&out);
                    }
                }
            }
            PathEscape::EscapeBackslash => {
                out = out.replace('\\', r#"\\"#);
            }
        }
    }
    out
}

#[cfg(windows)]
mod windows_path {
    use once_cell::sync::Lazy;
    use which::which;

    /// Check Unix-like shell env first, then cygpath.exe only if needed.
    /// Only support Unix-like path conversion for MSYS2/Git Bash (MSYSTEM).
    /// Cygwin is NOT supported.
    static SHOULD_USE_UNIX_PATH: Lazy<bool> =
        Lazy::new(|| std::env::var("MSYSTEM").is_ok() && which("cygpath").is_ok());

    pub(super) fn should_use_unix_path() -> bool {
        *SHOULD_USE_UNIX_PATH
    }

    /// Returns true if the path is a canonical Windows drive path (e.g. C:/foo/bar or D:\bar)
    fn is_canonical_windows_drive_path(p: &str) -> bool {
        let p = p.trim();
        p.len() >= 3
            && matches!(p.chars().next(), Some(c) if c.is_ascii_alphabetic())
            && p.chars().nth(1) == Some(':')
            && matches!(p.chars().nth(2), Some('/') | Some('\\'))
    }

    /// Converts a Windows-style path list to Unix-style.
    /// Optimizes for common patterns like C:/foo or D:/bar without calling cygpath.
    /// Falls back to cygpath for other cases.
    /// If cygpath fails, returns the original path string.
    pub(super) fn to_unix_path_list(path: &str) -> String {
        // If all paths are Windows-style (e.g. C:/foo), convert them manually
        if path.split(';').all(is_canonical_windows_drive_path) {
            let unix_paths: Vec<String> = path
                .split(';')
                .map(|p| {
                    let p = p.trim().replace('\\', "/");
                    if let Some(drive) = p.chars().next() {
                        if p.chars().nth(1) == Some(':') && p.chars().nth(2) == Some('/') {
                            // C:/foo/bar → /c/foo/bar
                            format!("/{}{}", drive.to_ascii_lowercase(), &p[2..])
                        } else {
                            p
                        }
                    } else {
                        p
                    }
                })
                .collect();
            return unix_paths.join(":");
        }

        // Otherwise, fallback to cygpath (slow, external process)
        if let Ok(output) = std::process::Command::new("cygpath")
            .args(["-u", "-p", path])
            .output()
        {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    return s.trim().to_string();
                }
            }
        }
        // Fallback: return the original path string if conversion fails
        String::from(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{PathEscape, to_path_list};

    #[test]
    fn test_to_path_list_backslash() {
        let input = r"\foo\bar";
        let output = to_path_list(&[PathEscape::EscapeBackslash], input);
        assert_eq!(output, r"\\foo\\bar");
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::super::windows_path;
        use super::{PathEscape, to_path_list};

        #[test]
        fn test_to_path_list_unix() {
            let input = "C:\\foo;D:\\bar";
            let output = to_path_list(&[PathEscape::Unix], input);
            if windows_path::should_use_unix_path() {
                assert_eq!(output, "/c/foo:/d/bar");
            } else {
                assert_eq!(output, input);
            }
        }
    }

    #[cfg(not(windows))]
    mod unix_tests {
        use super::{PathEscape, to_path_list};

        #[test]
        fn test_to_path_list_unix() {
            let input = "/foo:/bar";
            let output = to_path_list(&[PathEscape::Unix], input);
            assert_eq!(output, input);
        }
    }
}
