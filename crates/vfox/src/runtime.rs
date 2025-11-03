use crate::config::{arch, os};
use mlua::{UserData, UserDataFields};
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub(crate) struct Runtime {
    pub(crate) os: String,
    pub(crate) arch: String,
    pub(crate) version: String,
    pub(crate) plugin_dir_path: PathBuf,
}

static RUNTIME: Lazy<Mutex<Runtime>> = Lazy::new(|| {
    Mutex::new(Runtime {
        os: os(),
        arch: arch(),
        version: "0.6.0".to_string(), // https://github.com/version-fox/vfox/releases
        plugin_dir_path: PathBuf::new(),
    })
});

impl Runtime {
    pub(crate) fn get(plugin_dir_path: PathBuf) -> Runtime {
        let mut runtime = RUNTIME.lock().unwrap().clone();
        runtime.plugin_dir_path = plugin_dir_path;
        runtime
    }

    #[cfg(test)]
    pub(crate) fn set_os(os: String) {
        let mut runtime = RUNTIME.lock().unwrap();
        runtime.os = os;
    }

    #[cfg(test)]
    pub(crate) fn set_arch(arch: String) {
        let mut runtime = RUNTIME.lock().unwrap();
        runtime.arch = arch;
    }

    #[cfg(test)]
    pub(crate) fn reset() {
        let mut runtime = RUNTIME.lock().unwrap();
        runtime.os = os();
        runtime.arch = arch();
    }
}

impl UserData for Runtime {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("osType", |_, t| Ok(t.os.clone()));
        fields.add_field_method_get("archType", |_, t| Ok(t.arch.clone()));
        fields.add_field_method_get("version", |_, t| Ok(t.version.clone()));
        fields.add_field_method_get("pluginDirPath", |_, t| Ok(t.plugin_dir_path.clone()));
    }
}
