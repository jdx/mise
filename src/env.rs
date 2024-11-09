use std::collections::{HashMap, HashSet};
pub use std::env::*;
use std::path::PathBuf;
use std::string::ToString;
use std::sync::RwLock;
use std::{path, process};

use crate::cli::args::PROFILE_ARG;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::file::replace_path;
use crate::hook_env::{deserialize_watches, HookEnvWatches};
use itertools::Itertools;
use log::LevelFilter;
use once_cell::sync::Lazy;

pub static ARGS: RwLock<Vec<String>> = RwLock::new(vec![]);
#[cfg(unix)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));
#[cfg(windows)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));

// paths and directories
#[cfg(test)]
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"));
#[cfg(not(test))]
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| home::home_dir().unwrap_or_else(|| PathBuf::from("/")));

pub static EDITOR: Lazy<String> =
    Lazy::new(|| var("VISUAL").unwrap_or_else(|_| var("EDITOR").unwrap_or_else(|_| "nano".into())));

#[cfg(macos)]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join("Library/Caches")));
#[cfg(windows)]
pub static XDG_CACHE_HOME: Lazy<PathBuf> = Lazy::new(|| {
    var_path("XDG_CACHE_HOME")
        .or_else(|| var_path("TEMP"))
        .unwrap_or_else(|| temp_dir())
});
#[cfg(all(not(windows), not(macos)))]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join(".cache")));
pub static XDG_CONFIG_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config")));
#[cfg(unix)]
pub static XDG_DATA_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_DATA_HOME").unwrap_or_else(|| HOME.join(".local").join("share")));
#[cfg(windows)]
pub static XDG_DATA_HOME: Lazy<PathBuf> = Lazy::new(|| {
    var_path("XDG_DATA_HOME")
        .or(var_path("LOCALAPPDATA"))
        .unwrap_or_else(|| HOME.join("AppData/Local"))
});
pub static XDG_STATE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_STATE_HOME").unwrap_or_else(|| HOME.join(".local").join("state")));

pub static MISE_CACHE_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_CACHE_DIR").unwrap_or_else(|| XDG_CACHE_HOME.join("mise")));
pub static MISE_CONFIG_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_CONFIG_DIR").unwrap_or_else(|| XDG_CONFIG_HOME.join("mise")));
pub static MISE_DATA_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_DATA_DIR").unwrap_or_else(|| XDG_DATA_HOME.join("mise")));
pub static MISE_STATE_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_STATE_DIR").unwrap_or_else(|| XDG_STATE_HOME.join("mise")));
pub static MISE_TMP_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_TMP_DIR").unwrap_or_else(|| temp_dir().join("mise")));
pub static MISE_SYSTEM_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_SYSTEM_DIR").unwrap_or_else(|| PathBuf::from("/etc/mise")));

// data subdirs
pub static MISE_INSTALLS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_INSTALLS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("installs")));
pub static MISE_DOWNLOADS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_DOWNLOADS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("downloads")));
pub static MISE_PLUGINS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_PLUGINS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("plugins")));
pub static MISE_SHIMS_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_SHIMS_DIR").unwrap_or_else(|| MISE_DATA_DIR.join("shims")));

pub static MISE_DEFAULT_TOOL_VERSIONS_FILENAME: Lazy<String> = Lazy::new(|| {
    var("MISE_DEFAULT_TOOL_VERSIONS_FILENAME").unwrap_or_else(|_| ".tool-versions".into())
});
pub static MISE_DEFAULT_CONFIG_FILENAME: Lazy<String> =
    Lazy::new(|| var("MISE_DEFAULT_CONFIG_FILENAME").unwrap_or_else(|_| "mise.toml".into()));
pub static MISE_PROFILE: Lazy<Option<String>> = Lazy::new(|| environment(&ARGS.read().unwrap()));
pub static MISE_SETTINGS_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_SETTINGS_FILE").unwrap_or_else(|| MISE_CONFIG_DIR.join("settings.toml"))
});
pub static MISE_GLOBAL_CONFIG_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_GLOBAL_CONFIG_FILE")
        .or_else(|| var_path("MISE_CONFIG_FILE"))
        .unwrap_or_else(|| MISE_CONFIG_DIR.join("config.toml"))
});
pub static MISE_USE_TOML: Lazy<bool> = Lazy::new(|| !var_is_false("MISE_USE_TOML"));
pub static MISE_LIST_ALL_VERSIONS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LIST_ALL_VERSIONS"));
pub static ARGV0: Lazy<String> = Lazy::new(|| ARGS.read().unwrap()[0].to_string());
pub static MISE_BIN_NAME: Lazy<&str> = Lazy::new(|| filename(&ARGV0));
pub static MISE_LOG_FILE: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("MISE_LOG_FILE"));
pub static MISE_LOG_FILE_LEVEL: Lazy<Option<LevelFilter>> = Lazy::new(log_file_level);

