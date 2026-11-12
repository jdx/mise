//! Plugins installed for every user in system directories, such as a distro package's plugin in
//! `/usr/share/mise/plugins`.
//!
//! mise searches these after the user's own plugins directory, so a user plugin of the same name
//! always wins. A system plugin is used only when no one but root or the current user can change
//! anything in it: the same reasoning that lets mise read `/etc/mise` without trust. mise cannot
//! check that off Unix, so it ignores system plugin directories there.

use std::path::{Path, PathBuf};
use std::sync::LazyLock as Lazy;

use heck::ToKebabCase;

use crate::dirs;
use crate::file::display_path;
use crate::plugins::PluginType;

#[derive(Debug, Clone)]
pub struct SystemPlugin {
    pub name: String,
    pub path: PathBuf,
    pub plugin_type: PluginType,
}

static SYSTEM_PLUGINS: Lazy<Vec<SystemPlugin>> =
    Lazy::new(|| scan(&mise_util::env::MISE_SYSTEM_PLUGINS_DIRS, &dirs::PLUGINS));

/// Every usable system plugin, by name. A name found in an earlier directory hides the same
/// name in later ones, and a user plugin of the same name hides the system plugin.
pub fn list() -> &'static [SystemPlugin] {
    &SYSTEM_PLUGINS
}

/// The system plugin named `name`, unless a user plugin of that name overrides it. `foo_bar` and
/// `foo-bar` name the same plugin, since mise installs user plugins under the kebab-case name.
pub fn find(name: &str) -> Option<&'static SystemPlugin> {
    find_in(list(), name)
}

/// The identity of a plugin name: `foo_bar`, `foo-bar` and `FooBar` are the same plugin.
fn identity(name: &str) -> String {
    name.to_kebab_case()
}

fn find_in<'a>(plugins: &'a [SystemPlugin], name: &str) -> Option<&'a SystemPlugin> {
    let id = identity(name);
    plugins.iter().find(|plugin| identity(&plugin.name) == id)
}

/// The user's own plugin that `name` refers to, if one is installed.
fn user_plugin(user_dir: &Path, name: &str) -> Option<PathBuf> {
    [user_dir.join(identity(name)), user_dir.join(name)]
        .into_iter()
        .find(|path| path.exists())
}

/// Where the plugin named `name` lives: the user's plugins directory when it holds one, then a
/// system directory, and otherwise the user's directory, where it would be installed. The
/// directory's own name is the one vfox must be given to load the plugin.
pub fn resolve(name: &str, user_dir: &Path) -> PathBuf {
    user_plugin(user_dir, name)
        .or_else(|| find(name).map(|plugin| plugin.path.clone()))
        .unwrap_or_else(|| user_dir.join(identity(name)))
}

fn scan(system_dirs: &[PathBuf], user_dir: &Path) -> Vec<SystemPlugin> {
    let mut plugins: Vec<SystemPlugin> = Vec::new();
    for dir in system_dirs {
        if !dir.is_dir() {
            continue;
        }
        if let Err(reason) = protected(dir) {
            warn_once!("ignoring system plugins in {}: {reason}", display_path(dir));
            continue;
        }
        let names = match crate::file::dir_subdirs(dir) {
            Ok(names) => names,
            Err(err) => {
                warn_once!("failed to read {}: {err:#}", display_path(dir));
                continue;
            }
        };
        for name in names {
            // An earlier directory's plugin hides every spelling of its name in later ones.
            if name.starts_with('.') || find_in(&plugins, &name).is_some() {
                continue;
            }
            let path = dir.join(&name);
            if let Err(reason) = protected_tree(&path) {
                warn_once!("ignoring system plugin {}: {reason}", display_path(&path));
                continue;
            }
            let Some(plugin_type) = PluginType::from_plugin_path(&path) else {
                continue;
            };
            if let Some(user) = user_plugin(user_dir, &name) {
                warn_once!(
                    "plugin {name} in {} overrides the system plugin in {}",
                    display_path(&user),
                    display_path(&path)
                );
                // Still claim the name, so a later system directory cannot supply it either.
            }
            plugins.push(SystemPlugin {
                name,
                path,
                plugin_type,
            });
        }
    }
    plugins.retain(|plugin| user_plugin(user_dir, &plugin.name).is_none());
    plugins
}

