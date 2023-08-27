use std::collections::{BTreeSet, HashMap, HashSet};
pub use std::env::*;
use std::path::PathBuf;
use std::time::Duration;

use crate::duration::HOURLY;
use itertools::Itertools;
use log::LevelFilter;
use once_cell::sync::Lazy;

use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::file::replace_path;

pub static ARGS: Lazy<Vec<String>> = Lazy::new(|| args().collect());
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));

// paths and directories
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::home_dir().unwrap_or_else(|| PathBuf::from("/")));
pub static PWD: Lazy<PathBuf> = Lazy::new(|| current_dir().unwrap_or_else(|_| PathBuf::new()));
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| dirs_next::cache_dir().unwrap_or_else(|| HOME.join(".cache")));
pub static XDG_DATA_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_DATA_HOME").unwrap_or_else(|| HOME.join(".local/share")));
pub static XDG_CONFIG_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config")));
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
pub static RTX_LOG_LEVEL: Lazy<LevelFilter> = Lazy::new(log_level);
pub static RTX_LOG_FILE_LEVEL: Lazy<LevelFilter> = Lazy::new(log_file_level);
pub static RTX_MISSING_RUNTIME_BEHAVIOR: Lazy<Option<String>> =
    Lazy::new(|| var("RTX_MISSING_RUNTIME_BEHAVIOR").ok());
pub static RTX_VERBOSE: Lazy<bool> =
    Lazy::new(|| *RTX_LOG_LEVEL > LevelFilter::Info || var_is_true("RTX_VERBOSE"));
pub static RTX_JOBS: Lazy<usize> = Lazy::new(|| {
    var("RTX_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(4)
});
pub static RTX_FETCH_REMOTE_VERSIONS_TIMEOUT: Lazy<Duration> = Lazy::new(|| {
    var_duration("RTX_FETCH_REMOTE_VERSIONS_TIMEOUT").unwrap_or(Duration::from_secs(10))
});

/// duration that remote version cache is kept for
/// for "fast" commands (represented by PREFER_STALE), these are always
/// cached. For "slow" commands like `rtx ls-remote` or `rtx install`:
/// - if RTX_FETCH_REMOTE_VERSIONS_CACHE is set, use that
/// - if RTX_FETCH_REMOTE_VERSIONS_CACHE is not set, use HOURLY
pub static RTX_FETCH_REMOTE_VERSIONS_CACHE: Lazy<Option<Duration>> = Lazy::new(|| {
    if *PREFER_STALE {
        None
    } else {
        Some(var_duration("RTX_FETCH_REMOTE_VERSIONS_CACHE").unwrap_or(HOURLY))
    }
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
pub static RTX_ASDF_COMPAT: Lazy<bool> = Lazy::new(|| var_is_true("RTX_ASDF_COMPAT"));
pub static RTX_SHORTHANDS_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("RTX_SHORTHANDS_FILE"));
pub static RTX_DISABLE_DEFAULT_SHORTHANDS: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_DISABLE_DEFAULT_SHORTHANDS"));
pub static RTX_LEGACY_VERSION_FILE: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("RTX_LEGACY_VERSION_FILE"));
pub static RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS: Lazy<BTreeSet<String>> = Lazy::new(|| {
    var("RTX_LEGACY_VERSION_FILE_DISABLE_TOOLS")
        .map(|v| v.split(',').map(|s| s.to_string()).collect())
        .unwrap_or_default()
});
pub static RTX_DISABLE_TOOLS: Lazy<BTreeSet<String>> = Lazy::new(|| {
    var("RTX_DISABLE_TOOLS")
        .map(|v| v.split(',').map(|s| s.to_string()).collect())
        .unwrap_or_default()
});
pub static RTX_RAW: Lazy<bool> = Lazy::new(|| var_is_true("RTX_RAW"));
pub static RTX_YES: Lazy<bool> = Lazy::new(|| *CI || var_is_true("RTX_YES"));
pub static RTX_TRUSTED_CONFIG_PATHS: Lazy<BTreeSet<PathBuf>> = Lazy::new(|| {
    var("RTX_TRUSTED_CONFIG_PATHS")
        .map(|v| split_paths(&v).collect())
        .unwrap_or_default()
});
pub static RTX_ALWAYS_KEEP_DOWNLOAD: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_ALWAYS_KEEP_DOWNLOAD"));
pub static RTX_ALWAYS_KEEP_INSTALL: Lazy<bool> =
    Lazy::new(|| var_is_true("RTX_ALWAYS_KEEP_INSTALL"));

#[allow(unused)]
pub static GITHUB_API_TOKEN: Lazy<Option<String>> = Lazy::new(|| var("GITHUB_API_TOKEN").ok());

// python
pub static RTX_PYENV_REPO: Lazy<String> = Lazy::new(|| {
    var("RTX_PYENV_REPO").unwrap_or_else(|_| "https://github.com/pyenv/pyenv.git".into())
});
pub static RTX_PYTHON_PATCH_URL: Lazy<Option<String>> =
    Lazy::new(|| var("RTX_PYTHON_PATCH_URL").ok());
pub static RTX_PYTHON_PATCHES_DIRECTORY: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("RTX_PYTHON_PATCHES_DIRECTORY"));
pub static RTX_PYTHON_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("RTX_PYTHON_DEFAULT_PACKAGES_FILE")
        .unwrap_or_else(|| HOME.join(".default-python-packages"))
});
pub static PYENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("PYENV_ROOT").unwrap_or_else(|| HOME.join(".pyenv")));

