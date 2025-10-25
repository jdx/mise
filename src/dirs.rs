use std::path::{Path, PathBuf};

use std::sync::LazyLock as Lazy;

use crate::env;

pub static HOME: Lazy<&Path> = Lazy::new(|| &env::HOME);
pub static CWD: Lazy<Option<PathBuf>> = Lazy::new(|| env::current_dir().ok());
pub static DATA: Lazy<&Path> = Lazy::new(|| &env::MISE_DATA_DIR);
pub static CACHE: Lazy<&Path> = Lazy::new(|| &env::MISE_CACHE_DIR);
pub static CONFIG: Lazy<&Path> = Lazy::new(|| &env::MISE_CONFIG_DIR);
pub static STATE: Lazy<&Path> = Lazy::new(|| &env::MISE_STATE_DIR);
pub static SYSTEM: Lazy<Option<&Path>> =
    Lazy::new(|| env::MISE_SYSTEM_DIR.as_ref().map(|p| p.as_path()));

pub static PLUGINS: Lazy<&Path> = Lazy::new(|| &env::MISE_PLUGINS_DIR);
pub static DOWNLOADS: Lazy<&Path> = Lazy::new(|| &env::MISE_DOWNLOADS_DIR);
pub static INSTALLS: Lazy<&Path> = Lazy::new(|| &env::MISE_INSTALLS_DIR);
pub static SHIMS: Lazy<&Path> = Lazy::new(|| &env::MISE_SHIMS_DIR);

pub static TRACKED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-configs"));
pub static TRUSTED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("trusted-configs"));
pub static IGNORED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("ignored-configs"));
