use std::collections::{HashMap, HashSet};
pub use std::env::*;
use std::path::PathBuf;

use itertools::Itertools;
use log::LevelFilter;
use once_cell::sync::Lazy;

use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};

pub static ARGS: Lazy<Vec<String>> = Lazy::new(|| args().collect());
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));

// paths and directories
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("/")));
pub static PWD: Lazy<PathBuf> = Lazy::new(|| current_dir().unwrap_or_else(|_| PathBuf::new()));
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::cache_dir().unwrap_or_else(|| HOME.join(".cache")));
pub static XDG_DATA_HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::data_dir().unwrap_or_else(|| HOME.join(".local/share")));
pub static XDG_CONFIG_HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::config_dir().unwrap_or_else(|| HOME.join(".config")));
pub static RTX_CACHE_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("RTX_CACHE_DIR").unwrap_or_else(|| XDG_CACHE_HOME.join("rtx")));
pub static RTX_CONFIG_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("RTX_CONFIG_DIR").unwrap_or_else(|| XDG_CONFIG_HOME.join("rtx")));
pub static RTX_DATA_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("RTX_DATA_DIR").unwrap_or_else(|| XDG_DATA_HOME.join("rtx")));
pub static RTX_TMP_DIR: Lazy<PathBuf> = Lazy::new(|| temp_dir().join("rtx"));

pub static RTX_DEFAULT_TOOL_VERSIONS_FILENAME: Lazy<String> = Lazy::new(|| {
    var("RTX_DEFAULT_TOOL_VERSIONS_FILENAME").unwrap_or_else(|_| ".tool-versions".into())
});
pub static RTX_DEFAULT_CONFIG_FILENAME: Lazy<String> =
    Lazy::new(|| var("RTX_DEFAULT_CONFIG_FILENAME").unwrap_or_else(|_| ".rtx.toml".into()));
pub static RTX_ENV: Lazy<Option<String>> = Lazy::new(|| var("RTX_ENV").ok());
pub static RTX_CONFIG_FILE: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("RTX_CONFIG_FILE"));
pub static RTX_USE_TOML: Lazy<bool> = Lazy::new(|| var_is_true("RTX_USE_TOML"));
pub static RTX_EXE: Lazy<PathBuf> = Lazy::new(|| current_exe().unwrap_or_else(|_| "rtx".into()));
pub static RTX_EXPERIMENTAL_CORE_PLUGINS: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_EXPERIMENTAL_CORE_PLUGINS"));
pub static RTX_LOG_LEVEL: Lazy<LevelFilter> = Lazy::new(log_level);
pub static RTX_LOG_FILE_LEVEL: Lazy<LevelFilter> = Lazy::new(log_file_level);
pub static RTX_MISSING_RUNTIME_BEHAVIOR: Lazy<Option<String>> =
    Lazy::new(|| var("RTX_MISSING_RUNTIME_BEHAVIOR").ok());
pub static RTX_QUIET: Lazy<bool> = Lazy::new(|| var_is_true("RTX_QUIET"));
pub static RTX_DEBUG: Lazy<bool> = Lazy::new(|| var_is_true("RTX_DEBUG"));
pub static RTX_TRACE: Lazy<bool> = Lazy::new(|| var_is_true("RTX_TRACE"));
pub static RTX_VERBOSE: Lazy<bool> =
    Lazy::new(|| *RTX_DEBUG || *RTX_TRACE || var_is_true("RTX_VERBOSE"));
pub static RTX_JOBS: Lazy<usize> = Lazy::new(|| {
    var("RTX_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4)
});

/// true if inside a script like bin/exec-env or bin/install
/// used to prevent infinite loops
pub static __RTX_SCRIPT: Lazy<bool> = Lazy::new(|| var_is_true("__RTX_SCRIPT"));
pub static __RTX_DIFF: Lazy<EnvDiff> = Lazy::new(get_env_diff);
pub static CI: Lazy<bool> = Lazy::new(|| var_is_true("CI"));
pub static PREFER_STALE: Lazy<bool> = Lazy::new(|| prefer_stale(&ARGS));

/// essentially, this is whether we show spinners or build output on runtime install
pub static PRISTINE_ENV: Lazy<HashMap<String, String>> =
    Lazy::new(|| get_pristine_env(&__RTX_DIFF, vars().collect()));
pub static PATH: Lazy<Vec<PathBuf>> = Lazy::new(|| match PRISTINE_ENV.get("PATH") {
    Some(path) => split_paths(path).collect(),
    None => vec![],
});
pub static DIRENV_DIFF: Lazy<Option<String>> = Lazy::new(|| var("DIRENV_DIFF").ok());
pub static RTX_CONFIRM: Lazy<Confirm> = Lazy::new(|| var_confirm("RTX_CONFIRM"));
pub static RTX_EXPERIMENTAL: Lazy<bool> = Lazy::new(|| var_is_true("RTX_EXPERIMENTAL"));
pub static RTX_HIDE_UPDATE_WARNING: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_HIDE_UPDATE_WARNING"));
pub static RTX_ASDF_COMPAT: Lazy<bool> = Lazy::new(|| var_is_true("RTX_ASDF_COMPAT"));
pub static RTX_SHORTHANDS_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("RTX_SHORTHANDS_FILE"));
pub static RTX_DISABLE_DEFAULT_SHORTHANDS: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_DISABLE_DEFAULT_SHORTHANDS"));
pub static RTX_SHIMS_DIR: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("RTX_SHIMS_DIR"));
pub static RTX_RAW: Lazy<bool> = Lazy::new(|| var_is_true("RTX_RAW"));
pub static RTX_TRUSTED_CONFIG_PATHS: Lazy<Vec<PathBuf>> = Lazy::new(|| {
    var("RTX_TRUSTED_CONFIG_PATHS")
        .map(|v| split_paths(&v).collect())
        .unwrap_or_default()
});
#[allow(unused)]
pub static GITHUB_API_TOKEN: Lazy<Option<String>> = Lazy::new(|| var("GITHUB_API_TOKEN").ok());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirm {
    Yes,
    No,
    Prompt,
}

