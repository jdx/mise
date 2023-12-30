use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;

use crate::env;

pub static HOME: Lazy<&Path> = Lazy::new(|| &env::HOME);
pub static DATA: Lazy<&Path> = Lazy::new(|| &env::RTX_DATA_DIR);
pub static CACHE: Lazy<&Path> = Lazy::new(|| &env::RTX_CACHE_DIR);
pub static CONFIG: Lazy<&Path> = Lazy::new(|| &env::RTX_CONFIG_DIR);
pub static STATE: Lazy<&Path> = Lazy::new(|| &env::RTX_STATE_DIR);
pub static SYSTEM: Lazy<&Path> = Lazy::new(|| &env::RTX_SYSTEM_DIR);

pub static PLUGINS: Lazy<PathBuf> = Lazy::new(|| DATA.join("plugins"));
pub static DOWNLOADS: Lazy<PathBuf> = Lazy::new(|| DATA.join("downloads"));
pub static INSTALLS: Lazy<PathBuf> = Lazy::new(|| DATA.join("installs"));
pub static SHIMS: Lazy<PathBuf> = Lazy::new(|| DATA.join("shims"));

pub static TRACKED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-configs"));
pub static TRUSTED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("trusted-configs"));
