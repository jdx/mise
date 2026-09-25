use crate::config::SettingsExt;
use std::path::PathBuf;

use crate::env;

pub(crate) use mise_util::dirs::*;

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
