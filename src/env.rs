use crate::Result;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches, EnvMap};
use crate::file::replace_path;
use crate::shell::ShellType;
use crate::{cli::args::ToolArg, file::display_path};
use eyre::Context;
use indexmap::IndexSet;
use itertools::Itertools;
use log::LevelFilter;
pub use std::env::*;
use std::sync::LazyLock as Lazy;
use std::sync::RwLock;
use std::{
    collections::{HashMap, HashSet},
    ffi::OsStr,
    sync::Mutex,
};
use std::{path, process};
use std::{path::Path, string::ToString};
use std::{path::PathBuf, sync::atomic::AtomicBool};

pub static ARGS: RwLock<Vec<String>> = RwLock::new(vec![]);
pub static TOOL_ARGS: RwLock<Vec<ToolArg>> = RwLock::new(vec![]);
#[cfg(unix)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));
#[cfg(windows)]
pub static SHELL: Lazy<String> = Lazy::new(|| var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
pub static MISE_SHELL: Lazy<Option<ShellType>> = Lazy::new(|| {
    var("MISE_SHELL")
        .unwrap_or_else(|_| SHELL.clone())
        .parse()
        .ok()
});

// paths and directories
#[cfg(test)]
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"));
#[cfg(not(test))]
pub static HOME: Lazy<PathBuf> = Lazy::new(|| {
    homedir::my_home()
        .ok()
        .flatten()
        .unwrap_or_else(|| PathBuf::from("/"))
});

pub static EDITOR: Lazy<String> =
    Lazy::new(|| var("VISUAL").unwrap_or_else(|_| var("EDITOR").unwrap_or_else(|_| "nano".into())));

#[cfg(macos)]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join("Library/Caches")));
#[cfg(windows)]
pub static XDG_CACHE_HOME: Lazy<PathBuf> = Lazy::new(|| {
    var_path("XDG_CACHE_HOME")
        .or_else(|| var_path("TEMP"))
        .unwrap_or_else(temp_dir)
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

/// always display "friendly" errors even in debug mode
pub static MISE_FRIENDLY_ERROR: Lazy<bool> = Lazy::new(|| var_is_true("MISE_FRIENDLY_ERROR"));
pub static MISE_NO_CONFIG: Lazy<bool> = Lazy::new(|| var_is_true("MISE_NO_CONFIG"));
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
    var("MISE_DEFAULT_TOOL_VERSIONS_FILENAME")
        .ok()
        .or(MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES
            .as_ref()
            .and_then(|v| v.first().cloned()))
        .or(var("MISE_DEFAULT_TOOL_VERSIONS_FILENAME").ok())
        .unwrap_or_else(|| ".tool-versions".into())
});
pub static MISE_DEFAULT_CONFIG_FILENAME: Lazy<String> = Lazy::new(|| {
    var("MISE_DEFAULT_CONFIG_FILENAME")
        .ok()
        .or(MISE_OVERRIDE_CONFIG_FILENAMES.first().cloned())
        .unwrap_or_else(|| "mise.toml".into())
});
pub static MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES: Lazy<Option<IndexSet<String>>> =
    Lazy::new(|| match var("MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES") {
        Ok(v) if v == "none" => Some([].into()),
        Ok(v) => Some(v.split(':').map(|s| s.to_string()).collect()),
        Err(_) => Default::default(),
    });
pub static MISE_OVERRIDE_CONFIG_FILENAMES: Lazy<IndexSet<String>> =
    Lazy::new(|| match var("MISE_OVERRIDE_CONFIG_FILENAMES") {
        Ok(v) => v.split(':').map(|s| s.to_string()).collect(),
        Err(_) => Default::default(),
    });
pub static MISE_ENV: Lazy<Vec<String>> = Lazy::new(|| environment(&ARGS.read().unwrap()));
pub static MISE_GLOBAL_CONFIG_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("MISE_GLOBAL_CONFIG_FILE").or_else(|| var_path("MISE_CONFIG_FILE")));
pub static MISE_GLOBAL_CONFIG_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("MISE_GLOBAL_CONFIG_ROOT").unwrap_or_else(|| HOME.to_path_buf()));
pub static MISE_SYSTEM_CONFIG_FILE: Lazy<Option<PathBuf>> =
    Lazy::new(|| var_path("MISE_SYSTEM_CONFIG_FILE"));