/// Whether only root or the current user can change `path`, as reached through every directory
/// and link on the way to it: see [`resolve_protected`].
#[cfg(unix)]
fn protected(path: &Path) -> Result<(), String> {
    let real = resolve_protected(path)?;
    let meta = std::fs::metadata(&real).map_err(|err| format!("{}: {err}", display_path(path)))?;
    check_metadata(&meta).map_err(|reason| format!("{}: {reason}", display_path(path)))
}

#[cfg(unix)]
fn owned_by_root_or_me(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() == 0 || meta.uid() == nix::unistd::geteuid().as_raw()
}

/// Whether only root or the current user can change what `meta` describes: it must be owned by
/// one of them and not writable by its group or others.
#[cfg(unix)]
fn check_metadata(meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if !owned_by_root_or_me(meta) {
        return Err(format!(
            "it is owned by uid {}, not root or the current user",
            meta.uid()
        ));
    }
    if meta.mode() & 0o022 != 0 {
        return Err("it is writable by its group or by other users".into());
    }
    Ok(())
}

/// Whether no one but root or the current user can replace the entry `entry` in directory `dir`.
/// That holds when `dir` itself is protected, or when it is a sticky directory such as `/tmp`
/// owned by one of them and the entry is too, since then only its owner can rename or remove it.
#[cfg(unix)]
fn check_lookup(dir: &std::fs::Metadata, entry: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    match check_metadata(dir) {
        Ok(()) => Ok(()),
        Err(_)
            if dir.mode() & 0o1000 != 0
                && owned_by_root_or_me(dir)
                && owned_by_root_or_me(entry) =>
        {
            Ok(())
        }
        Err(reason) => Err(reason),
    }
}

/// Resolves `path` one component at a time, the way the kernel does, and checks that no one but
/// root or the current user can swap out anything on the way: each directory it looks a name up
/// in, and each symbolic link it follows, including links in the middle of a link's target.
/// Returns the path with every link resolved.
#[cfg(unix)]
fn resolve_protected(path: &Path) -> Result<PathBuf, String> {
    use std::collections::VecDeque;
    use std::path::Component;

    const MAX_LINKS: usize = 40;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|err| err.to_string())?
            .join(path)
    };
    let mut pending: VecDeque<_> = path
        .components()
        .map(|c| c.as_os_str().to_owned())
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut links = 0;
    while let Some(part) = pending.pop_front() {
        match Path::new(&part).components().next() {
            Some(Component::RootDir) => resolved = PathBuf::from("/"),
            Some(Component::CurDir) | None => {}
            Some(Component::ParentDir) => {
                resolved.pop();
            }
            Some(Component::Prefix(_)) => {}
            Some(Component::Normal(name)) => {
                let fail = |reason: String| format!("{}: {reason}", display_path(&resolved));
                let dir = std::fs::metadata(&resolved).map_err(|err| fail(err.to_string()))?;
                let candidate = resolved.join(name);
                let entry = std::fs::symlink_metadata(&candidate)
                    .map_err(|err| format!("{}: {err}", display_path(&candidate)))?;
                check_lookup(&dir, &entry).map_err(fail)?;
                if entry.file_type().is_symlink() {
                    links += 1;
                    if links > MAX_LINKS {
                        return Err(format!("{}: too many symbolic links", display_path(&path)));
                    }
                    let target = std::fs::read_link(&candidate)
                        .map_err(|err| format!("{}: {err}", display_path(&candidate)))?;
                    for part in target.components().rev() {
                        pending.push_front(part.as_os_str().to_owned());
                    }
                } else {
                    resolved = candidate;
                }
            }
        }
    }
    Ok(resolved)
}

