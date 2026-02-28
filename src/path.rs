pub use std::path::*;

use crate::dirs;

pub trait PathExt {
    /// replaces $HOME with "~"
    fn display_user(&self) -> String;
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
}
