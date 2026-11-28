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

/// The project a config was found in: its config root, or for a conf.d folder fragment the
/// directory that holds the `conf.d`.
///
/// A fragment's project comes from the directories the fragment is in. The `mise.toml` beside
/// its `conf.d` can be a link to global config, whose root is not the project.
fn enclosing_project(path: &Path) -> PathBuf {
    let root = config_root(path);
    if is_conf_d_folder_file(path)
        && let Some(conf_d) = root.parent()
    {
        return root_by_name(&conf_d.join("mise.toml"));
    }
    root
}

/// The project that holds a config root of its own: a conf.d folder, or `.config/mise` for
/// `.config/mise/mise.toml`. `None` for any other config and for global config.
///
/// Such a root is part of the project, so an ignore or untrust of the project covers it.
pub fn nested_root_project(path: &Path) -> Option<PathBuf> {
    if is_global_config(path) {
        return None;
    }
    let path = path.absolutize().ok()?.to_path_buf();
    let root = config_root(&path);
    let project = outer_project(&path);
    (project != root && root.starts_with(&project)).then_some(project)
}

/// The project directory a config belongs to: [`enclosing_project`], with `.config/mise` taken
/// back to the directory that holds `.config`.
fn outer_project(path: &Path) -> PathBuf {
    let project = enclosing_project(path);
    if project.ends_with(".config/mise")
        && let Some(outer) = project.parent().and_then(Path::parent)
    {
        return outer.to_path_buf();
    }
    project
}

/// The project a config was found in, when a symlink lies between that project's directory and
/// the config, the config itself included.
///
/// Such a link is content of the project, so it decides nothing about trust: a config reached
/// through it is trusted only when the project is, whatever the link leads to. Global and system
/// config are exempt, since the user placed them and any link in them.
pub(crate) fn linked_project(path: &Path) -> Option<PathBuf> {
    if is_global_config(path) {
        return None;
    }
    let path = path.absolutize().ok()?.to_path_buf();
    let project = outer_project(&path);
    path.ancestors()
        .take_while(|p| *p != project && p.starts_with(&project))
        .any(is_link)
        .then_some(project)
}

/// Whether `path` is a link: a symlink, or on Windows a directory junction too, which
/// `Path::is_symlink` does not report, which needs no administrator rights to make, and which
/// `canonicalize` follows like a symlink.
pub(crate) fn is_link(path: &Path) -> bool {
    path.is_symlink() || is_junction(path)
}

/// Whether `path` is a directory junction (`mklink /J`). `read_link` reads the two name-surrogate
/// reparse points, symlinks and junctions, and fails on the others, such as a cloud file's.
#[cfg(windows)]
fn is_junction(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    path.symlink_metadata()
        .is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        && std::fs::read_link(path).is_ok()
}

#[cfg(not(windows))]
fn is_junction(_path: &Path) -> bool {
    false
}

/// [`linked_project`] for a lockfile, which sits beside the config that owns it: a link that the
/// config would be reached through also leads to its lockfile.
///
/// Every config that may own the lockfile is asked, and the outermost project wins. The project
/// is the boundary the lockfile's directory must resolve inside, so a candidate whose own root
/// is a linked directory must not narrow it to that directory.
///
/// A conf.d fragment is a candidate of its own: in a `.mise` directory with no `mise.toml`, the
/// root of a `mise.toml` there would be `.mise` itself, and a `config.toml` there may be global
/// config, so neither would see a `.mise` that links out of the project.
pub(crate) fn linked_lockfile_project(lockfile: &Path) -> Option<PathBuf> {
    let conf_d_fragment = lockfile.with_file_name("conf.d").join("fragment.toml");
    ["mise.toml", "config.toml"]
        .into_iter()
        .map(|name| lockfile.with_file_name(name))
        .chain([conf_d_fragment])
        .filter_map(|candidate| linked_project(&candidate))
        .min_by_key(|project| project.components().count())
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
    let out = root_by_name(&path);
    CONFIG_ROOT_CACHE.lock().unwrap().insert(path, out.clone());
    out
}

