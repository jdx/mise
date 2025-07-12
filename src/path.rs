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
                    out = windows_path::to_unix_path_list(&out);
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
    use which::which;
    use once_cell::sync::Lazy;

    static CYGPATH_AVAILABLE: Lazy<bool> = Lazy::new(|| which("cygpath").is_ok());

    pub(super) fn to_unix_path_list(path: &str) -> String {
        if *CYGPATH_AVAILABLE {
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
        }
        String::from(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{to_path_list, PathEscape};

    #[test]
    fn test_to_path_list_backslash() {
        let input = r"\foo\bar";
        let output = to_path_list(&[PathEscape::EscapeBackslash], input);
        assert_eq!(output, r"\\foo\\bar");
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::{to_path_list, PathEscape};
        use which::which;
        use once_cell::sync::Lazy;

        static CYGPATH_AVAILABLE: Lazy<bool> = Lazy::new(|| which("cygpath").is_ok());

        #[test]
        fn test_to_path_list_unix() {
            let input = "C:\\foo;D:\\bar";
            let output = to_path_list(&[PathEscape::Unix], input);
            if *CYGPATH_AVAILABLE {
                assert_eq!(output, "/c/foo:/d/bar");
            } else {
                assert_eq!(output, input);
            }
        }
    }

    #[cfg(not(windows))]
    mod unix_tests {
        use super::{to_path_list, PathEscape};

        #[test]
        fn test_to_path_list_unix() {
            let input = "/foo:/bar";
            let output = to_path_list(&[PathEscape::Unix], input);
            assert_eq!(output, input);
        }
    }
}
