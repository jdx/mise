//! The portable mapping between local paths and the setup branch.
//!
//! The setup branch mirrors the global configuration directory at its root
//! (`config.toml`, `conf.d/`, `tasks/`, templates), publishes sources under
//! `dotfiles.root` to `sources/dotfiles/<relative>` and other sources under
//! `$HOME` to `sources/home/<relative>`, and holds the shared version of
//! every tracked entry under `tracked/home/<relative>` (a variant stream
//! under `tracked/home@<variant>/<relative>`). A physical path maps to
//! exactly one branch path, a function of the path alone, so the same
//! content is published once. Sources outside `$HOME` are not portable.

use std::path::{Path, PathBuf};

use crate::system::history::tracked::{EntryKind, global_config_dir, normalize};

/// The explicit versioned marker of a history-enabled repository.
pub(crate) const MARKER_PATH: &str = ".mise-history/format.toml";
pub(crate) const TRACKED_PREFIX: &str = "tracked/home";
pub(crate) const SOURCES_DOTFILES: &str = "sources/dotfiles";
pub(crate) const SOURCES_HOME: &str = "sources/home";

/// The roots the mapping is relative to on this machine.
#[derive(Clone, Debug)]
pub(crate) struct Roots {
    pub home: PathBuf,
    pub config_dir: PathBuf,
    pub dotfiles_root: PathBuf,
}

impl Roots {
    pub(crate) fn current() -> Self {
        Self {
            home: normalize(&crate::dirs::HOME),
            config_dir: normalize(&global_config_dir()),
            dotfiles_root: normalize(&crate::system::files::dotfiles_root()),
        }
    }

    /// The setup-branch path of a local file, given the entry that owns it.
    /// `None` when the file is not portable (outside `$HOME`) or never
    /// shared as its own file (a rendered output).
    pub(crate) fn branch_path(
        &self,
        kind: EntryKind,
        local: &Path,
        variant: Option<&str>,
    ) -> Option<String> {
        if kind == EntryKind::Output {
            return None;
        }
        let rel_home = local.strip_prefix(&self.home).ok()?;
        if kind == EntryKind::Track {
            let stream = match variant {
                Some(variant) => format!("{TRACKED_PREFIX}@{variant}"),
                None => TRACKED_PREFIX.to_string(),
            };
            return Some(format!("{stream}/{}", slash(rel_home)));
        }
        if let Ok(rel) = local.strip_prefix(&self.config_dir) {
            return Some(slash(rel));
        }
        if let Ok(rel) = local.strip_prefix(&self.dotfiles_root) {
            return Some(format!("{SOURCES_DOTFILES}/{}", slash(rel)));
        }
        Some(format!("{SOURCES_HOME}/{}", slash(rel_home)))
    }

    /// Where a setup-branch path lands on this machine.
    pub(crate) fn locate(&self, branch_path: &str) -> Located {
        if !is_safe_branch_path(branch_path) {
            return Located::Unmapped;
        }
        if branch_path == MARKER_PATH || branch_path.starts_with(".mise-history/") {
            return Located::Marker;
        }
        if let Some(rest) = branch_path.strip_prefix(&format!("{TRACKED_PREFIX}@")) {
            let Some((variant, rel)) = rest.split_once('/') else {
                return Located::Unmapped;
            };
            return Located::Tracked {
                path: self.home.join(rel),
                variant: Some(variant.to_string()),
            };
        }
        if let Some(rel) = branch_path.strip_prefix(&format!("{TRACKED_PREFIX}/")) {
            return Located::Tracked {
                path: self.home.join(rel),
                variant: None,
            };
        }
        if let Some(rel) = branch_path.strip_prefix(&format!("{SOURCES_DOTFILES}/")) {
            return Located::Source(self.dotfiles_root.join(rel));
        }
        if let Some(rel) = branch_path.strip_prefix(&format!("{SOURCES_HOME}/")) {
            return Located::Source(self.home.join(rel));
        }
        if branch_path.starts_with("sources/") || branch_path.starts_with("tracked/") {
            return Located::Unmapped;
        }
        Located::Config(self.config_dir.join(branch_path))
    }
}

/// A setup-branch path resolved on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Located {
    /// A file of the global configuration directory.
    Config(PathBuf),
    /// A source referenced by a `[dotfiles]` declaration.
    Source(PathBuf),
    /// The shared version of a tracked entry; `variant` names its stream.
    Tracked {
        path: PathBuf,
        variant: Option<String>,
    },
    /// The repository marker: never materialized.
    Marker,
    /// A path under a reserved prefix this version does not understand.
    Unmapped,
}