/// The root of the config at the absolute `path` from its name and the directories it is in,
/// whatever file is there: [`config_root`] without its global config lookup.
fn root_by_name(path: &Path) -> PathBuf {
    let path = path.to_path_buf();
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
    // A file task grouped in a subdirectory (`mise-tasks/sub/nested`) belongs
    // to the same project as one directly in the task directory. Files that
    // are project configs rather than tasks (`mise.toml`, `.tool-versions`,
    // idiomatic version files) keep their own directory as the root, so a
    // project that merely lives below a directory called `tasks` is not folded
    // into the outer one. A `config.toml` here is a task include, not a
    // project config: the project-config spelling is handled above.
    let is_project_config = filename.starts_with('.')
        || (filename.starts_with("mise") && filename.ends_with(".toml"))
        || matches!(
            filename,
            "package.json"
                | "go.mod"
                | "Gemfile"
                | "Pipfile"
                | "pyproject.toml"
                | "rust-toolchain.toml"
                | "global.json"
                | "deno.json"
                | "deno.jsonc"
        );
    let nested_task_root = || {
        if is_project_config {
            return None;
        }
        // Skip the file's own directory: the direct-child cases are below.
        let dirs = path.ancestors().skip(2).collect::<Vec<_>>();
        dirs.iter().find_map(|dir| {
            let name = dir.file_name()?.to_string_lossy();
            let above = dir.parent()?;
            let above_name = above.file_name().map(|n| n.to_string_lossy());
            if name == "mise-tasks" || name == ".mise-tasks" {
                dir.parent().map(Path::to_path_buf)
            } else if name == "tasks" && above_name.as_deref().is_some_and(is_mise_dir) {
                let base = above.parent()?;
                if base.file_name().is_some_and(|n| n == ".config") {
                    base.parent().map(Path::to_path_buf)
                } else {
                    Some(base.to_path_buf())
                }
            } else {
                None
            }
        })
    };
    if parent == "mise-tasks" || parent == ".mise-tasks" {
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
    } else if let Some(root) = nested_task_root() {
        root
    } else {
        parent_path()
    }
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

    /// A conf.d folder and `.config/mise` are roots of their own, held by the project around them.
    #[test]
    fn a_nested_config_root_belongs_to_the_project_around_it() {
        let temp = tempfile::tempdir().unwrap();
        let p = temp.path().join("proj");
        for nested in [
            ".mise/conf.d/x/mise.local.toml",
            "mise/conf.d/x/mise.toml",
            ".config/mise/mise.local.toml",
            ".config/mise/mise.toml",
            ".config/mise/conf.d/x/mise.toml",
        ] {
            assert_eq!(
                nested_root_project(&p.join(nested)),
                Some(p.clone()),
                "{nested}"
            );
        }
        for own in [
            "mise.toml",
            ".mise/config.toml",
            ".config/mise.toml",
            "mise.local.toml",
        ] {
            assert_eq!(nested_root_project(&p.join(own)), None, "{own}");
        }
    }

    /// Any link between a project and its config makes the config the project's, whatever the
    /// link leads to.
    #[test]
    fn a_link_inside_a_project_ties_the_config_to_the_project() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let other = root.join("other");
        std::fs::create_dir_all(other.join("x")).unwrap();
        std::fs::write(other.join("mise.toml"), "").unwrap();
        std::fs::write(other.join("x/mise.toml"), "").unwrap();

        let plain = root.join("plain");
        std::fs::create_dir_all(plain.join(".mise/conf.d/x")).unwrap();
        std::fs::write(plain.join("mise.toml"), "").unwrap();
        std::fs::write(plain.join(".mise/conf.d/x/mise.toml"), "").unwrap();
        assert_eq!(linked_project(&plain.join("mise.toml")), None);
        assert_eq!(
            linked_project(&plain.join(".mise/conf.d/x/mise.toml")),
            None
        );

        // a conf.d folder that is a link out of the project
        let folder = root.join("folder");
        std::fs::create_dir_all(folder.join(".mise/conf.d")).unwrap();
        symlink(other.join("x"), folder.join(".mise/conf.d/x")).unwrap();
        assert_eq!(
            linked_project(&folder.join(".mise/conf.d/x/mise.toml")),
            Some(folder.clone())
        );

        // conf.d itself is the link
        let conf_d = root.join("conf-d");
        std::fs::create_dir_all(conf_d.join(".mise")).unwrap();
        symlink(&other, conf_d.join(".mise/conf.d")).unwrap();
        assert_eq!(
            linked_project(&conf_d.join(".mise/conf.d/x/mise.toml")),
            Some(conf_d.clone())
        );

        // the config file is the link
        let file = root.join("file");
        std::fs::create_dir_all(&file).unwrap();
        symlink(other.join("mise.toml"), file.join("mise.toml")).unwrap();
        assert_eq!(linked_project(&file.join("mise.toml")), Some(file.clone()));

        // a project named `mise`, whose config root is its parent: a link to a sibling stays
        // inside that parent and still counts
        let w = root.join("w");
        std::fs::create_dir_all(w.join("mise")).unwrap();
        symlink(other.join("mise.toml"), w.join("mise/config.toml")).unwrap();
        assert_eq!(linked_project(&w.join("mise/config.toml")), Some(w.clone()));

        // `.config/mise/mise.toml` has `.config/mise` as its root, but a link at `.config` or
        // `.config/mise` still lies inside the project that holds `.config`
        for link in [".config", ".config/mise"] {
            let project = root.join(format!("dot-{}", link.replace('/', "-")));
            let link = project.join(link);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            symlink(&other, &link).unwrap();
            assert_eq!(
                linked_project(&project.join(".config/mise/mise.toml")),
                Some(project.clone()),
                "{}",
                link.display()
            );
        }
        let plain_config = root.join("plain-config");
        std::fs::create_dir_all(plain_config.join(".config/mise")).unwrap();
        assert_eq!(
            linked_project(&plain_config.join(".config/mise/mise.toml")),
            None
        );
    }

    #[test]
    fn a_lockfile_reached_through_a_link_in_a_project_belongs_to_it() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let other = root.join("other");
        std::fs::create_dir_all(other.join("mise")).unwrap();

        let plain = root.join("plain");
        std::fs::create_dir_all(plain.join(".config/mise")).unwrap();
        assert_eq!(linked_lockfile_project(&plain.join("mise.lock")), None);
        assert_eq!(
            linked_lockfile_project(&plain.join(".config/mise/mise.lock")),
            None
        );

        // `.config` is the link
        let dot_config = root.join("dot-config");
        std::fs::create_dir_all(&dot_config).unwrap();
        symlink(&other, dot_config.join(".config")).unwrap();
        assert_eq!(
            linked_lockfile_project(&dot_config.join(".config/mise.lock")),
            Some(dot_config.clone())
        );
        assert_eq!(
            linked_lockfile_project(&dot_config.join(".config/mise/mise.lock")),
            Some(dot_config.clone())
        );

        // `.mise` is the link
        let dot_mise = root.join("dot-mise");
        std::fs::create_dir_all(&dot_mise).unwrap();
        symlink(other.join("mise"), dot_mise.join(".mise")).unwrap();
        assert_eq!(
            linked_lockfile_project(&dot_mise.join(".mise/mise.lock")),
            Some(dot_mise.clone())
        );

        // `.mise` is the link, and the config inside what it leads to is a link too: the
        // boundary stays the project that holds `.mise`, not `.mise` itself
        let nested = root.join("nested");
        let shared = root.join("shared");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(other.join("real.toml"), "").unwrap();
        symlink(other.join("real.toml"), shared.join("mise.toml")).unwrap();
        symlink(other.join("real.toml"), shared.join("config.toml")).unwrap();
        symlink(&shared, nested.join(".mise")).unwrap();
        assert_eq!(
            linked_lockfile_project(&nested.join(".mise/mise.lock")),
            Some(nested.clone())
        );
    }

    #[test]
    fn a_symlink_is_a_link_and_a_directory_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dir");
        std::fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(&dir, temp.path().join("link")).unwrap();
        assert!(is_link(&temp.path().join("link")));
        assert!(!is_link(&dir));
        assert!(!is_link(&temp.path().join("missing")));
    }

    /// A file task grouped in a subdirectory shares its project's root.
    #[test]
    fn nested_file_tasks_share_the_project_root() {
        for p in [
            "/foo/bar/mise-tasks/sub/nested",
            "/foo/bar/mise-tasks/a/b/nested",
            "/foo/bar/.mise-tasks/sub/nested",
            "/foo/bar/.mise/tasks/sub/nested",
            "/foo/bar/mise/tasks/sub/nested",
            "/foo/bar/.config/mise/tasks/sub/nested",
        ] {
            assert_eq!(config_root(Path::new(p)), Path::new("/foo/bar"), "{p}");
        }
        // Project configs below a directory named `tasks` are their own root,
        // while a `config.toml` task include is not.
        assert_eq!(
            config_root(Path::new("/foo/mise/tasks/proj/.tool-versions")),
            Path::new("/foo/mise/tasks/proj")
        );
        assert_eq!(
            config_root(Path::new("/foo/bar/mise-tasks/sub/config.toml")),
            Path::new("/foo/bar")
        );
        assert_eq!(
            config_root(Path::new("/foo/mise/tasks/proj/mise.toml")),
            Path::new("/foo/mise/tasks/proj")
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

    /// A fragment belongs to the project that holds its `conf.d`, also when the `mise.toml`
    /// beside that `conf.d` is a link to global config.
    #[test]
    fn a_fragment_belongs_to_its_project_beside_a_link_to_global_config() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("proj");
        let conf_d = project.join(".mise").join("conf.d");
        std::fs::create_dir_all(conf_d.join("x")).unwrap();
        std::os::unix::fs::symlink(
            crate::config::global_config_path(),
            conf_d.join("mise.toml"),
        )
        .unwrap();
        let fragment = conf_d.join("x").join("mise.toml");
        std::fs::write(&fragment, "").unwrap();
        assert!(is_global_config(&conf_d.join("mise.toml")));

        assert_eq!(nested_root_project(&fragment), Some(project.clone()));
        std::fs::remove_file(&fragment).unwrap();
        std::fs::remove_dir(conf_d.join("x")).unwrap();
        std::os::unix::fs::symlink(temp.path(), conf_d.join("x")).unwrap();
        assert_eq!(linked_project(&fragment), Some(project));
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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// A junction is not a symlink to `Path::is_symlink`, but `canonicalize` follows it, so a
    /// junction in a project counts as a link the way a symlink does.
    #[test]
    fn a_junction_is_a_link_and_a_directory_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dir");
        let junction = temp.path().join("junction");
        std::fs::create_dir_all(&dir).unwrap();
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(&junction)
            .arg(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
        assert!(!junction.is_symlink());
        assert!(is_link(&junction));
        assert!(!is_link(&dir));

        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let status = std::process::Command::new("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            .arg(project.join(".config"))
            .arg(&dir)
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed");
        assert_eq!(
            linked_project(&project.join(".config/mise.toml")),
            Some(project.absolutize().unwrap().to_path_buf())
        );
    }
}
