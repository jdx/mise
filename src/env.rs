use std::collections::{HashMap, HashSet};
pub use std::env::*;
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

use itertools::Itertools;
use log::LevelFilter;
use once_cell::sync::Lazy;
use url::Url;

use crate::duration::HOURLY;
use crate::env_diff::{EnvDiff, EnvDiffOperation, EnvDiffPatches};
use crate::file::replace_path;
use crate::hook_env::{deserialize_watches, HookEnvWatches};

pub static ARGS: RwLock<Vec<String>> = RwLock::new(vec![]);
pub static SHELL: Lazy<String> = Lazy::new(|| var("SHELL").unwrap_or_else(|_| "sh".into()));

// paths and directories
pub static HOME: Lazy<PathBuf> =
    Lazy::new(|| home::home_dir().unwrap_or_else(|| PathBuf::from("/")));
pub static EDITOR: Lazy<String> =
    Lazy::new(|| var("VISUAL").unwrap_or_else(|_| var("EDITOR").unwrap_or_else(|_| "nano".into())));

#[cfg(target_os = "macos")]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join("Library/Caches")));
#[cfg(not(target_os = "macos"))]
pub static XDG_CACHE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CACHE_HOME").unwrap_or_else(|| HOME.join(".cache")));
pub static XDG_CONFIG_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_CONFIG_HOME").unwrap_or_else(|| HOME.join(".config")));
pub static XDG_DATA_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_DATA_HOME").unwrap_or_else(|| HOME.join(".local/share")));
pub static XDG_STATE_HOME: Lazy<PathBuf> =
    Lazy::new(|| var_path("XDG_STATE_HOME").unwrap_or_else(|| HOME.join(".local/state")));

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

pub static MISE_DEFAULT_TOOL_VERSIONS_FILENAME: Lazy<String> = Lazy::new(|| {
    var("MISE_DEFAULT_TOOL_VERSIONS_FILENAME").unwrap_or_else(|_| ".tool-versions".into())
});
pub static MISE_DEFAULT_CONFIG_FILENAME: Lazy<String> =
    Lazy::new(|| var("MISE_DEFAULT_CONFIG_FILENAME").unwrap_or_else(|_| ".mise.toml".into()));
pub static MISE_ENV: Lazy<Option<String>> =
    Lazy::new(|| var("MISE_ENV").or_else(|_| var("MISE_ENVIRONMENT")).ok());
pub static MISE_SETTINGS_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_SETTINGS_FILE").unwrap_or_else(|| MISE_CONFIG_DIR.join("settings.toml"))
});
pub static MISE_GLOBAL_CONFIG_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_GLOBAL_CONFIG_FILE")
        .or_else(|| var_path("MISE_CONFIG_FILE"))
        .unwrap_or_else(|| MISE_CONFIG_DIR.join("config.toml"))
});
pub static MISE_USE_TOML: Lazy<bool> = Lazy::new(|| var_is_true("MISE_USE_TOML"));
pub static MISE_BIN: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_BIN")
        .or_else(|| current_exe().ok())
        .unwrap_or_else(|| "mise".into())
});
pub static ARGV0: Lazy<String> = Lazy::new(|| ARGS.read().unwrap()[0].to_string());
pub static MISE_BIN_NAME: Lazy<&str> = Lazy::new(|| filename(&ARGV0));
pub static MISE_LOG_FILE: Lazy<Option<PathBuf>> = Lazy::new(|| var_path("MISE_LOG_FILE"));
pub static MISE_LOG_FILE_LEVEL: Lazy<Option<LevelFilter>> = Lazy::new(log_file_level);
pub static MISE_FETCH_REMOTE_VERSIONS_TIMEOUT: Lazy<Duration> = Lazy::new(|| {
    var_duration("MISE_FETCH_REMOTE_VERSIONS_TIMEOUT").unwrap_or(Duration::from_secs(10))
});

#[cfg(test)]
pub static TERM_WIDTH: Lazy<usize> = Lazy::new(|| 80);

#[cfg(not(test))]
pub static TERM_WIDTH: Lazy<usize> = Lazy::new(|| {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
        .max(80)
});

/// duration that remote version cache is kept for
/// for "fast" commands (represented by PREFER_STALE), these are always
/// cached. For "slow" commands like `mise ls-remote` or `mise install`:
/// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is set, use that
/// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is not set, use HOURLY
pub static MISE_FETCH_REMOTE_VERSIONS_CACHE: Lazy<Option<Duration>> = Lazy::new(|| {
    if *PREFER_STALE {
        None
    } else {
        Some(var_duration("MISE_FETCH_REMOTE_VERSIONS_CACHE").unwrap_or(HOURLY))
    }
});

/// true if inside a script like bin/exec-env or bin/install
/// used to prevent infinite loops
pub static __MISE_SCRIPT: Lazy<bool> = Lazy::new(|| var_is_true("__MISE_SCRIPT"));
pub static __MISE_DIFF: Lazy<EnvDiff> = Lazy::new(get_env_diff);
pub static __MISE_ORIG_PATH: Lazy<Option<String>> = Lazy::new(|| var("__MISE_ORIG_PATH").ok());
pub static __MISE_WATCH: Lazy<Option<HookEnvWatches>> = Lazy::new(|| match var("__MISE_WATCH") {
    Ok(raw) => deserialize_watches(raw)
        .map_err(|e| warn!("Failed to deserialize __MISE_WATCH {e}"))
        .ok(),
    _ => None,
});
pub static CI: Lazy<bool> = Lazy::new(|| var_is_true("CI"));
pub static LINUX_DISTRO: Lazy<Option<String>> = Lazy::new(linux_distro);
pub static PREFER_STALE: Lazy<bool> = Lazy::new(|| prefer_stale(&ARGS.read().unwrap()));
/// essentially, this is whether we show spinners or build output on runtime install
pub static PRISTINE_ENV: Lazy<HashMap<String, String>> =
    Lazy::new(|| get_pristine_env(&__MISE_DIFF, vars().collect()));
