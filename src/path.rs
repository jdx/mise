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
