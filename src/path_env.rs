use crate::dirs;
use itertools::Itertools;
use std::env::join_paths;
use std::ffi::OsString;
use std::fmt;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

pub struct PathEnv {
    pre: Vec<PathBuf>,
    rtx: Vec<PathBuf>,
    post: Vec<PathBuf>,
    seen_shims: bool,
}

impl PathEnv {
    pub fn new() -> Self {
        Self {
            pre: Vec::new(),
            rtx: Vec::new(),
            post: Vec::new(),
            seen_shims: false,
        }
    }

    pub fn add(&mut self, path: PathBuf) {
        self.rtx.push(path);
    }

    pub fn to_vec(&self) -> Vec<PathBuf> {
        let mut paths = self.pre.iter().chain(self.rtx.iter()).collect_vec();
        if self.seen_shims {
            paths.push(&dirs::SHIMS);
        }
        paths.into_iter().chain(self.post.iter()).cloned().collect()
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
        let mut path_env = Self::new();

        for path in paths {
            if path_env.seen_shims {
                path_env.post.push(path);
            } else if path == *dirs::SHIMS {
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
    use super::*;

    #[test]
    fn test_path_env() {
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
    fn test_path_env_no_rtx() {
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
}
