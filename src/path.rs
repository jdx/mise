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

    // Check Unix-like shell env first, then cygpath.exe only if needed.
    static SHOULD_USE_UNIX_PATH: Lazy<bool> = Lazy::new(|| {
        let unix_env = std::env::var("MSYSTEM").is_ok()
            || std::env::var("OSTYPE").map_or(false, |v| v == "cygwin");
        if !unix_env {
            return false;
        }
        which("cygpath").is_ok()
    });

    pub(super) fn should_use_unix_path() -> bool {
        *SHOULD_USE_UNIX_PATH
    }

    pub(super) fn to_unix_path_list(path: &str) -> String {
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
