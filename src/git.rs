use std::path::PathBuf;

use color_eyre::eyre::Result;

use crate::cmd;
use crate::file::touch_dir;

pub struct Git {
    pub dir: PathBuf,
}

impl Git {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn remote_default_branch(&self) -> Result<String> {
        let branch = cmd!(
            "git",
            "-C",
            &self.dir,
            "symbolic-ref",
            "refs/remotes/origin/HEAD"
        )
        .read()?;

        let branch = branch.rsplit_once('/').unwrap().1;
        Ok(branch.to_string())
    }

    pub fn update(&self, gitref: Option<String>) -> Result<(String, String)> {
        let gitref = gitref.map_or_else(|| self.remote_default_branch(), Ok)?;
        debug!("updating {} to {}", self.dir.display(), gitref);
        cmd!(
            "git",
            "-C",
            &self.dir,
            "fetch",
            "--prune",
            "--update-head-ok",
            "origin",
            [gitref.as_str(), gitref.as_str()].join(":"),
        )
        .run()?;
        let prev_rev = self.current_sha()?;
        cmd!(
            "git",
            "-C",
            &self.dir,
            "-c",
            "advice.detachedHead=false",
            "-c",
            "advice.objectNameWarning=false",
            "checkout",
            "--force",
            gitref
        )
        .run()?;
        let post_rev = self.current_sha()?;
        touch_dir(&self.dir)?;

        Ok((prev_rev, post_rev))
    }

    pub fn clone(&self, url: &str) -> Result<()> {
        debug!("cloning {} to {}", url, self.dir.display());
        cmd!("git", "clone", "-q", "--depth", "1", url, &self.dir).run()?;
        Ok(())
    }

    pub fn current_sha(&self) -> Result<String> {
        let sha = cmd!("git", "-C", &self.dir, "rev-parse", "HEAD").read()?;
        debug!("current sha for {}: {}", self.dir.display(), &sha);
        Ok(sha)
    }

    pub fn get_remote_url(&self) -> Result<String> {
        let url = cmd!(
            "git",
            "-C",
            &self.dir,
            "config",
            "--get",
            "remote.origin.url"
        )
        .read()?;
        debug!("remote url for {}: {}", self.dir.display(), &url);
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_clone_and_update() {
        let dir = tempdir().unwrap().into_path();
        let git = Git::new(dir);
        git.clone("https://github.com/asdf-vm/asdf-plugins")
            .unwrap();
        let prev_rev = "f4b510d1d0c01ab2da95b80a1c1521f651cdd708".to_string();
        let latest = git.current_sha().unwrap();
        let update_result = git.update(Some(prev_rev.clone())).unwrap();
        assert_eq!(update_result, (latest.to_string(), prev_rev.to_string()));
        let update_result = git.update(None).unwrap();
        assert_eq!(update_result, (prev_rev, latest));
    }
}