pub static MISE_IGNORED_CONFIG_PATHS: Lazy<Vec<PathBuf>> = Lazy::new(|| {
    var("MISE_IGNORED_CONFIG_PATHS")
        .ok()
        .map(|v| {
            v.split(':')
                .filter(|p| !p.is_empty())
                .map(PathBuf::from)
                .map(replace_path)
                .collect()
        })
        .unwrap_or_default()
});
pub static MISE_TASK_LEVEL: Lazy<u8> = Lazy::new(|| var_u8("MISE_TASK_LEVEL"));
pub static MISE_USE_TOML: Lazy<bool> = Lazy::new(|| !var_is_false("MISE_USE_TOML"));
pub static MISE_LIST_ALL_VERSIONS: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LIST_ALL_VERSIONS"));
pub static ARGV0: Lazy<String> = Lazy::new(|| ARGS.read().unwrap()[0].to_string());
pub static MISE_BIN_NAME: Lazy<&str> = Lazy::new(|| filename(&ARGV0));
pub static MISE_LOG_FILE: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("MISE_LOG_FILE"));
pub static MISE_LOG_FILE_LEVEL: Lazy<Option<LevelFilter>> = Lazy::new(log_file_level);
pub static MISE_LOG_HTTP: Lazy<bool> = Lazy::new(|| var_is_true("MISE_LOG_HTTP"));

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
pub static MISE_TIMINGS: Lazy<u8> = Lazy::new(|| var_u8("MISE_TIMINGS"));
pub static MISE_PID: Lazy<String> = Lazy::new(|| process::id().to_string());
pub static __MISE_SCRIPT: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SCRIPT"));
pub static __MISE_DIFF: Lazy<EnvDiff> = Lazy::new(get_env_diff);
pub static __MISE_ORIG_PATH: Lazy<Option<String>> = Lazy::new(|| var("__MISE_ORIG_PATH").ok());
pub static LINUX_DISTRO: Lazy<Option<String>> = Lazy::new(linux_distro);
pub static PREFER_OFFLINE: Lazy<AtomicBool> =
    Lazy::new(|| prefer_offline(&ARGS.read().unwrap()).into());
pub static OFFLINE: Lazy<bool> = Lazy::new(|| offline(&ARGS.read().unwrap()));
/// essentially, this is whether we show spinners or build output on runtime install
pub static PRISTINE_ENV: Lazy<EnvMap> =
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
    let token = var("MISE_GITHUB_TOKEN")
        .or_else(|_| var("GITHUB_API_TOKEN"))
        .or_else(|_| var("GITHUB_TOKEN"))
        .ok()
        .and_then(|v| if v.is_empty() { None } else { Some(v) });

    // set or unset the token for plugins+ubi
    if let Some(token) = token.as_ref() {
        set_var("MISE_GITHUB_TOKEN", token);
        set_var("GITHUB_TOKEN", token);
        set_var("GITHUB_API_TOKEN", token);
    } else {
        remove_var("MISE_GITHUB_TOKEN");
        remove_var("GITHUB_TOKEN");
        remove_var("GITHUB_API_TOKEN");
    }

    token
});
pub static MISE_GITHUB_ENTERPRISE_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| match var("MISE_GITHUB_ENTERPRISE_TOKEN") {
        Ok(v) if v.trim() != "" => {
            set_var("MISE_GITHUB_ENTERPRISE_TOKEN", &v);
            Some(v)
        }
        _ => {
            remove_var("MISE_GITHUB_ENTERPRISE_TOKEN");
            None
        }
    });
pub static GITLAB_TOKEN: Lazy<Option<String>> =
    Lazy::new(
        || match var("MISE_GITLAB_TOKEN").or_else(|_| var("GITLAB_TOKEN")) {
            Ok(v) if v.trim() != "" => {
                set_var("MISE_GITLAB_TOKEN", &v);
                set_var("GITLAB_TOKEN", &v);
                Some(v)
            }
            _ => {
                remove_var("MISE_GITLAB_TOKEN");
                remove_var("GITLAB_TOKEN");
                None
            }
        },
    );
pub static MISE_GITLAB_ENTERPRISE_TOKEN: Lazy<Option<String>> =
    Lazy::new(|| match var("MISE_GITLAB_ENTERPRISE_TOKEN") {
        Ok(v) if v.trim() != "" => {
            set_var("MISE_GITLAB_ENTERPRISE_TOKEN", &v);
            Some(v)
        }
        _ => {
            remove_var("MISE_GITLAB_ENTERPRISE_TOKEN");
            None
        }
    });

pub static TEST_TRANCHE: Lazy<usize> = Lazy::new(|| var_u8("TEST_TRANCHE") as usize);
pub static TEST_TRANCHE_COUNT: Lazy<usize> = Lazy::new(|| var_u8("TEST_TRANCHE_COUNT") as usize);

