use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::env;

pub static CURRENT: Lazy<PathBuf> = Lazy::new(|| env::PWD.clone());
pub static HOME: Lazy<PathBuf> = Lazy::new(|| env::HOME.clone());
pub static ROOT: Lazy<PathBuf> = Lazy::new(|| env::RTX_DATA_DIR.clone());
pub static CACHE: Lazy<PathBuf> = Lazy::new(|| env::RTX_CACHE_DIR.clone());
pub static CONFIG: Lazy<PathBuf> = Lazy::new(|| env::RTX_CONFIG_DIR.clone());
pub static PLUGINS: Lazy<PathBuf> = Lazy::new(|| env::RTX_DATA_DIR.join("plugins"));
pub static DOWNLOADS: Lazy<PathBuf> = Lazy::new(|| env::RTX_DATA_DIR.join("downloads"));
pub static INSTALLS: Lazy<PathBuf> = Lazy::new(|| env::RTX_DATA_DIR.join("installs"));
