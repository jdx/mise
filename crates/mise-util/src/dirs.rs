//! Directories mise reads from and writes to.

use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;

use crate::env;
use mise_settings::Settings;

pub static HOME: Lazy<&Path> = Lazy::new(|| &env::HOME);
pub static CWD: Lazy<Option<PathBuf>> = Lazy::new(|| env::current_dir().ok());
pub static DATA: Lazy<&Path> = Lazy::new(|| &env::MISE_DATA_DIR);
pub static CACHE: Lazy<&Path> = Lazy::new(|| &env::MISE_CACHE_DIR);
pub static CONFIG: Lazy<&Path> = Lazy::new(|| &env::MISE_CONFIG_DIR);
pub static STATE: Lazy<&Path> = Lazy::new(|| &env::MISE_STATE_DIR);
pub static SYSTEM_CONFIG: Lazy<&Path> = Lazy::new(|| &env::MISE_SYSTEM_CONFIG_DIR);

pub static PLUGINS: Lazy<&Path> = Lazy::new(|| &env::MISE_PLUGINS_DIR);
pub static DOWNLOADS: Lazy<&Path> = Lazy::new(|| &env::MISE_DOWNLOADS_DIR);
pub static INSTALLS: Lazy<&Path> = Lazy::new(|| &env::MISE_INSTALLS_DIR);
pub static COMMAND_WRAPPERS: Lazy<PathBuf> =
    Lazy::new(|| DATA.join("command-wrappers").join("bin"));

pub static TRACKED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-configs"));
pub static TRACKED_STUBS: Lazy<PathBuf> = Lazy::new(|| STATE.join("tracked-stubs"));
pub static TOOL_PURGATORY: Lazy<PathBuf> = Lazy::new(|| STATE.join("tool-purgatory.json"));
pub static TRUSTED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("trusted-configs"));
pub static IGNORED_CONFIGS: Lazy<PathBuf> = Lazy::new(|| STATE.join("ignored-configs"));

/// The system installs directory: the `system_installs_dir` setting, or
/// `MISE_SYSTEM_INSTALLS_DIR`.
pub fn system_installs_dir(settings: &Settings) -> &Path {
    settings
        .system_installs_dir
        .as_deref()
        .unwrap_or(&env::MISE_SYSTEM_INSTALLS_DIR)
}

/// The user shims directory: the `shims_dir` setting, or `MISE_SHIMS_DIR`.
pub fn shims_dir(settings: &Settings) -> &Path {
    settings
        .shims_dir
        .as_deref()
        .unwrap_or(&env::MISE_SHIMS_DIR)
}

/// The system shims directory: the `system_shims_dir` setting, or the `shims`
/// directory under `MISE_SYSTEM_DATA_DIR`.
pub fn system_shims_dir(settings: &Settings) -> PathBuf {
    settings
        .system_shims_dir
        .clone()
        .unwrap_or_else(|| env::MISE_SYSTEM_DATA_DIR.join("shims"))
}

/// [`shims_dir`] from the loaded settings, or `MISE_SHIMS_DIR` when settings cannot load.
pub fn shims() -> PathBuf {
    Settings::try_get()
        .map(|settings| shims_dir(&settings).to_path_buf())
        .unwrap_or_else(|_| env::MISE_SHIMS_DIR.clone())
}

/// [`system_shims_dir`] from the loaded settings, with the same fallback when settings cannot
/// load.
pub fn system_shims() -> PathBuf {
    Settings::try_get()
        .map(|settings| system_shims_dir(&settings))
        .unwrap_or_else(|_| env::MISE_SYSTEM_DATA_DIR.join("shims"))
}