pub static PATH: Lazy<Vec<PathBuf>> = Lazy::new(|| match PRISTINE_ENV.get("PATH") {
    Some(path) => split_paths(path).collect(),
    None => vec![],
});
pub static DIRENV_DIFF: Lazy<Option<String>> = Lazy::new(|| var("DIRENV_DIFF").ok());
#[allow(unused)]
pub static GITHUB_API_TOKEN: Lazy<Option<String>> = Lazy::new(|| var("GITHUB_API_TOKEN").ok());

pub static MISE_USE_VERSIONS_HOST: Lazy<bool> =
    Lazy::new(|| !var_is_false("MISE_USE_VERSIONS_HOST"));

// python
pub static PYENV_ROOT: Lazy<PathBuf> =
    Lazy::new(|| var_path("PYENV_ROOT").unwrap_or_else(|| HOME.join(".pyenv")));

// node
pub static MISE_NODE_MIRROR_URL: Lazy<Url> = Lazy::new(|| {
    var_url("MISE_NODE_MIRROR_URL")
        .or_else(|| var_url("NODE_BUILD_MIRROR_URL"))
        .unwrap_or_else(|| Url::parse("https://nodejs.org/dist/").unwrap())
});
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

// ruby
pub static MISE_RUBY_BUILD_REPO: Lazy<String> = Lazy::new(|| {
    var("MISE_RUBY_BUILD_REPO").unwrap_or_else(|_| "https://github.com/rbenv/ruby-build.git".into())
});
pub static MISE_RUBY_INSTALL_REPO: Lazy<String> = Lazy::new(|| {
    var("MISE_RUBY_INSTALL_REPO")
        .unwrap_or_else(|_| "https://github.com/postmodern/ruby-install.git".into())
});
pub static MISE_RUBY_INSTALL: Lazy<bool> = Lazy::new(|| var_is_true("MISE_RUBY_INSTALL"));
pub static MISE_RUBY_APPLY_PATCHES: Lazy<Option<String>> =
    Lazy::new(|| var("MISE_RUBY_APPLY_PATCHES").ok());
pub static MISE_RUBY_VERBOSE_INSTALL: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("MISE_RUBY_VERBOSE_INSTALL"));
pub static MISE_RUBY_INSTALL_OPTS: Lazy<Result<Vec<String>, shell_words::ParseError>> =
    Lazy::new(|| shell_words::split(&var("MISE_RUBY_INSTALL_OPTS").unwrap_or_default()));
pub static MISE_RUBY_BUILD_OPTS: Lazy<Result<Vec<String>, shell_words::ParseError>> =
    Lazy::new(|| shell_words::split(&var("MISE_RUBY_BUILD_OPTS").unwrap_or_default()));
pub static MISE_RUBY_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_RUBY_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| HOME.join(".default-gems"))
});

// go
pub static MISE_GO_DEFAULT_PACKAGES_FILE: Lazy<PathBuf> = Lazy::new(|| {
    var_path("MISE_GO_DEFAULT_PACKAGES_FILE").unwrap_or_else(|| HOME.join(".default-go-packages"))
});
pub static MISE_GO_SKIP_CHECKSUM: Lazy<bool> = Lazy::new(|| var_is_true("MISE_GO_SKIP_CHECKSUM"));
pub static MISE_GO_REPO: Lazy<String> =
    Lazy::new(|| var("MISE_GO_REPO").unwrap_or_else(|_| "https://github.com/golang/go".into()));
pub static MISE_GO_DOWNLOAD_MIRROR: Lazy<String> = Lazy::new(|| {
    var("MISE_GO_DOWNLOAD_MIRROR").unwrap_or_else(|_| "https://dl.google.com/go".into())
});
pub static MISE_GO_SET_GOROOT: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("MISE_GO_SET_GOROOT"));
pub static MISE_GO_SET_GOPATH: Lazy<Option<bool>> =
    Lazy::new(|| var_option_bool("MISE_GO_SET_GOPATH"));

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

fn var_url(key: &str) -> Option<Url> {
    var(key).ok().map(|v| Url::parse(&v).unwrap())
}

fn var_duration(key: &str) -> Option<Duration> {
    var(key)
        .ok()
        .map(|v| v.parse::<humantime::Duration>().unwrap().into())
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
    let path = match env.get("PATH") {
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
    path.rsplit_once('/').map(|(_, file)| file).unwrap_or(path)
}

fn is_ninja_on_path() -> bool {
    which::which("ninja").is_ok()
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
        set_var("MISE_TEST_PATH", "/foo/bar");
        assert_eq!(
            var_path("MISE_TEST_PATH").unwrap(),
            PathBuf::from("/foo/bar")
        );
        remove_var("MISE_TEST_PATH");
    }
}
