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
        // gix's `with_ref_name` and git CLI's `-b` only accept branch/tag names.
        // If the caller passed a commit SHA, clone without a ref and then
        // fetch + checkout the SHA explicitly. gix in particular panics
        // ("we map by name only and have no object-id in refspec") if a SHA
        // is fed to `with_ref_name`.
        let sha_branch = options
            .branch
            .as_deref()
            .filter(|b| looks_like_sha(b))
            .map(str::to_string);
        let named_branch = options.branch.as_deref().filter(|b| !looks_like_sha(b));
        if Settings::get().libgit2 || Settings::get().gix {
            debug!("cloning {} to {} with gix", url, self.dir.display());
            let mut prepare_clone = gix::prepare_clone(url, &self.dir)?;

            if let Some(branch) = named_branch {
                prepare_clone = prepare_clone.with_ref_name(Some(branch))?;
            }

            let (mut prepare_checkout, _) = prepare_clone
                .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            prepare_checkout
                .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            if let Some(sha) = sha_branch {
                self.update_ref(sha, false)?;
            }
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
            .arg("core.autocrlf=false");
        // `--depth 1` is incompatible with checking out an arbitrary SHA later,
        // so do a full clone when the caller passed a SHA.
        if sha_branch.is_none() {
            cmd = cmd.arg("--depth").arg("1");
        }
        cmd = cmd.arg(url).arg(&self.dir);

        if let Some(branch) = named_branch {
            cmd = cmd.args([
                "-b",
                branch,
                "--single-branch",
                "-c",
                "advice.detachedHead=false",
            ]);
        }

        cmd.execute()?;

        if let Some(sha) = sha_branch {
            self.update_ref(sha, false)?;
        }
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

/// Heuristic for whether a ref string is a commit SHA (full SHA-1 or SHA-256).
///
/// Branch and tag names that happen to be all-hex would also match, but git
/// disallows refs that are valid object IDs anyway (see `git check-ref-format`),
/// so the heuristic is safe in practice. Abbreviated SHAs are intentionally not
/// matched — they are ambiguous with short branch names and need server-side
/// resolution before they can be checked out.
fn looks_like_sha(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
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
    use super::{CloneOptions, Git, looks_like_sha};
    use std::process::Command;

    #[test]
    fn sha_detection() {
        assert!(looks_like_sha("0123456789abcdef0123456789abcdef01234567"));
        assert!(looks_like_sha(
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        ));
        assert!(!looks_like_sha("main"));
        assert!(!looks_like_sha("v1.2.3"));
        assert!(!looks_like_sha("abcdef1")); // short SHA not supported
        assert!(!looks_like_sha(""));
        assert!(!looks_like_sha("g123456789abcdef0123456789abcdef01234567")); // non-hex
    }

    /// Regression test for https://github.com/jdx/mise/discussions/9472:
    /// gix's `with_ref_name` panics ("we map by name only and have no
    /// object-id in refspec") when given a commit SHA. Our `clone()` must
    /// detect that case and fall back to a plain clone + checkout.
    #[test]
    fn clone_by_sha_does_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();

        let git_in_src = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(&src)
                .output()
                .expect("spawn git");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            out
        };
        git_in_src(&["-c", "init.defaultBranch=main", "init", "-q"]);
        git_in_src(&[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "first",
        ]);
        let sha = String::from_utf8(git_in_src(&["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_string();
        assert_eq!(sha.len(), 40);

        let url = format!("file://{}", src.display());
        Git::new(&dst)
            .clone(&url, CloneOptions::default().branch(&sha))
            .expect("clone with SHA must not panic and must succeed");

        let head = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&dst)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8(head.stdout).unwrap().trim(),
            sha,
            "worktree should be checked out at the requested SHA"
        );
    }
}
