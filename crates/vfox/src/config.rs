use std::env::consts::{ARCH, OS};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone)]
pub struct Config {
    pub plugin_dir: PathBuf,
}

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

impl Config {
    pub fn get() -> Self {
        Self::_get().as_ref().unwrap().clone()
    }

    fn _get() -> MutexGuard<'static, Option<Config>> {
        let mut config = CONFIG.lock().unwrap();
        if config.is_none() {
            let home = homedir::my_home()
                .ok()
                .flatten()
                .unwrap_or_else(|| PathBuf::from("/"));
            *config = Some(Config {
                plugin_dir: home.join(".version-fox/plugin"),
            });
        }
        config
    }
}

pub fn os() -> String {
    match OS {
        "macos" => "darwin".to_string(),
        os => os.to_string(),
    }
}

pub fn arch() -> String {
    match ARCH {
        "aarch64" => "arm64".to_string(),
        "x86_64" => "amd64".to_string(),
        arch => arch.to_string(),
    }
}
