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

fn find_in<'a>(plugins: &'a [SystemPlugin], name: &str) -> Option<&'a SystemPlugin> {
    plugins
        .iter()
        .find(|plugin| plugin.name == name)
        .or_else(|| {
            let kebab = name.to_kebab_case();
            plugins
                .iter()
                .find(|plugin| plugin.name.to_kebab_case() == kebab)
        })
}

/// Where the plugin named `name` lives: the user's plugins directory when it holds one, then a
/// system directory, and otherwise the user's directory, where it would be installed. The
/// directory's own name is the one vfox must be given to load the plugin.
pub fn resolve(name: &str, user_dir: &Path) -> PathBuf {
    let user = user_dir.join(name.to_kebab_case());
    if user.exists() {
        return user;
    }
    let exact = user_dir.join(name);
    if exact.exists() {
        return exact;
    }
    find(name).map_or(user, |plugin| plugin.path.clone())
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
            if name.starts_with('.') || plugins.iter().any(|plugin| plugin.name == name) {
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
            let user = user_dir.join(&name);
            if user.exists() {
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
    plugins.retain(|plugin| !user_dir.join(&plugin.name).exists());
    plugins
}

/// Whether only root or the current user can change `path`: it must be owned by one of them and
/// not writable by its group or others.
#[cfg(unix)]
fn protected(path: &Path) -> Result<(), String> {
    check_metadata(&std::fs::metadata(path).map_err(|err| err.to_string())?)
}

#[cfg(unix)]
fn check_metadata(meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let uid = meta.uid();
    if uid != 0 && uid != nix::unistd::geteuid().as_raw() {
        return Err(format!(
            "it is owned by uid {uid}, not root or the current user"
        ));
    }
    if meta.mode() & 0o022 != 0 {
        return Err("it is writable by its group or by other users".into());
    }
    Ok(())
}

/// Whether only root or the current user can change anything mise could load from the plugin at
/// `root`: the directory, every file and directory under it, and where each symbolic link points,
/// with the directory that holds the link's target.
#[cfg(unix)]
fn protected_tree(root: &Path) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let real = std::fs::canonicalize(&path)
            .map_err(|err| format!("{}: {err}", display_path(&path)))?;
        if !seen.insert(real.clone()) {
            continue;
        }
        if real != path {
            // A link, or a path through one: the target and the directory holding it must be
            // protected too, or someone else could swap the target out.
            if let Some(parent) = real.parent() {
                protected(parent)
                    .map_err(|reason| format!("{}: {reason}", display_path(parent)))?;
            }
        }
        let meta =
            std::fs::metadata(&real).map_err(|err| format!("{}: {err}", display_path(&path)))?;
        check_metadata(&meta).map_err(|reason| format!("{}: {reason}", display_path(&path)))?;
        if meta.is_dir() {
            let entries = std::fs::read_dir(&real)
                .map_err(|err| format!("{}: {err}", display_path(&path)))?;
            for entry in entries {
                let entry = entry.map_err(|err| format!("{}: {err}", display_path(&path)))?;
                pending.push(entry.path());
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

    fn set_mode(path: &Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn earlier_system_directory_wins_and_user_plugin_overrides() {
        let tmp = tempfile::tempdir().unwrap();
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
        let tmp = tempfile::tempdir().unwrap();
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
        let tmp = tempfile::tempdir().unwrap();
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
        let tmp = tempfile::tempdir().unwrap();
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
        let tmp = tempfile::tempdir().unwrap();
        let system = tmp.path().join("system");
        let plugin = package_plugin(&system, "foo_bar");
        let plugins = scan(&[system], &tmp.path().join("user"));
        assert_eq!(find_in(&plugins, "foo-bar").map(|p| &p.path), Some(&plugin));
        assert_eq!(find_in(&plugins, "foo_bar").map(|p| &p.path), Some(&plugin));
    }
}
