use crate::backend::INSTALLED_BACKENDS;
use crate::cli::version::VERSION;
use crate::plugins::{INSTALLED_PLUGINS, VERSION_REGEX};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Mutex;

/// initializes slow parts of mise eagerly in the background
pub fn early_init() {
    time!("early_init");
    rayon::spawn(|| {
        let _ = &*VERSION_REGEX;
    });
    rayon::spawn(|| {
        let _ = &*VERSION;
    });
    rayon::spawn(|| {
        let _ = &*INSTALLED_PLUGINS;
    });
}

pub static CONFIG_FILES: Lazy<Mutex<Vec<PathBuf>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// run after SETTING has been loaded
pub fn post_settings() {
    time!("post_settings");
    rayon::spawn(|| {
        let _ = &*INSTALLED_BACKENDS;
    });
}
