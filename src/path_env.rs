use std::env::join_paths;
use std::ffi::OsString;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use itertools::Itertools;

use crate::config::Settings;
use crate::dirs;

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
        self.mise.push(path);
    }

    pub fn to_vec(&self) -> Vec<PathBuf> {
        let mut paths = self
            .pre
            .iter()
            .chain(self.mise.iter())
            .map(|p| p.to_path_buf())
            .collect_vec();
        if self.seen_shims {
            paths.push(dirs::SHIMS.to_path_buf())
        }
        paths
            .into_iter()
            .chain(self.post.iter().map(|p| p.to_path_buf()))
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

        let mut path_env = Self::new();

        for path in paths {
            if path_env.seen_shims {
                path_env.post.push(path);
            } else if path == *dirs::SHIMS && !settings.activate_aggressive {
                path_env.seen_shims = true;
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use crate::test::reset;

    use super::*;

    #[test]
    fn test_path_env() {
        reset();
        let mut path_env = PathEnv::from_iter(
            [
                "/before-1",
                "/before-2",
                "/before-3",
                dirs::SHIMS.to_str().unwrap(),
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
            format!(
                "/before-1:/before-2:/before-3:/1:/2:/3:{}:/after-1:/after-2:/after-3",
                dirs::SHIMS.to_str().unwrap()
            )
        );
    }

    #[test]
    fn test_path_env_no_mise() {
        reset();
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
    #[test]
    fn test_path_env_with_colon() {
        reset();
        let mut path_env = PathEnv::from_iter(
            [
                "/before1",
                "/before2"
            ]
            .map(PathBuf::from),
        );
        path_env.add("/after1:/after2".into());
        assert_eq!(
            path_env.to_string(),
            format!("/before1:/before2:/after1:/after2")
        );
    }
}