/// Whether only root or the current user can change anything mise could load from the plugin at
/// `root`: the directory and every path to it, every file and directory under it, and every link
/// in it, with each directory and link on the way to the link's target.
#[cfg(unix)]
fn protected_tree(root: &Path) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let real = resolve_protected(&path)?;
        let meta =
            std::fs::metadata(&real).map_err(|err| format!("{}: {err}", display_path(&path)))?;
        check_metadata(&meta).map_err(|reason| format!("{}: {reason}", display_path(&path)))?;
        if meta.is_dir() && seen.insert(real.clone()) {
            let entries = std::fs::read_dir(&real)
                .map_err(|err| format!("{}: {err}", display_path(&path)))?;
            for entry in entries {
                let entry = entry.map_err(|err| format!("{}: {err}", display_path(&path)))?;
                pending.push(real.join(entry.file_name()));
            }
        }
    }
    Ok(())
}

/// Off Unix mise cannot tell who can change a directory, so it trusts none.
#[cfg(not(unix))]
fn protected(_path: &Path) -> Result<(), String> {
    Err("mise cannot check who can change it on this platform".into())
}

#[cfg(not(unix))]
fn protected_tree(path: &Path) -> Result<(), String> {
    protected(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A package plugin whose files only the owner can write, whatever the umask.
    fn package_plugin(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(path.join("hooks")).unwrap();
        std::fs::write(path.join("metadata.lua"), "PLUGIN = {}").unwrap();
        std::fs::write(path.join("hooks/package_installed.lua"), "").unwrap();
        std::fs::write(path.join("hooks/package_install.lua"), "").unwrap();
        for dir in [dir, &path, &path.join("hooks")] {
            set_mode(dir, 0o755);
        }
        for file in [
            "metadata.lua",
            "hooks/package_installed.lua",
            "hooks/package_install.lua",
        ] {
            set_mode(&path.join(file), 0o644);
        }
        path
    }

    /// A temporary directory only its owner can write, whatever the umask: system plugins under
    /// it must pass the check on every directory above them.
    fn tempdir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        set_mode(tmp.path(), 0o700);
        tmp
    }

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn earlier_system_directory_wins_and_user_plugin_overrides() {
        let tmp = tempdir();
        let vendor = tmp.path().join("vendor");
        let admin = tmp.path().join("admin");
        let user = tmp.path().join("user");
        package_plugin(&vendor, "alpha");
        package_plugin(&admin, "alpha");
        package_plugin(&admin, "beta");
        package_plugin(&vendor, "gamma");
        package_plugin(&user, "gamma");
        for dir in [&vendor, &admin] {
            set_mode(dir, 0o755);
        }

        let plugins = scan(&[vendor.clone(), admin.clone()], &user);
        let found: Vec<_> = plugins
            .iter()
            .map(|plugin| (plugin.name.as_str(), plugin.path.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("alpha", vendor.join("alpha")),
                ("beta", admin.join("beta"))
            ]
        );
        assert_eq!(plugins[0].plugin_type, PluginType::Package);
    }

    #[test]
    fn writable_directories_are_ignored() {
        let tmp = tempdir();
        let shared = tmp.path().join("shared");
        let plugin = package_plugin(&shared, "alpha");
        set_mode(&shared, 0o777);
        assert!(scan(std::slice::from_ref(&shared), &tmp.path().join("user")).is_empty());

        set_mode(&shared, 0o755);
        set_mode(&plugin, 0o775);
        assert!(scan(std::slice::from_ref(&shared), &tmp.path().join("user")).is_empty());

        set_mode(&plugin, 0o755);
        assert_eq!(scan(&[shared], &tmp.path().join("user")).len(), 1);
    }

    #[test]
    fn writable_files_inside_a_plugin_are_ignored() {
        let tmp = tempdir();
        let system = tmp.path().join("system");
        let user = tmp.path().join("user");
        let plugin = package_plugin(&system, "alpha");
        set_mode(&plugin.join("hooks/package_install.lua"), 0o666);
        assert!(scan(std::slice::from_ref(&system), &user).is_empty());

        set_mode(&plugin.join("hooks/package_install.lua"), 0o644);
        set_mode(&plugin.join("hooks"), 0o777);
        assert!(scan(std::slice::from_ref(&system), &user).is_empty());

        set_mode(&plugin.join("hooks"), 0o755);
        assert_eq!(scan(&[system], &user).len(), 1);
    }

    #[test]
    fn links_to_writable_files_are_ignored() {
        let tmp = tempdir();
        let system = tmp.path().join("system");
        let shared = tmp.path().join("shared");
        let plugin = package_plugin(&system, "alpha");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::write(shared.join("install.lua"), "").unwrap();
        set_mode(&shared.join("install.lua"), 0o644);
        set_mode(&shared, 0o777);
        let hook = plugin.join("hooks/package_install.lua");
        std::fs::remove_file(&hook).unwrap();
        std::os::unix::fs::symlink(shared.join("install.lua"), &hook).unwrap();
        // The target is protected, but anyone can replace it in its directory.
        assert!(scan(std::slice::from_ref(&system), &tmp.path().join("user")).is_empty());

        set_mode(&shared, 0o755);
        assert_eq!(scan(&[system], &tmp.path().join("user")).len(), 1);
    }

    #[test]
    fn snake_and_kebab_case_names_find_the_same_plugin() {
        let tmp = tempdir();
        let system = tmp.path().join("system");
        let plugin = package_plugin(&system, "foo_bar");
        let plugins = scan(&[system], &tmp.path().join("user"));
        assert_eq!(find_in(&plugins, "foo-bar").map(|p| &p.path), Some(&plugin));
        assert_eq!(find_in(&plugins, "foo_bar").map(|p| &p.path), Some(&plugin));
    }

    #[test]
    fn links_through_a_writable_directory_are_ignored() {
        let tmp = tempdir();
        let system = tmp.path().join("system");
        let shared = tmp.path().join("shared");
        let safe = tmp.path().join("safe");
        let plugin = package_plugin(&system, "alpha");
        std::fs::create_dir_all(&safe).unwrap();
        std::fs::write(safe.join("install.lua"), "").unwrap();
        set_mode(&safe.join("install.lua"), 0o644);
        set_mode(&safe, 0o755);
        std::fs::create_dir_all(&shared).unwrap();
        // The final target and its directory are protected, but the link in the middle lives
        // where anyone can replace it.
        std::os::unix::fs::symlink(&safe, shared.join("redirect")).unwrap();
        set_mode(&shared, 0o777);
        let hook = plugin.join("hooks/package_install.lua");
        std::fs::remove_file(&hook).unwrap();
        std::os::unix::fs::symlink(shared.join("redirect/install.lua"), &hook).unwrap();
        assert!(scan(std::slice::from_ref(&system), &tmp.path().join("user")).is_empty());

        set_mode(&shared, 0o755);
        assert_eq!(scan(&[system], &tmp.path().join("user")).len(), 1);
    }

    #[test]
    fn a_name_spelled_differently_is_still_the_same_plugin() {
        let tmp = tempdir();
        let vendor = tmp.path().join("vendor");
        let admin = tmp.path().join("admin");
        let user = tmp.path().join("user");
        let first = package_plugin(&vendor, "foo_bar");
        package_plugin(&admin, "foo-bar");
        let plugins = scan(&[vendor.clone(), admin.clone()], &user);
        // the earlier directory wins whichever spelling config uses
        assert_eq!(plugins.len(), 1);
        assert_eq!(find_in(&plugins, "foo-bar").map(|p| &p.path), Some(&first));
        assert_eq!(find_in(&plugins, "foo_bar").map(|p| &p.path), Some(&first));

        // a user plugin under the kebab-case name overrides the snake_case system plugin
        package_plugin(&user, "foo-bar");
        assert!(scan(&[vendor, admin], &user).is_empty());
    }
}
