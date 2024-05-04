use std::fs::create_dir_all;
use std::path::PathBuf;

use duct::Expression;
use eyre::{eyre, Result, WrapErr};

use crate::cmd;
use crate::file::touch_dir;

pub struct Git {
    pub dir: PathBuf,
}

macro_rules! git_cmd {
    ( $dir:expr $(, $arg:expr )* $(,)? ) => {
        {
            let safe = format!("safe.directory={}", $dir.display());
            cmd!("git", "-C", $dir, "-c", safe $(, $arg)*)
        }
    }
}

macro_rules! git_cmd_read {
    ( $dir:expr $(, $arg:expr )* $(,)? ) => {
        {
            git_cmd!($dir $(, $arg)*).read().wrap_err_with(|| {
                let args = [$($arg,)*].join(" ");
                format!("git {args} failed")
            })
        }
    }
}

impl Git {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn is_repo(&self) -> bool {
        self.dir.join(".git").is_dir()
    }

    pub fn update(&self, gitref: Option<String>) -> Result<(String, String)> {
        let gitref = gitref.map_or_else(|| self.current_branch(), Ok)?;
        debug!("updating {} to {}", self.dir.display(), gitref);
        let exec = |cmd: Expression| match cmd.stderr_to_stdout().stdout_capture().unchecked().run()
        {
            Ok(res) => {
                if res.status.success() {
                    Ok(())
                } else {
                    Err(eyre!(
                        "git failed: {cmd:?} {}",
                        String::from_utf8(res.stdout).unwrap()
                    ))
                }
            }
            Err(err) => Err(eyre!("git failed: {cmd:?} {err:#}")),
        };
        exec(git_cmd!(
            &self.dir,
            "fetch",
            "--prune",
            "--update-head-ok",
            "origin",
            &format!("{}:{}", gitref, gitref),
        ))?;
        let prev_rev = self.current_sha()?;
        exec(git_cmd!(
            &self.dir,
            "-c",
            "advice.detachedHead=false",
            "-c",
            "advice.objectNameWarning=false",
            "checkout",
            "--force",
            &gitref
        ))?;
        let post_rev = self.current_sha()?;
        touch_dir(&self.dir)?;

        Ok((prev_rev, post_rev))
    }

    pub fn clone(&self, url: &str) -> Result<()> {
        debug!("cloning {} to {}", url, self.dir.display());
        if let Some(parent) = self.dir.parent() {
            create_dir_all(parent)?;
        }
        match get_git_version() {
            Ok(version) => trace!("git version: {}", version),
            Err(err) => warn!(
                "failed to get git version: {:#}\n Git is required to use mise.",
                err
            ),
        }
        cmd!("git", "clone", "-q", "--depth", "1", url, &self.dir).run()?;
        Ok(())
    }

    pub fn current_branch(&self) -> Result<String> {
        let branch = git_cmd_read!(&self.dir, "branch", "--show-current")?;
        debug!("current branch for {}: {}", self.dir.display(), &branch);
        Ok(branch)
    }
    pub fn current_sha(&self) -> Result<String> {
        let sha = git_cmd_read!(&self.dir, "rev-parse", "HEAD")?;
        debug!("current sha for {}: {}", self.dir.display(), &sha);
        Ok(sha)
    }

    pub fn current_sha_short(&self) -> Result<String> {
        let sha = git_cmd_read!(&self.dir, "rev-parse", "--short", "HEAD")?;
        debug!("current sha for {}: {}", self.dir.display(), &sha);
        Ok(sha)
    }

    pub fn current_abbrev_ref(&self) -> Result<String> {
        let aref = git_cmd_read!(&self.dir, "rev-parse", "--abbrev-ref", "HEAD")?;
        debug!("current abbrev ref for {}: {}", self.dir.display(), &aref);
        Ok(aref)
    }

    pub fn get_remote_url(&self) -> Option<String> {
        if !self.dir.exists() {
            return None;
        }
        let res = git_cmd_read!(&self.dir, "config", "--get", "remote.origin.url");
        match res {
            Ok(url) => {
                debug!("remote url for {}: {}", self.dir.display(), &url);
                Some(url)
            }
            Err(err) => {
                warn!(
                    "failed to get remote url for {}: {:#}",
                    self.dir.display(),
                    err
                );
                None
            }
        }
    }

    pub fn split_url_and_ref(url: &str) -> (String, Option<String>) {
        match url.split_once('#') {
            Some((url, _ref)) => (url.to_string(), Some(_ref.to_string())),
            None => (url.to_string(), None),
        }
    }
}

fn get_git_version() -> Result<String> {
    let version = cmd!("git", "--version").read()?;
    Ok(version.trim().into())
}

// #[cfg(test)]
// mod tests {
//     use tempfile::tempdir;
//
//     use super::*;
//
//     #[test]
//     fn test_clone_and_update() {
//         let dir = tempdir().unwrap().into_path();
//         let git = Git::new(dir);
//         git.clone("https://github.com/mise-plugins/rtx-tiny")
//             .unwrap();
//         let prev_rev = "c85ab2bea15e8b785592ce1a75db341e38ac4d33".to_string();
//         let latest = git.current_sha().unwrap();
//         let update_result = git.update(Some(prev_rev.clone())).unwrap();
//         assert_eq!(update_result, (latest.to_string(), prev_rev.to_string()));
//         assert_str_eq!(git.current_sha_short().unwrap(), "c85ab2b");
//         let update_result = git.update(None).unwrap();
//         assert_eq!(update_result, (prev_rev, latest));
//     }
// }
