use std::fmt::Debug;
use std::path::{Path, PathBuf};

use duct::Expression;
use eyre::{Result, WrapErr, eyre};
use gix::{self};
use once_cell::sync::OnceCell;
use xx::file;

use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::file::touch_dir;
use crate::ui::progress_report::SingleReport;

pub struct Git {
    pub dir: PathBuf,
    pub repo: OnceCell<gix::Repository>,
}

macro_rules! git_cmd {
    ( $dir:expr $(, $arg:expr )* $(,)? ) => {
        {
            let safe = format!("safe.directory={}", $dir.display());
            cmd!("git", "-C", $dir, "-c", safe, "-c", "core.autocrlf=false" $(, $arg)*)
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
    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            repo: OnceCell::new(),
        }
    }

    pub fn repo(&self) -> Result<&gix::Repository> {
        self.repo.get_or_try_init(|| {
            trace!("opening git repository via gix at {:?}", self.dir);
            gix::open(&self.dir)
                .wrap_err_with(|| format!("failed to open git repository at {:?}", self.dir))
                .inspect_err(|err| warn!("{err:#}"))
        })
    }

    pub fn is_repo(&self) -> bool {
        self.dir.join(".git").is_dir()
    }

    pub fn update(&self, gitref: Option<String>) -> Result<(String, String)> {
        let gitref = gitref.map_or_else(|| self.current_branch(), Ok)?;
        self.update_ref(gitref, false)
    }

    pub fn update_tag(&self, gitref: String) -> Result<(String, String)> {
        self.update_ref(gitref, true)
    }

    fn update_ref(&self, gitref: String, is_tag_ref: bool) -> Result<(String, String)> {
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
        debug!("updating {} to {} with git", self.dir.display(), gitref);

        let refspec = if is_tag_ref {
            format!("refs/tags/{gitref}:refs/tags/{gitref}")
        } else {
            format!("{gitref}:{gitref}")
        };
        exec(git_cmd!(
            &self.dir,
            "fetch",
            "--prune",
            "--update-head-ok",
            "origin",
            &refspec
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

    pub fn clone(&self, url: &str, options: CloneOptions) -> Result<()> {
        if let Some(parent) = self.dir.parent() {
            file::mkdirp(parent)?;
        }

        // gix's with_ref_name panics on SHA hashes; use init+fetch+checkout instead
        if let Some(sha) = options.branch.as_deref().filter(|b| looks_like_git_sha(b)) {
            debug!(
                "cloning {} to {} at SHA {} via init+fetch",
                url,
                self.dir.display(),
                sha
            );
            if let Some(pr) = &options.pr {
                pr.abandon();
            }
            CmdLineRunner::new("git")
                .args(["-c", "core.autocrlf=false", "init", "-q"])
                .arg(&self.dir)
                .execute()?;
            CmdLineRunner::new("git")
                .args(["-C"])
                .arg(&self.dir)
                .args(["remote", "add", "origin", url])
                .execute()?;
            CmdLineRunner::new("git")
                .args(["-C"])
                .arg(&self.dir)
                .args(["fetch", "--depth", "1", "origin", sha])
                .execute()
                .wrap_err_with(|| {
                    format!(
                        "failed to fetch SHA {sha} from {url}; \
                     self-hosted servers may need `uploadpack.allowReachableSHA1InWant=true`"
                    )
                })?;
            CmdLineRunner::new("git")
                .args(["-C"])
                .arg(&self.dir)
                .args(["-c", "advice.detachedHead=false", "checkout", "FETCH_HEAD"])
                .execute()?;
            return Ok(());
        }

        if Settings::get().libgit2 || Settings::get().gix {
            debug!("cloning {} to {} with gix", url, self.dir.display());
            let mut prepare_clone = gix::prepare_clone(url, &self.dir)?;

            if let Some(branch) = &options.branch {
                prepare_clone = prepare_clone.with_ref_name(Some(branch))?;
            }

            let (mut prepare_checkout, _) = prepare_clone
                .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            prepare_checkout
                .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            return Ok(());
        }
        debug!("cloning {} to {} with git", url, self.dir.display());
        match get_git_version() {
            Ok(version) => trace!("git version: {}", version),
            Err(err) => warn!(
                "failed to get git version: {:#}\n Git is required to use mise.",
                err
            ),
        }
        if let Some(pr) = &options.pr {
            // in order to prevent hiding potential password prompt, just disable the progress bar
            pr.abandon();
        }

        let mut cmd = CmdLineRunner::new("git")
            .arg("clone")
            .arg("-q")
            .arg("-o")
            .arg("origin")
            .arg("-c")
            .arg("core.autocrlf=false")
            .arg("--depth")
            .arg("1")
            .arg(url)
            .arg(&self.dir);

        if let Some(branch) = &options.branch {
            cmd = cmd.args([
                "-b",
                branch,
                "--single-branch",
                "-c",
                "advice.detachedHead=false",
            ]);
        }

        cmd.execute()?;
        Ok(())
    }

    pub fn update_submodules(&self) -> Result<()> {
        debug!("updating submodules in {}", self.dir.display());

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

        exec(
            git_cmd!(&self.dir, "submodule", "update", "--init", "--recursive")
                .env("GIT_TERMINAL_PROMPT", "0"),
        )?;

        Ok(())
    }

    pub fn current_branch(&self) -> Result<String> {
        let dir = &self.dir;
        if let Ok(repo) = self.repo() {
            let head = repo.head()?;
            let branch = head
                .referent_name()
                .map(|name| name.shorten().to_string())
                .unwrap_or_else(|| head.id().unwrap().to_string());
            debug!("current branch for {dir:?}: {branch}");
            return Ok(branch);
        }
        let branch = git_cmd_read!(&self.dir, "branch", "--show-current")?;
        debug!("current branch for {}: {}", self.dir.display(), &branch);
        Ok(branch)
    }
    pub fn current_sha(&self) -> Result<String> {
        let dir = &self.dir;
        if let Ok(repo) = self.repo() {
            let head = repo.head()?;
            let id = head.id();
            let sha = id.unwrap().to_string();
            debug!("current sha for {dir:?}: {sha}");
            return Ok(sha);
        }
        let sha = git_cmd_read!(&self.dir, "rev-parse", "HEAD")?;
        debug!("current sha for {}: {}", self.dir.display(), &sha);
        Ok(sha)
    }

    pub fn current_sha_short(&self) -> Result<String> {
        let dir = &self.dir;
        if let Ok(repo) = self.repo() {
            let head = repo.head()?;
            let id = head.id();
            let sha = id.unwrap().to_string()[..7].to_string();
            debug!("current sha for {dir:?}: {sha}");
            return Ok(sha);
        }
        let sha = git_cmd_read!(&self.dir, "rev-parse", "--short", "HEAD")?;
        debug!("current sha for {dir:?}: {sha}");
        Ok(sha)
    }

    pub fn current_abbrev_ref(&self) -> Result<String> {
        let dir = &self.dir;
        if let Ok(repo) = self.repo() {
            let head = repo.head()?;
            let head = head.name().shorten().to_string();
            debug!("current abbrev ref for {dir:?}: {head}");
            return Ok(head);
        }
        let aref = git_cmd_read!(&self.dir, "rev-parse", "--abbrev-ref", "HEAD")?;
        debug!("current abbrev ref for {}: {}", self.dir.display(), &aref);
        Ok(aref)
    }

    pub fn get_remote_url(&self) -> Option<String> {
        let dir = &self.dir;
        if !self.exists() {
            return None;
        }
        if let Ok(repo) = self.repo()
            && let Ok(remote) = repo.find_remote("origin")
            && let Some(url) = remote.url(gix::remote::Direction::Fetch)
        {
            trace!("remote url for {dir:?}: {url}");
            return Some(url.to_string());
        }
        let res = git_cmd_read!(&self.dir, "config", "--get", "remote.origin.url");
        match res {
            Ok(url) => {
                debug!("remote url for {dir:?}: {url}");
                Some(url)
            }
            Err(err) => {
                warn!("failed to get remote url for {dir:?}: {err:#}");
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

    pub fn remote_sha(&self, branch: &str) -> Result<Option<String>> {
        let output = git_cmd_read!(&self.dir, "ls-remote", "origin", branch)?;
        Ok(output
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().next())
            .map(|sha| sha.to_string()))
    }

    pub fn exists(&self) -> bool {
        self.dir.join(".git").is_dir()
    }

    pub fn get_root() -> eyre::Result<PathBuf> {
        Ok(cmd!("git", "rev-parse", "--show-toplevel")
            .read()?
            .trim()
            .into())
    }
}

fn get_git_version() -> Result<String> {
    let version = cmd!("git", "--version").read()?;
    Ok(version.trim().into())
}

fn looks_like_git_sha(s: &str) -> bool {
    // SHA-1: 7–40 hex chars; SHA-256 (new-format git): exactly 64 hex chars
    ((s.len() >= 7 && s.len() <= 40) || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Debug for Git {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Git").field("dir", &self.dir).finish()
    }
}

#[derive(Default)]
pub struct CloneOptions<'a> {
    pr: Option<&'a dyn SingleReport>,
    branch: Option<String>,
}

impl<'a> CloneOptions<'a> {
    pub fn pr(mut self, pr: &'a dyn SingleReport) -> Self {
        self.pr = Some(pr);
        self
    }

    pub fn branch(mut self, branch: &str) -> Self {
        self.branch = Some(branch.to_string());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_git_sha() {
        assert!(looks_like_git_sha("abc1234")); // 7 chars — minimum short SHA
        assert!(looks_like_git_sha(
            "abc1234def5678901234567890123456789012ab"
        )); // 40 chars — full SHA
        assert!(looks_like_git_sha("deadbeef1234567")); // mid-length
        assert!(!looks_like_git_sha("abc123")); // 6 chars — too short
        assert!(!looks_like_git_sha(
            "abc1234def5678901234567890123456789012abc"
        )); // 41 chars — too long
        assert!(!looks_like_git_sha("abc1234g")); // non-hex char
        assert!(!looks_like_git_sha("main")); // branch name
        assert!(!looks_like_git_sha("refs/heads/main")); // full ref
        assert!(!looks_like_git_sha("v1.0.0")); // tag
        assert!(!looks_like_git_sha("")); // empty
        // SHA-256 (64 hex chars) — used by git's new object format
        assert!(looks_like_git_sha("a".repeat(64).as_str()));
        assert!(!looks_like_git_sha("a".repeat(63).as_str())); // 63 chars — not a valid SHA-256
        assert!(!looks_like_git_sha("a".repeat(65).as_str())); // 65 chars — too long
    }
}
