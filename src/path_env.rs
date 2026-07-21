use crate::config::Settings;
use crate::dirs;
use std::env::{join_paths, split_paths};
use std::ffi::OsString;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

pub struct PathEnv {
    pre: Vec<PathBuf>,
    mise: Vec<PathBuf>,
    post: Vec<PathBuf>,
    seen_shims: bool,
}

impl PathEnv {
    pub fn new() -> Self {
        Self {
            pre: Vec::new(),
            mise: Vec::new(),
            post: Vec::new(),
            seen_shims: false,
        }
    }

    pub fn add(&mut self, path: PathBuf) {
        for part in split_paths(&path) {
            self.mise.push(part);
        }
    }

    pub fn to_vec(&self) -> Vec<PathBuf> {
        self.pre
            .iter()
            .chain(self.mise.iter())
            .chain(self.post.iter())
            .map(|p| p.to_path_buf())
            .collect()
    }

    pub fn join(&self) -> OsString {
        join_paths(self.to_vec()).unwrap()
    }
}

impl Display for PathEnv {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.join().to_string_lossy())
    }
}

impl FromIterator<PathBuf> for PathEnv {
    fn from_iter<T: IntoIterator<Item = PathBuf>>(paths: T) -> Self {
        let settings = Settings::get();

        // When not_found_auto_install is enabled, preserve shims in PATH so they can
        // trigger auto-install for tools that aren't installed yet
        let preserve_shims = settings.not_found_auto_install;

        let mut path_env = Self::new();

        for path in paths {
            if path_env.seen_shims {
                path_env.post.push(path);
            } else if crate::file::paths_eq(&crate::file::replace_path(&path), &dirs::SHIMS)
                && !settings.activate_aggressive
            {
                path_env.seen_shims = true;
                if preserve_shims {
                    path_env.post.push(path);
                }
            } else {
                path_env.pre.push(path);
            }
        }
        if !path_env.seen_shims {
            path_env.post = path_env.pre;
            path_env.pre = Vec::new();
        }

        path_env
    }
}

impl PathEnv {
    pub fn from_path_str(path: &str) -> Self {
        Self::from_iter(split_paths(path))
    }
}

impl FromStr for PathEnv {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_path_str(s))
    }
}

/// All mise-managed install dirs: the primary install dir plus any shared/system
/// install dirs (`MISE_SHARED_INSTALL_DIRS` and the system installs dir) that
/// `env::find_in_shared_installs` resolves tool runtime paths into. Computed once
/// and passed to [`is_mise_install_path`] so the per-PATH-entry check stays cheap.
pub(crate) fn mise_install_dirs() -> Vec<PathBuf> {
    let mut install_dirs = vec![dirs::INSTALLS.to_path_buf()];
    install_dirs.extend(crate::env::shared_install_dirs());
    install_dirs
}

/// Whether `path` is under one of `install_dirs` (see [`mise_install_dirs`]),
/// checked both literally and via canonicalized paths. Such dirs are mise-managed,
/// so a stale one left on PATH (e.g. carried in from a frozen env snapshot) must
/// not outrank the version the current toolset selects. Shared by hook-env
/// reactivation (#10162) and the `mise x`/`run`/`env` child PATH (#10345).
pub(crate) fn is_mise_install_path(path: &std::path::Path, install_dirs: &[PathBuf]) -> bool {
    if install_dirs.iter().any(|d| path.starts_with(d)) {
        return true;
    }
    let Some(path) = crate::file::canonicalize_cached(path) else {
        return false;
    };
    install_dirs
        .iter()
        .filter_map(|d| crate::file::canonicalize_cached(d))
        .any(|d| path.starts_with(d))
}

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::config::Config;

    use super::*;

    #[tokio::test]
    async fn test_path_env() {
        let _config = Config::get().await.unwrap();
        let shims = dirs::SHIMS.to_str().unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                shims,
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            format!("/before-1:/before-2:/before-3:/1:/2:/3:{shims}:/after-1:/after-2:/after-3")
        );
    }

    #[tokio::test]
    async fn test_path_env_no_mise() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                "/after-1",
                "/after-2",
                "/after-3",
            ]
            .map(PathBuf::from),
        );
        path_env.add("/1".into());
        path_env.add("/2".into());
        path_env.add("/3".into());
        assert_eq!(
            path_env.to_string(),
            format!("/1:/2:/3:/before-1:/before-2:/before-3:/after-1:/after-2:/after-3")
        );
    }
    #[tokio::test]
    async fn test_path_env_with_colon() {
        let _config = Config::get().await.unwrap();
        let mut path_env = PathEnv::from_iter(["/item1", "/item2"].map(PathBuf::from));
        path_env.add("/1:/2".into());
        assert_eq!(path_env.to_string(), format!("/1:/2:/item1:/item2"));
    }
}
