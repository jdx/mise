use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock as Lazy, Mutex};

use path_absolutize::Absolutize;
use xx::regex;

use crate::config::is_global_config;
use crate::env;

static CONFIG_ROOT_CACHE: Lazy<Mutex<HashMap<PathBuf, PathBuf>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub(crate) fn reset() {
    CONFIG_ROOT_CACHE.lock().unwrap().clear();
}

/// The config file a template is being rendered for, made absolute but not
/// resolved. Absolute in every case a working directory can be established —
/// see the fallback chain below for the one that cannot.
///
/// Deliberately not desymlinked. Which of the two a caller wants depends on why
/// the file is a symlink: a shared config linked into `conf.d` wants where the
/// real file lives, and a config that merely sits behind a symlinked home wants
/// the path it was reached by. Templates choose with the filters mise already
/// has — `{{ config_source | canonicalize | dirname }}` for the first,
/// `{{ config_source | dirname }}` for the second — rather than mise choosing
/// for them here.
///
/// Returns a `String` rather than a `PathBuf` so a path that is not valid UTF-8
/// still renders: serde's `PathBuf` serializer fails such a path outright, which
/// would turn one odd byte in a directory name into a template error. Lossy is
/// the better failure here — the value is being handed to a text template.
pub(crate) fn config_source(path: &Path) -> String {
    // `absolutize` asks the OS for the working directory, so it fails when that
    // directory has gone away underneath the process. `dirs::CWD` was captured
    // at startup and still holds a usable base in that case; joining a relative
    // path onto it is absolute, and joining an absolute one returns it
    // unchanged, so the same step is right either way.
    //
    // If both are unavailable the path is returned as given. That is the only
    // case where the value can be relative, and it means the working directory
    // was already gone before mise started — returning something a template can
    // render still beats failing the render, which is why this degrades rather
    // than propagating an error.
    path.absolutize()
        .map(|p| p.to_path_buf())
        .ok()
        .or_else(|| crate::dirs::CWD.as_ref().map(|cwd| cwd.join(path)))
        .unwrap_or_else(|| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

pub(crate) fn config_root(path: &Path) -> PathBuf {
    let path = path
        .absolutize()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf());
    if let Some(cached) = CONFIG_ROOT_CACHE.lock().unwrap().get(&path).cloned() {
        return cached;
    }
    if is_global_config(&path) {
        let root = env::MISE_GLOBAL_CONFIG_ROOT.to_path_buf();
        CONFIG_ROOT_CACHE.lock().unwrap().insert(path, root.clone());
        return root;
    }
    let parts = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    let filename = parts.last().map(|p| p.as_str()).unwrap_or_default();
    let parent = parts
        .iter()
        .nth_back(1)
        .map(|p| p.as_str())
        .unwrap_or_default();
    let grandparent = parts
        .iter()
        .nth_back(2)
        .map(|p| p.as_str())
        .unwrap_or_default();
    let great_grandparent = parts
        .iter()
        .nth_back(3)
        .map(|p| p.as_str())
        .unwrap_or_default();
    let parent_path = || path.parent().unwrap().to_path_buf();
    let grandparent_path = || parent_path().parent().unwrap().to_path_buf();
    let great_grandparent_path = || grandparent_path().parent().unwrap().to_path_buf();
    let great_great_grandparent_path = || great_grandparent_path().parent().unwrap().to_path_buf();
    let is_mise_dir = |d: &str| d == "mise" || d == ".mise";
    let is_config_filename = |f: &str| {
        f == "config.toml" || f == "config.local.toml" || regex!(r"config\..+\.toml").is_match(f)
    };
    let out = if parent == "mise-tasks" || parent == ".mise-tasks" {
        grandparent_path()
    } else if (parent == "tasks" || parent == "conf.d") && is_mise_dir(grandparent) {
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

    /// A template variable that is sometimes relative is a trap, so the path is
    /// absolutized even though it is not resolved.
    #[test]
    fn config_source_is_absolute() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            config_source(Path::new("mise.toml")),
            cwd.join("mise.toml").to_string_lossy()
        );
    }

    /// Not desymlinked, on purpose: `{{ config_source | canonicalize }}` asks
    /// for where the real file lives and plain `config_source` asks for the path
    /// it was reached by. Resolving here would take the second away.
    #[test]
    fn config_source_keeps_the_path_it_was_reached_by() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("shared").join("mise.team.toml");
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, "").unwrap();
        let conf_d = temp.path().join("conf.d");
        std::fs::create_dir_all(&conf_d).unwrap();
        let link = conf_d.join("mise.team.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert_eq!(config_source(&link), link.to_string_lossy());
        assert_ne!(config_source(&link), real.to_string_lossy());
        // ...and the resolution the reporter wants is still one filter away.
        assert_eq!(
            std::fs::canonicalize(&link).unwrap(),
            std::fs::canonicalize(&real).unwrap()
        );
    }

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
            "/foo/bar/.mise/conf.d/foo.toml",
            "/foo/bar/.mise/config.local.toml",
            "/foo/bar/.mise/config.toml",
            "/foo/bar/.mise/tasks/build.toml",
            "/foo/bar/.tool-versions",
            "/foo/bar/mise.env.toml",
            "/foo/bar/mise.local.toml",
            "/foo/bar/mise.toml",
            "/foo/bar/mise/config.local.toml",
            "/foo/bar/mise/config.toml",
            "/foo/bar/mise/conf.d/config.toml",
            "/foo/bar/mise/conf.d/foo.toml",
            "/foo/bar/mise/tasks/build.toml",
            "/foo/bar/.config/mise/config.env.toml",
            "/foo/bar/.config/mise.env.toml",
            "/foo/bar/.config/mise/tasks/build.toml",
            "/foo/bar/.mise/config.env.toml",
            "/foo/bar/.mise.env.toml",
            "/foo/bar/.mise-tasks/build.toml",
            "/foo/bar/mise-tasks/build.toml",
        ] {
            println!("{p}");
            assert_eq!(config_root(Path::new(p)), PathBuf::from("/foo/bar"));
        }
    }

    #[test]
    fn test_config_root_mise_dir() {
        for p in &[
            "/foo/mise/.config/mise/conf.d/config.toml",
            "/foo/mise/.config/mise/conf.d/foo.toml",
            "/foo/mise/.config/mise/config.local.toml",
            "/foo/mise/.config/mise/config.toml",
            "/foo/mise/.config/mise.local.toml",
            "/foo/mise/.config/mise.toml",
            "/foo/mise/.mise.env.toml",
            "/foo/mise/.mise.local.toml",
            "/foo/mise/.mise.toml",
            "/foo/mise/.mise/conf.d/config.toml",
            "/foo/mise/.mise/conf.d/foo.toml",
            "/foo/mise/.mise/config.local.toml",
            "/foo/mise/.mise/config.toml",
            "/foo/mise/.mise/tasks/build.toml",
            "/foo/mise/.tool-versions",
            "/foo/mise/mise.env.toml",
            "/foo/mise/mise.local.toml",
            "/foo/mise/mise.toml",
            "/foo/mise/mise/config.local.toml",
            "/foo/mise/mise/config.toml",
            "/foo/mise/mise/conf.d/config.toml",
            "/foo/mise/mise/conf.d/foo.toml",
            "/foo/mise/mise/tasks/build.toml",
            "/foo/mise/.config/mise/config.env.toml",
            "/foo/mise/.config/mise.env.toml",
            "/foo/mise/.config/mise/tasks/build.toml",
            "/foo/mise/.mise/config.env.toml",
            "/foo/mise/.mise.env.toml",
            "/foo/mise/.mise-tasks/build.toml",
            "/foo/mise/mise-tasks/build.toml",
        ] {
            println!("{p}");
            assert_eq!(config_root(Path::new(p)), PathBuf::from("/foo/mise"));
        }
    }
}
