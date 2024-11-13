use crate::cli::version::VERSION;
use crate::plugins::VERSION_REGEX;
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
    // rayon::spawn(|| {
    //     let _ = install_state::list_backends();
    // });
}

pub static CONFIG_FILES: Lazy<Mutex<Vec<PathBuf>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// run after SETTING has been loaded
pub fn post_settings() {
    time!("post_settings");
    // rayon::spawn(|| {
    //     let _ = load_tools();
    // });
}