impl Located {
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Config(path) | Self::Source(path) | Self::Tracked { path, .. } => Some(path),
            Self::Marker | Self::Unmapped => None,
        }
    }
}

/// Whether a setup-branch path is one this machine may materialize: a plain
/// relative path with no empty, `.`, or `..` components, no separators or
/// drive letters inside a component, no control characters, and no `.git`
/// directory (a hook committed upstream must never land in a checkout).
/// Anything else is never written or removed here.
pub(crate) fn is_safe_branch_path(branch_path: &str) -> bool {
    !branch_path.is_empty()
        && branch_path.split('/').all(|component| {
            !component.is_empty()
                && component != "."
                && component != ".."
                && !component.eq_ignore_ascii_case(".git")
                && !component.contains(['\\', ':'])
                && !component.chars().any(char::is_control)
        })
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether a file is configuration whose change may alter declarations
/// (`[tools]`, `[bootstrap.*]`, `[dotfiles]`) or an execution surface.
pub(crate) fn is_configuration(branch_path: &str) -> bool {
    !branch_path.starts_with("sources/")
        && !branch_path.starts_with("tracked/")
        && !branch_path.starts_with(".mise-history/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Roots {
        Roots {
            home: PathBuf::from("/home/u"),
            config_dir: PathBuf::from("/home/u/.config/mise"),
            dotfiles_root: PathBuf::from("/home/u/.dotfiles"),
        }
    }

    #[test]
    fn a_remote_path_never_escapes_its_root() {
        let roots = roots();
        for path in [
            "../.bashrc",
            "tracked/home/../../etc/passwd",
            "sources/home/./x",
            "/etc/passwd",
            "tracked/home//x",
            "tracked/home/.git/hooks/pre-commit",
            "tracked/home/.GIT/config",
            "tracked/home/a\\b",
            "sources/dotfiles/C:/x",
            "tracked@macos/",
            "config\u{0}.toml",
            "",
        ] {
            assert_eq!(roots.locate(path), Located::Unmapped, "{path:?}");
        }
        assert!(matches!(
            roots.locate("tracked/home/.gitconfig"),
            Located::Tracked { .. }
        ));
        assert!(matches!(roots.locate("conf.d/a.toml"), Located::Config(_)));
    }

    #[test]
    fn maps_every_kind_of_file() {
        let r = roots();
        assert_eq!(
            r.branch_path(EntryKind::Track, Path::new("/home/u/.zshrc"), None),
            Some("tracked/home/.zshrc".into())
        );
        assert_eq!(
            r.branch_path(EntryKind::Track, Path::new("/home/u/.zshrc"), Some("macos")),
            Some("tracked/home@macos/.zshrc".into())
        );
        assert_eq!(
            r.branch_path(
                EntryKind::Implicit,
                Path::new("/home/u/.config/mise/conf.d/a.toml"),
                None
            ),
            Some("conf.d/a.toml".into())
        );
        assert_eq!(
            r.branch_path(
                EntryKind::Source,
                Path::new("/home/u/.dotfiles/nvim/init.lua"),
                None
            ),
            Some("sources/dotfiles/nvim/init.lua".into())
        );
        assert_eq!(
            r.branch_path(
                EntryKind::Source,
                Path::new("/home/u/templates/x.tera"),
                None
            ),
            Some("sources/home/templates/x.tera".into())
        );
        assert_eq!(
            r.branch_path(EntryKind::Source, Path::new("/etc/x"), None),
            None
        );
        assert_eq!(
            r.branch_path(EntryKind::Output, Path::new("/home/u/.gitconfig"), None),
            None
        );
    }

    #[test]
    fn locates_branch_paths() {
        let r = roots();
        assert_eq!(
            r.locate("config.toml"),
            Located::Config(PathBuf::from("/home/u/.config/mise/config.toml"))
        );
        assert_eq!(
            r.locate("tracked/home@work/.gitconfig-work"),
            Located::Tracked {
                path: PathBuf::from("/home/u/.gitconfig-work"),
                variant: Some("work".into())
            }
        );
        assert_eq!(
            r.locate("tracked/home/.config/hypr/a.lua"),
            Located::Tracked {
                path: PathBuf::from("/home/u/.config/hypr/a.lua"),
                variant: None
            }
        );
        assert_eq!(
            r.locate("sources/dotfiles/nvim/init.lua"),
            Located::Source(PathBuf::from("/home/u/.dotfiles/nvim/init.lua"))
        );
        assert_eq!(r.locate(MARKER_PATH), Located::Marker);
        assert_eq!(r.locate("sources/other/x"), Located::Unmapped);
        assert!(is_configuration("config.toml"));
        assert!(!is_configuration("tracked/home/.zshrc"));
    }
}