pub static CLICOLOR_FORCE: Lazy<Option<bool>> =
    Lazy::new(|| var("CLICOLOR_FORCE").ok().map(|v| v != "0"));

pub static CLICOLOR: Lazy<Option<bool>> = Lazy::new(|| {
    if *CLICOLOR_FORCE == Some(true) {
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
pub static UV_PYTHON_INSTALL_DIR: Lazy<PathBuf> = Lazy::new(|| {
    var_path("UV_PYTHON_INSTALL_DIR").unwrap_or_else(|| XDG_DATA_HOME.join("uv").join("python"))
});

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
pub static MISE_JOBS: Lazy<Option<usize>> =
    Lazy::new(|| var("MISE_JOBS").ok().and_then(|v| v.parse::<usize>().ok()));
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

#[cfg(unix)]
pub const PATH_ENV_SEP: char = ':';
#[cfg(windows)]
pub const PATH_ENV_SEP: char = ';';

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

fn var_u8(key: &str) -> u8 {
    var(key)
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or_default()
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

pub fn in_home_dir() -> bool {
    current_dir().is_ok_and(|d| d == *HOME)
}

pub fn var_path(key: &str) -> Option<PathBuf> {
    var_os(key).map(PathBuf::from).map(replace_path)
}

/// this returns the environment as if __MISE_DIFF was reversed.
/// putting the shell back into a state before hook-env was run
fn get_pristine_env(mise_diff: &EnvDiff, orig_env: EnvMap) -> EnvMap {
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

fn apply_patches(env: &EnvMap, patches: &EnvDiffPatches) -> EnvMap {
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

fn offline(args: &[String]) -> bool {
    if var_is_true("MISE_OFFLINE") {
        return true;
    }

    args.iter()
        .take_while(|a| *a != "--")
        .any(|a| a == "--offline")
}

/// returns true if new runtime versions should not be fetched
fn prefer_offline(args: &[String]) -> bool {
    // First check if MISE_PREFER_OFFLINE is set
    if var_is_true("MISE_PREFER_OFFLINE") {
        return true;
    }

    // Otherwise fall back to the original command-based logic
    args.iter()
        .take_while(|a| *a != "--")
        .filter(|a| !a.starts_with('-') || *a == "--prefer-offline")
        .nth(1)
        .map(|a| {
            [
                "--prefer-offline",
                "activate",
                "current",
                "direnv",
                "env",
                "exec",
                "hook-env",
                "ls",
                "where",
                "x",
            ]
            .contains(&a.as_str())
        })
        .unwrap_or_default()
}

fn environment(args: &[String]) -> Vec<String> {
    let arg_defs = HashSet::from(["--profile", "-P", "--env", "-E"]);

    args.windows(2)
        .take_while(|window| !window.iter().any(|a| a == "--"))
        .find_map(|window| {
            if arg_defs.contains(&*window[0]) {
                Some(window[1].clone())
            } else {
                None
            }
        })
        .or_else(|| var("MISE_ENV").ok())
        .or_else(|| var("MISE_PROFILE").ok())
        .or_else(|| var("MISE_ENVIRONMENT").ok())
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
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

pub fn set_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
    static MUTEX: Mutex<()> = Mutex::new(());
    let _mutex = MUTEX.lock().unwrap();
    unsafe {
        std::env::set_var(key, value);
    }
}

pub fn remove_var<K: AsRef<OsStr>>(key: K) {
    static MUTEX: Mutex<()> = Mutex::new(());
    let _mutex = MUTEX.lock().unwrap();
    unsafe {
        std::env::remove_var(key);
    }
}

pub fn set_current_dir<P: AsRef<Path>>(path: P) -> Result<()> {
    let path = path.as_ref();
    trace!("cd {}", display_path(path));
    unsafe {
        std::env::set_current_dir(path).wrap_err_with(|| {
            format!("failed to set current directory to {}", display_path(path))
        })?;
        path_absolutize::update_cwd();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_apply_patches() {
        let _config = Config::get().await.unwrap();
        let mut env = EnvMap::new();
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

    #[tokio::test]
    async fn test_var_path() {
        let _config = Config::get().await.unwrap();
        set_var("MISE_TEST_PATH", "/foo/bar");
        assert_eq!(
            var_path("MISE_TEST_PATH").unwrap(),
            PathBuf::from("/foo/bar")
        );
        remove_var("MISE_TEST_PATH");
    }
}