pub static __USAGE: Lazy<Option<String>> = Lazy::new(|| var("__USAGE").ok());

// true if running inside a shim
pub static __MISE_SHIM: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SHIM"));

#[cfg(test)]
pub static TERM_WIDTH: Lazy<usize> = Lazy::new(|| 80);

#[cfg(not(test))]
pub static TERM_WIDTH: Lazy<usize> = Lazy::new(|| {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
        .max(80)
});

/// true if inside a script like bin/exec-env or bin/install
/// used to prevent infinite loops
pub static MISE_BIN: Lazy<PathBuf> = Lazy::new(|| {
    var_path("__MISE_BIN")
        .or_else(|| current_exe().ok())
        .unwrap_or_else(|| "mise".into())
});
#[cfg(feature = "timings")]
pub static MISE_TIMINGS: Lazy<Option<String>> = Lazy::new(|| var("MISE_TIMINGS").ok());
pub static MISE_PID: Lazy<String> = Lazy::new(|| process::id().to_string());
pub static __MISE_SCRIPT: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SCRIPT"));
pub static __MISE_DIFF: Lazy<EnvDiff> = Lazy::new(get_env_diff);
pub static __MISE_ORIG_PATH: Lazy<Option<String>> = Lazy::new(|| var("__MISE_ORIG_PATH").ok());
pub static __MISE_WATCH: Lazy<Option<HookEnvWatches>> = Lazy::new(|| match var("__MISE_WATCH") {
    Ok(raw) => deserialize_watches(raw)
        .map_err(|e| warn!("Failed to deserialize __MISE_WATCH {e}"))
        .ok(),
    _ => None,
});
pub static LINUX_DISTRO: Lazy<Option<String>> = Lazy::new(linux_distro);
pub static PREFER_STALE: Lazy<bool> = Lazy::new(|| prefer_stale(&ARGS.read().unwrap()));
/// essentially, this is whether we show spinners or build output on runtime install
pub static PRISTINE_ENV: Lazy<HashMap<String, String>> =
    Lazy::new(|| get_pristine_env(&__MISE_DIFF, vars().collect()));
pub static PATH_KEY: Lazy<String> = Lazy::new(|| {
    vars()
        .map(|(k, _)| k)
        .find_or_first(|k| k.to_uppercase() == "PATH")
        .map(|k| k.to_string())
        .unwrap_or("PATH".into())
});
pub static PATH: Lazy<Vec<PathBuf>> = Lazy::new(|| match PRISTINE_ENV.get(&*PATH_KEY) {
    Some(path) => split_paths(path).collect(),
    None => vec![],
});
pub static PATH_NON_PRISTINE: Lazy<Vec<PathBuf>> = Lazy::new(|| match var(&*PATH_KEY) {
    Ok(ref path) => split_paths(path).collect(),
    Err(_) => vec![],
});
pub static DIRENV_DIFF: Lazy<Option<String>> = Lazy::new(|| var("DIRENV_DIFF").ok());
pub static GITHUB_TOKEN: Lazy<Option<String>> = Lazy::new(|| {
    var("MISE_GITHUB_TOKEN")
        .or_else(|_| var("GITHUB_API_TOKEN"))
        .or_else(|_| var("GITHUB_TOKEN"))
        .ok()
        .and_then(|v| if v.is_empty() { None } else { Some(v) })
});

pub static CLICOLOR: Lazy<Option<bool>> = Lazy::new(|| {
    if var("CLICOLOR_FORCE").is_ok_and(|v| v != "0") {
        Some(true)
    } else if let Ok(v) = var("CLICOLOR") {
        Some(v != "0")
    } else {
        None
    }
});

// python
pub static PYENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("PYENV_ROOT").unwrap_or_else(|| HOME.join(".pyenv")));

