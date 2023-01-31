use std::collections::HashMap;
pub use std::env::*;
use std::ffi::OsString;
use std::path::PathBuf;

use lazy_static::lazy_static;

use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};

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
    pub static ref RTX_TMP_DIR: PathBuf = temp_dir().join("rtx");
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
                if *RTX_TRACE {
                    log::LevelFilter::Trace
                } else if *RTX_DEBUG {
                    log::LevelFilter::Debug
                } else if *RTX_QUIET {
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
    pub static ref __RTX_DIFF: EnvDiff = get_env_diff();
    pub static ref RTX_QUIET: bool = var_is_true("RTX_QUIET");
    pub static ref RTX_DEBUG: bool = var_is_true("RTX_DEBUG");
    pub static ref RTX_TRACE: bool = var_is_true("RTX_TRACE");
    pub static ref PRISTINE_ENV: HashMap<String, String> =
        get_pristine_env(&__RTX_DIFF, vars().collect());
    pub static ref PATH: Vec<PathBuf> =
        split_paths(&var_os("PATH").unwrap_or(OsString::new())).collect();
    pub static ref RTX_DEFAULT_TOOL_VERSIONS_FILENAME: String = if cfg!(test) {
        ".tool-versions".into()
    } else {
        var("RTX_DEFAULT_TOOL_VERSIONS_FILENAME").unwrap_or_else(|_| ".tool-versions".into())
    };
    pub static ref DIRENV_DIR: Option<String> = var("DIRENV_DIR").ok();
    pub static ref DIRENV_DIFF: Option<String> = var("DIRENV_DIFF").ok();
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

/// this returns the environment as if __RTX_DIFF was reversed.
/// putting the shell back into a state before hook-env was run
fn get_pristine_env(
    rtx_diff: &EnvDiff,
    orig_env: HashMap<String, String>,
) -> HashMap<String, String> {
    let patches = rtx_diff.reverse().to_patches();
    apply_patches(&orig_env, &patches)
}

fn apply_patches(
    env: &HashMap<String, String>,
    patches: &EnvDiffPatches,
) -> HashMap<String, String> {
    let mut new_env = env.clone();
    for patch in patches {
        match patch {
            EnvDiffOperation::Add(k, v) | EnvDiffOperation::Change(k, v) => {
                new_env.insert(k.into(), v.into());
            }
            EnvDiffOperation::Remove(k) => {
                new_env.remove(k);
            }
        }
    }

    new_env
}

#[test]
fn test_apply_patches() {
    let mut env = HashMap::new();
    env.insert("foo".into(), "bar".into());
    env.insert("baz".into(), "qux".into());
    let patches = vec![
        EnvDiffOperation::Add("foo".into(), "bar".into()),
        EnvDiffOperation::Change("baz".into(), "qux".into()),
        EnvDiffOperation::Remove("quux".into()),
    ];
    let new_env = apply_patches(&env, &patches);
    assert_eq!(new_env.len(), 2);
    assert_eq!(new_env.get("foo").unwrap(), "bar");
    assert_eq!(new_env.get("baz").unwrap(), "qux");
}
