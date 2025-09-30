use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock as Lazy, Mutex};

use path_absolutize::Absolutize;
use xx::regex;

use crate::config::is_global_config;
use crate::env;

static CONFIG_ROOT_CACHE: Lazy<Mutex<HashMap<PathBuf, PathBuf>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn reset() {
    CONFIG_ROOT_CACHE.lock().unwrap().clear();
}

pub fn config_root(path: &Path) -> PathBuf {
    if is_global_config(path) {
        return env::MISE_GLOBAL_CONFIG_ROOT.to_path_buf();
    }
    let path = path
        .absolutize()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf());
    if let Some(cached) = CONFIG_ROOT_CACHE.lock().unwrap().get(&path).cloned() {
        return cached;
    }
    let parts = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    const EMPTY: &str = "";
    let filename = parts.last().map(|p| p.as_str()).unwrap_or(EMPTY);
    let parent = parts
        .iter()
        .nth_back(1)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let grandparent = parts
        .iter()
        .nth_back(2)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let great_grandparent = parts
        .iter()
        .nth_back(3)
        .map(|p| p.as_str())
        .unwrap_or(EMPTY);
    let parent_path = || path.parent().unwrap().to_path_buf();
    let grandparent_path = || parent_path().parent().unwrap().to_path_buf();
    let great_grandparent_path = || grandparent_path().parent().unwrap().to_path_buf();
    let great_great_grandparent_path = || great_grandparent_path().parent().unwrap().to_path_buf();
    let is_mise_dir = |d: &str| d == "mise" || d == ".mise";
    let is_config_filename = |f: &str| {
        f == "config.toml" || f == "config.local.toml" || regex!(r"config\..+\.toml").is_match(f)
    };
    let out = if parent == "conf.d" && is_mise_dir(grandparent) {
        if great_grandparent == ".config" {
            great_great_grandparent_path()
        } else {
            great_grandparent_path()
        }
    } else if is_mise_dir(parent) && is_config_filename(filename) {
        if grandparent == ".config" {
            great_grandparent_path()
        } else {
            grandparent_path()
        }
    } else if parent == ".config" {
        grandparent_path()
    } else {
        parent_path()
    };
    CONFIG_ROOT_CACHE.lock().unwrap().insert(path, out.clone());
    out
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[test]
    fn test_config_root() {
        for p in &[
            "/foo/bar/.config/mise/conf.d/config.toml",
            "/foo/bar/.config/mise/conf.d/foo.toml",
            "/foo/bar/.config/mise/config.local.toml",
            "/foo/bar/.config/mise/config.toml",
            "/foo/bar/.config/mise.local.toml",
            "/foo/bar/.config/mise.toml",
            "/foo/bar/.mise.env.toml",
            "/foo/bar/.mise.local.toml",
            "/foo/bar/.mise.toml",
            "/foo/bar/.mise/conf.d/config.toml",
            "/foo/bar/.mise/config.local.toml",
            "/foo/bar/.mise/config.toml",
            "/foo/bar/.tool-versions",
            "/foo/bar/mise.env.toml",
            "/foo/bar/mise.local.toml",
            "/foo/bar/mise.toml",
            "/foo/bar/mise/config.local.toml",
            "/foo/bar/mise/config.toml",
            "/foo/bar/.config/mise/config.env.toml",
            "/foo/bar/.config/mise.env.toml",
            "/foo/bar/.mise/config.env.toml",
            "/foo/bar/.mise.env.toml",
        ] {
            println!("{p}");
            assert_eq!(config_root(Path::new(p)), PathBuf::from("/foo/bar"));
        }
    }
}
