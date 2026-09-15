//! Portable paths depend on location, never on deployment ownership.
use std::path::{Path, PathBuf};

use crate::system::history::tracked::{global_config_dir, normalize};

pub(crate) const MARKER_PATH: &str = ".mise-history/format.toml";

#[derive(Clone, Debug)]
pub(crate) struct Roots {
    pub home: PathBuf,
    pub config_dir: PathBuf,
}

impl Roots {
    pub(crate) fn current() -> Self {
        Self {
            home: normalize(&crate::dirs::HOME),
            config_dir: normalize(&global_config_dir()),
        }
    }

    pub(crate) fn branch_path(&self, local: &Path, variant: Option<&str>) -> Option<String> {
        local.to_str()?;
        if variant.is_some_and(|name| {
            name.contains('@') || !is_safe_branch_path(name) || name.contains('/')
        }) {
            return None;
        }
        let (root, relative) = if let Ok(relative) = local.strip_prefix(&self.config_dir) {
            ("config", relative)
        } else {
            ("home", local.strip_prefix(&self.home).ok()?)
        };
        let stem = variant.map_or_else(|| root.to_string(), |variant| format!("{root}@{variant}"));
        let path = if relative.as_os_str().is_empty() {
            stem
        } else {
            format!("{stem}/{}", slash(relative))
        };
        is_safe_branch_path(&path).then_some(path)
    }

    pub(crate) fn locate(&self, branch_path: &str) -> Located {
        if !is_safe_branch_path(branch_path) {
            return Located::Unmapped;
        }
        if branch_path.starts_with(".mise-history/") {
            return Located::Marker;
        }
        let (stem, relative) = branch_path.split_once('/').unwrap_or((branch_path, ""));
        let (root, variant) = stem
            .split_once('@')
            .map_or((stem, None), |(root, variant)| (root, Some(variant)));
        if variant.is_some_and(|variant| variant.is_empty() || variant.contains('@')) {
            return Located::Unmapped;
        }
        let base = match root {
            "home" => &self.home,
            "config" => &self.config_dir,
            _ => return Located::Unmapped,
        };
        if root == "config" && variant.is_none() {
            return Located::Config(base.join(relative));
        }
        Located::Tracked {
            path: base.join(relative),
            variant: variant.map(str::to_owned),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Located {
    Config(PathBuf),
    Tracked {
        path: PathBuf,
        variant: Option<String>,
    },
    Marker,
    Unmapped,
}

impl Located {
    pub(crate) fn path(&self) -> Option<&Path> {
        match self {
            Self::Config(path) | Self::Tracked { path, .. } => Some(path),
            Self::Marker | Self::Unmapped => None,
        }
    }
}

pub(crate) fn is_safe_branch_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|component| {
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

pub(crate) fn is_configuration(path: &str) -> bool {
    path == "config" || path.starts_with("config/") || path.starts_with("config@")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn roots() -> Roots {
        Roots {
            home: "/home/u".into(),
            config_dir: "/config/mise".into(),
        }
    }

    #[test]
    fn mapping_is_independent_of_deployment_ownership() {
        let roots = roots();
        assert_eq!(
            roots.branch_path(Path::new("/home/u/templates/gitconfig.tera"), None),
            Some("home/templates/gitconfig.tera".into())
        );
        assert_eq!(
            roots.branch_path(Path::new("/config/mise/config.toml"), None),
            Some("config/config.toml".into())
        );
        assert_eq!(roots.branch_path(Path::new("/etc/passwd"), None), None);
    }

    #[test]
    fn variants_and_config_roots_round_trip() {
        let roots = roots();
        for (local, variant) in [
            ("/home/u/.zshrc", Some("macos")),
            ("/config/mise/config.toml", None),
            ("/config/mise", None),
        ] {
            let path = roots.branch_path(Path::new(local), variant).unwrap();
            assert_eq!(roots.locate(&path).path(), Some(Path::new(local)));
        }
    }

    #[test]
    fn unsafe_and_repository_owned_paths_are_never_materialized() {
        let roots = roots();
        for path in [
            "../x",
            "home/../../etc/passwd",
            "home/.git/config",
            "home/a\\b",
            "home@/x",
            "README.md",
            "sources/home/x",
            "tracked/home/x",
            "/etc/passwd",
        ] {
            assert_eq!(roots.locate(path), Located::Unmapped, "{path}");
        }
        assert_eq!(roots.locate(MARKER_PATH), Located::Marker);
    }
}
