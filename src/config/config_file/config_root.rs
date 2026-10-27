use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock as Lazy, Mutex};

use path_absolutize::Absolutize;
use xx::regex;

use crate::config::{discovered_global_config, is_conf_d_folder_file, is_global_config};
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

pub fn config_root(path: &Path) -> PathBuf {
    let path = path
        .absolutize()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf());
    if let Some(cached) = CONFIG_ROOT_CACHE.lock().unwrap().get(&path).cloned() {
        return cached;
    }
    // Global config is trusted for what `path` resolves to, so where it was
    // discovered decides its root: a project's symlink to it must not make the
    // project's directory the root.
    let global = discovered_global_config(&path);
    // A folder fragment is its own root even in global config, so relative
    // paths and `{{ config_root }}` keep pointing inside the folder.
    let fragment = global.as_ref().unwrap_or(&path);
    if is_conf_d_folder_file(fragment) {
        let root = fragment.parent().unwrap().to_path_buf();
        CONFIG_ROOT_CACHE.lock().unwrap().insert(path, root.clone());
        return root;
    }
    if global.is_some() {
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
    // The filesystem root has no parent, so every step saturates there instead
    // of unwrapping: `config_root` is also handed a root it computed earlier
    // (`/` for `/mise.toml`) and a config nested under a directory named `mise`
    // directly below `/` climbs past it.
    let up = |p: PathBuf| p.parent().map(Path::to_path_buf).unwrap_or(p);
    let parent_path = || up(path.clone());
    let grandparent_path = || up(parent_path());
    let great_grandparent_path = || up(grandparent_path());
    let great_great_grandparent_path = || up(great_grandparent_path());
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

/// The git checkout a config file belongs to when that checkout lies below
/// the directory the file configures.
///
/// `<dir>/mise/config.toml`, `<dir>/mise/conf.d/*.toml` and their `.mise/` and
/// `.config/` forms are config of `<dir>`, so a repository cloned into `<dir>`
/// under one of those names looks like a config folder by name alone. A
/// checkout of its own is a project of its own: it is not `<dir>`'s config, and
/// trust in `<dir>` does not extend to it. A submodule belongs to the checkout
/// that holds it, so a project's `.mise/` submodule stays part of the project.
pub fn nested_checkout(path: &Path) -> Option<PathBuf> {
    if is_global_config(path) {
        return None;
    }
    let path = path.absolutize().ok()?;
    let dir = path.parent()?;
    // A folder fragment is its own config root, but it configures the
    // directory its `conf.d` belongs to.
    let configured = if is_conf_d_folder_file(&path) {
        config_root(dir)
    } else {
        config_root(&path)
    };
    checkout_between(&configured, dir)
}

/// The git checkout holding `dir` when it lies strictly below `configured`,
/// for a directory reached from `configured` by a fixed relative path such as
/// `mise/tasks`. See [`nested_checkout`].
pub(crate) fn checkout_between(configured: &Path, dir: &Path) -> Option<PathBuf> {
    // Only a `.git` between the two can make a checkout of its own, so most
    // configs need no walk at all.
    let first = dir
        .ancestors()
        .take_while(|a| *a != configured && a.starts_with(configured))
        .find(|a| a.join(".git").symlink_metadata().is_ok())?;
    let checkout = crate::git::checkout_of(first);
    checkout
        .worktree
        .or(checkout.repository)
        .filter(|c| c != configured && c.starts_with(configured))
        // A git-managed `~/.config` or mise config directory holds the user's
        // own dotfiles, and no clone can land on one that already exists.
        .filter(|c| *c != crate::dirs::HOME.join(".config") && !crate::dirs::CONFIG.starts_with(c))
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
    fn test_config_root_at_filesystem_root() {
        for p in ["/mise.toml", "/.mise.toml", "/mise/config.toml", "/"] {
            assert_eq!(config_root(Path::new(p)), PathBuf::from("/"), "{p}");
        }
    }

    #[test]
    fn test_config_root_conf_d_folder() {
        for (p, root) in [
            (
                "/foo/bar/.mise/conf.d/git/mise.toml",
                "/foo/bar/.mise/conf.d/git",
            ),
            (
                "/foo/bar/mise/conf.d/git/mise.linux.toml",
                "/foo/bar/mise/conf.d/git",
            ),
            (
                "/foo/bar/.config/mise/conf.d/git/mise.local.toml",
                "/foo/bar/.config/mise/conf.d/git",
            ),
            // only mise.*.toml inside a visible folder is a folder fragment
            (
                "/foo/bar/.mise/conf.d/.git/mise.toml",
                "/foo/bar/.mise/conf.d/.git",
            ),
        ] {
            assert_eq!(config_root(Path::new(p)), PathBuf::from(root), "{p}");
        }
        assert!(crate::config::is_conf_d_folder_file(Path::new(
            "/foo/bar/.mise/conf.d/git/mise.linux.toml"
        )));
        assert!(!crate::config::is_conf_d_folder_file(Path::new(
            "/foo/bar/.mise/conf.d/.git/mise.toml"
        )));
        assert!(!crate::config::is_conf_d_folder_file(Path::new(
            "/foo/bar/.mise/conf.d/git/config.toml"
        )));
        // a project that happens to live under some other conf.d
        assert!(!crate::config::is_conf_d_folder_file(Path::new(
            "/etc/nginx/conf.d/site/mise.toml"
        )));
    }

    /// Global config is trusted through any path that resolves to it, so a
    /// project's link to it must not move its root into the project.
    #[test]
    fn test_config_root_conf_d_folder_link_to_global_config() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("mise").join("conf.d").join("git");
        std::fs::create_dir_all(&folder).unwrap();
        let link = folder.join("mise.toml");
        std::os::unix::fs::symlink(crate::config::global_config_path(), &link).unwrap();

        assert!(is_conf_d_folder_file(&link));
        assert_eq!(config_root(&link), *env::MISE_GLOBAL_CONFIG_ROOT);
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

    /// A repository cloned into `projects` under the name `mise`, `.mise` or
    /// `.config` is a project of its own, not the config folder of `projects`.
    #[test]
    fn test_nested_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let projects = temp.path().join("projects");
        for (name, files, tasks) in [
            (
                "mise",
                &[
                    "config.toml",
                    "config.local.toml",
                    "conf.d/foo.toml",
                    "conf.d/sub/mise.toml",
                ][..],
                "tasks",
            ),
            (".mise", &["config.toml"][..], "tasks"),
            (
                ".config",
                &["mise.toml", "mise/config.toml"][..],
                "mise/tasks",
            ),
        ] {
            let clone = projects.join(name);
            std::fs::create_dir_all(clone.join(".git")).unwrap();
            for p in files {
                let p = clone.join(p);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                assert_eq!(nested_checkout(&p), Some(clone.clone()), "{}", p.display());
            }
            // nor are its tasks the tasks of `projects`
            assert_eq!(
                checkout_between(&projects, &clone.join(tasks)),
                Some(clone.clone())
            );
        }
        // the checkout's own config, and the directory it was cloned into
        assert_eq!(nested_checkout(&projects.join("mise/mise.toml")), None);
        assert_eq!(nested_checkout(&projects.join("mise.toml")), None);

        // a project's own `.mise/` inside its checkout belongs to the project
        let app = temp.path().join("app");
        std::fs::create_dir_all(app.join(".git")).unwrap();
        std::fs::create_dir_all(app.join(".mise/tasks")).unwrap();
        assert_eq!(nested_checkout(&app.join(".mise/config.toml")), None);
        assert_eq!(
            nested_checkout(&app.join(".config/mise/conf.d/foo.toml")),
            None
        );
        assert_eq!(checkout_between(&app, &app.join(".mise/tasks")), None);
        // and so does a submodule there
        std::fs::create_dir_all(app.join(".git/modules/mise")).unwrap();
        std::fs::write(app.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        std::fs::write(app.join(".mise/.git"), "gitdir: ../.git/modules/mise\n").unwrap();
        assert_eq!(nested_checkout(&app.join(".mise/config.toml")), None);
    }
}
