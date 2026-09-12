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
pub(crate) static COMMAND_WRAPPERS: Lazy<PathBuf> =
    Lazy::new(|| DATA.join("command-wrappers").join("bin"));

pub(crate) static TRACKED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-configs"));
pub(crate) static TRACKED_STUBS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-stubs"));
pub(crate) static TOOL_PURGATORY: Lazy<PathBuf> = Lazy::new(|| STATE.join("tool-purgatory.json"));
pub(crate) static TRUSTED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("trusted-configs"));
pub(crate) static IGNORED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("ignored-configs"));

pub(crate) fn shims() -> PathBuf {
    crate::config::Settings::try_get()
        .map(|settings| settings.shims_dir().to_path_buf())
        .unwrap_or_else(|_| env::MISE_SHIMS_DIR.clone())
}

pub(crate) fn system_shims() -> PathBuf {
    crate::config::Settings::try_get()
        .map(|settings| settings.system_shims_dir())
        .unwrap_or_else(|_| env::MISE_SYSTEM_DATA_DIR.join("shims"))
}
