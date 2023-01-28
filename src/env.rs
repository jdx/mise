use std::collections::HashMap;
pub use std::env::*;
use std::ffi::OsString;
use std::path::PathBuf;

use lazy_static::lazy_static;

use crate::env_diff::EnvDiff;
use crate::hook_env::get_pristine_env;

lazy_static! {
    pub static ref ARGS: Vec<String> = args().collect();
    pub static ref HOME: PathBuf = if cfg!(test) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test")
    } else {
        dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    };
    pub static ref PWD: PathBuf = if cfg!(test) {
        HOME.join("cwd")
    } else {
        current_dir().unwrap_or_else(|_| PathBuf::new())
    };
    pub static ref XDG_DATA_HOME: PathBuf = if cfg!(test) {
        HOME.join("data")
    } else {
        var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| HOME.join(".local/share"))
    };
    pub static ref XDG_CONFIG_HOME: PathBuf = if cfg!(test) {
        HOME.join("config")
    } else {
        var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| HOME.join(".config"))
    };
    pub static ref RTX_CONFIG_DIR: PathBuf = if cfg!(test) {
        XDG_CONFIG_HOME.clone()
    } else {
        var_os("RTX_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| XDG_CONFIG_HOME.join("rtx"))
    };
    pub static ref RTX_DATA_DIR: PathBuf = if cfg!(test) {
        XDG_DATA_HOME.clone()
    } else {
        var_os("RTX_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| XDG_DATA_HOME.join("rtx"))
    };
    pub static ref PATH: OsString = var_os("PATH").unwrap_or_default();
    pub static ref SHELL: String = var("SHELL").unwrap_or_else(|_| "sh".into());
    pub static ref RTX_EXE: PathBuf = current_exe().unwrap_or_else(|_| "rtx".into());
    pub static ref RTX_LOG_LEVEL: log::LevelFilter = {
        let log_level = var("RTX_LOG_LEVEL").unwrap_or_default();
        match log_level.as_str() {
            "trace" => log::LevelFilter::Trace,
            "debug" => log::LevelFilter::Debug,
            "info" => log::LevelFilter::Info,
            "warn" => log::LevelFilter::Warn,
            "error" => log::LevelFilter::Error,
            _ => {
                if var_is_true("RTX_DEBUG") {
                    log::LevelFilter::Debug
                } else if var_is_true("RTX_QUIET") {
                    log::LevelFilter::Error
                } else {
                    log::LevelFilter::Info
                }
            }
        }
    };
    pub static ref RTX_MISSING_RUNTIME_BEHAVIOR: Option<String> = if cfg!(test) {
        Some("autoinstall".into())
    } else {
        var("RTX_MISSING_RUNTIME_BEHAVIOR").ok()
    };
    pub static ref __RTX_DIR: Option<PathBuf> = var_os("__RTX_DIR").map(PathBuf::from);
    pub static ref __RTX_DIFF: EnvDiff = get_env_diff();
    pub static ref PRISTINE_ENV: HashMap<String, String> =
        get_pristine_env(&__RTX_DIFF, vars().collect());
    pub static ref RTX_DEFAULT_TOOL_VERSIONS_FILENAME: String = if cfg!(test) {
        ".tool-versions".into()
    } else {
        var("RTX_DEFAULT_TOOL_VERSIONS_FILENAME").unwrap_or_else(|_| ".tool-versions".into())
    };
    pub static ref DIRENV_DIR: Option<String> = var("DIRENV_DIR").ok();
    pub static ref RTX_DISABLE_DIRENV_WARNING: bool = var_is_true("RTX_DISABLE_DIRENV_WARNING");
}

fn get_env_diff() -> EnvDiff {
    let env = vars().collect::<HashMap<_, _>>();
    match env.get("__RTX_DIFF") {
        Some(raw) => EnvDiff::deserialize(raw).unwrap_or_else(|err| {
            warn!("Failed to deserialize __RTX_DIFF: {}", err);
            EnvDiff::default()
        }),
        None => EnvDiff::default(),
    }
}

fn var_is_true(key: &str) -> bool {
    match var(key) {
        Ok(v) => v == "true" || v == "1",
        Err(_) => false,
    }
}
