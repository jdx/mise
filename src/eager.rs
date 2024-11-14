use crate::backend;
use crate::cli::version::VERSION;
use crate::plugins::VERSION_REGEX;
use crate::toolset::install_state;
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
        let _ = install_state::init();
    });
}

pub static CONFIG_FILES: Lazy<Mutex<Vec<PathBuf>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// run after SETTING has been loaded
pub fn post_settings() {
    time!("post_settings");
    rayon::spawn(|| {
        backend::load_tools();
    });
}