fn get_env_diff() -> EnvDiff {
    let env = vars().collect::<HashMap<_, _>>();
    match env.get("__RTX_DIFF") {
        Some(raw) => EnvDiff::deserialize(raw).unwrap_or_else(|err| {
            warn!("Failed to deserialize __RTX_DIFF: {:#}", err);
            EnvDiff::default()
        }),
        None => EnvDiff::default(),
    }
}

fn var_is_true(key: &str) -> bool {
    match var(key) {
        Ok(v) => {
            let v = v.to_lowercase();
            v == "y" || v == "yes" || v == "true" || v == "1" || v == "on"
        }
        Err(_) => false,
    }
}

fn var_is_false(key: &str) -> bool {
    match var(key) {
        Ok(v) => {
            let v = v.to_lowercase();
            v == "n" || v == "no" || v == "false" || v == "0" || v == "off"
        }
        Err(_) => false,
    }
}

fn var_path(key: &str) -> Option<PathBuf> {
    var_os(key).map(PathBuf::from)
}

fn var_confirm(key: &str) -> Confirm {
    if var_is_true(key) {
        Confirm::Yes
    } else if var_is_false(key) {
        Confirm::No
    } else {
        Confirm::Prompt
    }
}

/// this returns the environment as if __RTX_DIFF was reversed.
/// putting the shell back into a state before hook-env was run
fn get_pristine_env(
    rtx_diff: &EnvDiff,
    orig_env: HashMap<String, String>,
) -> HashMap<String, String> {
    let patches = rtx_diff.reverse().to_patches();
    let mut env = apply_patches(&orig_env, &patches);

    // get the current path as a vector
    let path = match env.get("PATH") {
        Some(path) => split_paths(path).collect(),
        None => vec![],
    };
    // get the paths that were removed by rtx as a hashset
    let mut to_remove = rtx_diff.path.iter().collect::<HashSet<_>>();

    // remove those paths that were added by rtx, but only once (the first time)
    let path = path
        .into_iter()
        .filter(|p| !to_remove.remove(p))
        .collect_vec();

    // put the pristine PATH back into the environment
    env.insert(
        "PATH".into(),
        join_paths(path).unwrap().to_string_lossy().to_string(),
    );
    env
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

/// returns true if new runtime versions should not be fetched
fn prefer_stale(args: &[String]) -> bool {
    if let Some(c) = args.get(1) {
        return vec![
            "env", "hook-env", "x", "exec", "direnv", "activate", "current", "ls", "where",
        ]
        .contains(&c.as_str());
    }
    false
}

fn log_level() -> LevelFilter {
    for (i, arg) in ARGS.iter().enumerate() {
        if arg == "--" {
            break;
        }
        if let Some(("--log-level", level)) = arg.split_once('=') {
            set_var("RTX_LOG_LEVEL", level);
        }
        if arg == "--log-level" {
            if let Some(level) = ARGS.get(i + 1) {
                set_var("RTX_LOG_LEVEL", level);
            }
        }
        if arg == "--debug" {
            set_var("RTX_DEBUG", "1");
        }
        if arg == "--trace" {
            set_var("RTX_TRACE", "1");
        }
    }
    let log_level = var("RTX_LOG_LEVEL").unwrap_or_default();
    match log_level.parse::<LevelFilter>() {
        Ok(level) => level,
        _ => {
            if *RTX_TRACE {
                LevelFilter::Trace
            } else if *RTX_DEBUG {
                LevelFilter::Debug
            } else if *RTX_QUIET {
                LevelFilter::Warn
            } else {
                LevelFilter::Info
            }
        }
    }
}

fn log_file_level() -> LevelFilter {
    let log_level = var("RTX_LOG_FILE_LEVEL").unwrap_or_default();
    match log_level.parse::<LevelFilter>() {
        Ok(level) => level,
        _ => *RTX_LOG_LEVEL,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::env::apply_patches;
    use crate::env_diff::EnvDiffOperation;

    use super::*;

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

    #[test]
    fn test_var_path() {
        set_var("RTX_TEST_PATH", "/foo/bar");
        assert_eq!(
            var_path("RTX_TEST_PATH").unwrap(),
            PathBuf::from("/foo/bar")
        );
        remove_var("RTX_TEST_PATH");
    }

    #[test]
    fn test_var_confirm() {
        set_var("RTX_TEST_CONFIRM", "true");
        assert_eq!(var_confirm("RTX_TEST_CONFIRM"), Confirm::Yes);
        remove_var("RTX_TEST_CONFIRM");
        set_var("RTX_TEST_CONFIRM", "false");
        assert_eq!(var_confirm("RTX_TEST_CONFIRM"), Confirm::No);
        remove_var("RTX_TEST_CONFIRM");
        set_var("RTX_TEST_CONFIRM", "prompt");
        assert_eq!(var_confirm("RTX_TEST_CONFIRM"), Confirm::Prompt);
        remove_var("RTX_TEST_CONFIRM");
        assert_eq!(var_confirm("RTX_TEST_CONFIRM"), Confirm::Prompt);
    }
}