// node
pub static MISE_NODE_CONCURRENCY: Lazy<Option<usize>> = Lazy::new(|| {
    var("MISE_NODE_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|v| v.max(1))
        .or_else(|| {
            if *MISE_NODE_NINJA {
                None
            } else {
                Some(num_cpus::get_physical())
            }
        })
});
pub static MISE_NODE_MAKE: Lazy<String> =
    Lazy::new(|| var("MISE_NODE_MAKE").unwrap_or_else(|_| "make".into()));
pub static MISE_NODE_NINJA: Lazy<bool> =
    Lazy::new(|| var_option_bool("MISE_NODE_NINJA").unwrap_or_else(is_ninja_on_path));
pub static MISE_NODE_VERIFY: Lazy<bool> = Lazy::new(|| !var_is_false("MISE_NODE_VERIFY"));
pub static MISE_NODE_CFLAGS: Lazy<Option<String>> =
    Lazy::new(|| var("MISE_NODE_CFLAGS").or_else(|_| var("NODE_CFLAGS")).ok());
pub static MISE_NODE_CONFIGURE_OPTS: Lazy<Option<String>> = Lazy::new(|| {
    var("MISE_NODE_CONFIGURE_OPTS")
        .or_else(|_| var("NODE_CONFIGURE_OPTS"))
        .ok()
});
pub static MISE_NODE_MAKE_OPTS: Lazy<Option<String>> = Lazy::new(|| {
    var("MISE_NODE_MAKE_OPTS")
        .or_else(|_| var("NODE_MAKE_OPTS"))
        .ok()
});
pub static MISE_NODE_MAKE_INSTALL_OPTS: Lazy<Option<String>> = Lazy::new(|| {
    var("MISE_NODE_MAKE_INSTALL_OPTS")
        .or_else(|_| var("NODE_MAKE_INSTALL_OPTS"))
        .ok()
});
pub static MISE_NODE_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_NODE_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| {
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
pub static MISE_NODE_COREPACK: Lazy<bool> = Lazy::new(|| var_is_true("MISE_NODE_COREPACK"));
pub static NVM_DIR: Lazy<PathBuf> =
    Lazy::new(|| var_path("NVM_DIR").unwrap_or_else(|| HOME.join(".nvm")));
pub static NODENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("NODENV_ROOT").unwrap_or_else(|| HOME.join(".nodenv")));

fn get_env_diff() -> EnvDiff {
    let env = vars().collect::<HashMap<_, _>>();
    match env.get("__MISE_DIFF") {
        Some(raw) => EnvDiff::deserialize(raw).unwrap_or_else(|err| {
            warn!("Failed to deserialize __MISE_DIFF: {:#}", err);
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

pub fn var_path(key: &str) -> Option<PathBuf> {
    var_os(key).map(PathBuf::from).map(replace_path)
}

/// this returns the environment as if __MISE_DIFF was reversed.
/// putting the shell back into a state before hook-env was run
fn get_pristine_env(
    mise_diff: &EnvDiff,
    orig_env: HashMap<String, String>,
) -> HashMap<String, String> {
    let patches = mise_diff.reverse().to_patches();
    let mut env = apply_patches(&orig_env, &patches);

    // get the current path as a vector
    let path = match env.get(&*PATH_KEY) {
        Some(path) => split_paths(path).collect(),
        None => vec![],
    };
    // get the paths that were removed by mise as a hashset
    let mut to_remove = mise_diff.path.iter().collect::<HashSet<_>>();

    // remove those paths that were added by mise, but only once (the first time)
    let path = path
        .into_iter()
        .filter(|p| !to_remove.remove(p))
        .collect_vec();

    // put the pristine PATH back into the environment
    env.insert(
        PATH_KEY.to_string(),
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
    [
        "env", "hook-env", "x", "exec", "direnv", "activate", "current", "ls", "where",
    ]
    .contains(&c.as_str())
}

fn environment(args: &[String]) -> Option<String> {
    let long_arg = format!("--{}", PROFILE_ARG.get_long().unwrap_or_default());
    let short_arg = format!("-{}", PROFILE_ARG.get_short().unwrap_or_default());

    args.windows(2)
        .find_map(|window| {
            if window[0] == long_arg || window[0] == short_arg {
                Some(window[1].clone())
            } else {
                None
            }
        })
        .or_else(|| var("MISE_PROFILE").ok())
        // TODO: it may make sense to deprecate these in the future if we want to reuse MISE_ENV for something else
        .or_else(|| var("MISE_ENV").ok())
        .or_else(|| var("MISE_ENVIRONMENT").ok())
}

fn log_file_level() -> Option<LevelFilter> {
    let log_level = var("MISE_LOG_FILE_LEVEL").unwrap_or_default();
    log_level.parse::<LevelFilter>().ok()
}

fn linux_distro() -> Option<String> {
    match sys_info::linux_os_release() {
        Ok(release) => release.id,
        _ => None,
    }
}

fn filename(path: &str) -> &str {
    path.rsplit_once(path::MAIN_SEPARATOR_STR)
        .map(|(_, file)| file)
        .unwrap_or(path)
}

fn is_ninja_on_path() -> bool {
    which::which("ninja").is_ok()
}

pub fn is_activated() -> bool {
    var("__MISE_DIFF").is_ok()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_apply_patches() {
        reset();
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
        reset();
        set_var("MISE_TEST_PATH", "/foo/bar");
        assert_eq!(
            var_path("MISE_TEST_PATH").unwrap(),
            PathBuf::from("/foo/bar")
        );
        remove_var("MISE_TEST_PATH");
    }
}
