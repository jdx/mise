use std::path::PathBuf;

use lazy_static::lazy_static;

use crate::env;

lazy_static! {
    pub static ref CURRENT: PathBuf = env::PWD.clone();
    pub static ref HOME: PathBuf = env::HOME.clone();
    pub static ref ROOT: PathBuf = env::RTX_DATA_DIR.clone();
    pub static ref CACHE: PathBuf = env::RTX_CACHE_DIR.clone();
    pub static ref CONFIG: PathBuf = env::RTX_CONFIG_DIR.clone();
    pub static ref PLUGINS: PathBuf = env::RTX_DATA_DIR.join("plugins");
    pub static ref DOWNLOADS: PathBuf = env::RTX_DATA_DIR.join("downloads");
    pub static ref INSTALLS: PathBuf = env::RTX_DATA_DIR.join("installs");
}