// node
pub static RTX_NODE_BUILD_REPO: Lazy<String> = Lazy::new(|| {
    var("RTX_NODE_BUILD_REPO").unwrap_or_else(|_| "https://github.com/nodenv/node-build.git".into())
});
pub static RTX_NODE_CONCURRENCY: Lazy<usize> = Lazy::new(|| {
    var("RTX_NODE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(num_cpus::get() / 2)
        .max(1)
});
pub static RTX_NODE_VERBOSE_INSTALL: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("RTX_NODE_VERBOSE_INSTALL"));
pub static RTX_NODE_FORCE_COMPILE: Lazy<bool> = Lazy::new(|| var_is_true("RTX_NODE_FORCE_COMPILE"));
pub static RTX_NODE_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("RTX_NODE_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| {
        let p = HOME.join(".default-nodejs-packages");
        if p.exists() {
            return p;
        }
        let p = HOME.join(".default-node-packages");
        if p.exists() {
            return p;
        }
        HOME.join(".default-npm-packages")
    })
});
pub static NVM_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("NVM_DIR").unwrap_or_else(|| HOME.join(".nvm")));
pub static NODENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("NODENV_ROOT").unwrap_or_else(|| HOME.join(".nodenv")));

// ruby
pub static RTX_RUBY_BUILD_REPO: Lazy<String> = Lazy::new(|| {
    var("RTX_RUBY_BUILD_REPO").unwrap_or_else(|_| "https://github.com/rbenv/ruby-build.git".into())
});
pub static RTX_RUBY_INSTALL_REPO: Lazy<String> = Lazy::new(|| {
    var("RTX_RUBY_INSTALL_REPO")
        .unwrap_or_else(|_| "https://github.com/postmodern/ruby-install.git".into())
});
pub static RTX_RUBY_INSTALL: Lazy<bool> = Lazy::new(|| var_is_true("RTX_RUBY_INSTALL"));
pub static RTX_RUBY_APPLY_PATCHES: Lazy<Option<String>> =
    Lazy::new(|| var("RTX_RUBY_APPLY_PATCHES").ok());
pub static RTX_RUBY_VERBOSE_INSTALL: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("RTX_RUBY_VERBOSE_INSTALL"));
pub static RTX_RUBY_INSTALL_OPTS: Lazy<Result<Vec<String>, shell_words::ParseError>> =
    Lazy::new(|| shell_words::split(&var("RTX_RUBY_INSTALL_OPTS").unwrap_or_default()));
pub static RTX_RUBY_BUILD_OPTS: Lazy<Result<Vec<String>, shell_words::ParseError>> =
    Lazy::new(|| shell_words::split(&var("RTX_RUBY_BUILD_OPTS").unwrap_or_default()));
pub static RTX_RUBY_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("RTX_RUBY_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| HOME.join(".default-gems"))
});

// go
pub static RTX_GO_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("RTX_GO_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| HOME.join(".default-go-packages"))
});
pub static RTX_GO_SKIP_CHECKSUM: Lazy<bool> = Lazy::new(|| var_is_true("RTX_GO_SKIP_CHECKSUM"));
pub static RTX_GO_REPO: Lazy<String> =
    Lazy::new(|| var("RTX_GO_REPO").unwrap_or_else(|_| "https://github.com/golang/go".into()));
pub static RTX_GO_DOWNLOAD_MIRROR: Lazy<String> = Lazy::new(|| {
    var("RTX_GO_DOWNLOAD_MIRROR").unwrap_or_else(|_| "https://dl.google.com/go".into())
});
pub static RTX_GO_SET_GOROOT: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("RTX_GO_SET_GOROOT"));
pub static RTX_GO_SET_GOPATH: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("RTX_GO_SET_GOPATH"));

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

fn var_option_bool(key: &str) -> Option<bool> {
    match var(key) {
        Ok(_) if var_is_true(key) => Some(true),
        Ok(_) if var_is_false(key) => Some(false),
        Ok(v) => {
            warn!("Invalid value for env var {}={}", key, v);
            None
        }
        _ => None,
    }
}

fn var_path(key: &str) -> Option<PathBuf> {
    var_os(key).map(PathBuf::from).map(replace_path)
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

fn var_duration(key: &str) -> Option<Duration> {
    var(key)
        .ok()
        .map(|v| v.parse::<humantime::Duration>().unwrap().into())
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
    let binding = String::new();
    let c = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        .nth(1)
        .unwrap_or(&binding);
    return [
        "env", "hook-env", "x", "exec", "direnv", "activate", "current", "ls", "where",
    ]
    .contains(&c.as_str());
}

fn log_level() -> LevelFilter {
    if var_is_true("RTX_QUIET") {
        set_var("RTX_LOG_LEVEL", "warn");
    }
    if var_is_true("RTX_DEBUG") {
        set_var("RTX_LOG_LEVEL", "debug");
    }
    if var_is_true("RTX_TRACE") {
        set_var("RTX_LOG_LEVEL", "trace");
    }
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
            set_var("RTX_LOG_LEVEL", "debug");
        }
        if arg == "--trace" {
            set_var("RTX_LOG_LEVEL", "trace");
        }
    }
    let log_level = var("RTX_LOG_LEVEL")
        .unwrap_or_default()
        .parse::<LevelFilter>()
        .unwrap_or(LevelFilter::Info);
    // set RTX_DEBUG/RTX_TRACE for plugins to use
    match log_level {
        LevelFilter::Trace => {
            set_var("RTX_TRACE", "1");
            set_var("RTX_DEBUG", "1");
        }
        LevelFilter::Debug => {
            set_var("RTX_DEBUG", "1");
        }
        _ => {}
    }

    log_level
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
