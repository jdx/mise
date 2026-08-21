use std::path::{Path, PathBuf};

use std::sync::LazyLock as Lazy;

use crate::env;

pub(crate) static HOME: Lazy<&Path> = Lazy::new(|| &env::HOME);
pub(crate) static CWD: Lazy<Option<PathBuf>> = Lazy::new(|| env::current_dir().ok());
pub(crate) static DATA: Lazy<&Path> = Lazy::new(|| &env::MISE_DATA_DIR);
pub(crate) static CACHE: Lazy<&Path> = Lazy::new(|| &env::MISE_CACHE_DIR);
pub(crate) static CONFIG: Lazy<&Path> = Lazy::new(|| &env::MISE_CONFIG_DIR);
pub(crate) static STATE: Lazy<&Path> = Lazy::new(|| &env::MISE_STATE_DIR);
pub(crate) static SYSTEM_CONFIG: Lazy<&Path> = Lazy::new(|| &env::MISE_SYSTEM_CONFIG_DIR);

pub(crate) static PLUGINS: Lazy<&Path> = Lazy::new(|| &env::MISE_PLUGINS_DIR);
pub(crate) static DOWNLOADS: Lazy<&Path> = Lazy::new(|| &env::MISE_DOWNLOADS_DIR);
pub(crate) static INSTALLS: Lazy<&Path> = Lazy::new(|| &env::MISE_INSTALLS_DIR);
pub(crate) static SHIMS: Lazy<&Path> = Lazy::new(|| &env::MISE_SHIMS_DIR);

pub(crate) static TRACKED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-configs"));
pub(crate) static TRACKED_STUBS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-stubs"));
pub(crate) static TRUSTED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("trusted-configs"));
pub(crate) static IGNORED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("ignored-configs"));
