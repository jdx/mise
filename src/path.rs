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

#[cfg(windows)]
pub(crate) fn to_unix_path_list(path: &str) -> String {
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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    #[test]
    fn test_to_unix_path_list() {
        let input = "C:\\foo;D:\\bar";
        let cygpath_available = std::process::Command::new("cygpath")
            .arg("--version")
            .output()
            .is_ok();

        let output = super::to_unix_path_list(input);

        if cygpath_available {
            assert_eq!(output, "/c/foo:/d/bar");
        } else {
            assert_eq!(output, input);
        }
    }
}
