use std::collections::{BTreeSet, HashMap};
use std::ffi::OsStr;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use duct::Expression;
use eyre::{Result, WrapErr, eyre};
use gix::{self};
use once_cell::sync::OnceCell;
use xx::file;

use crate::cmd::CmdLineRunner;
use crate::file::touch_dir;
use crate::progress::SingleReport;
use mise_settings::Settings;

use std::ffi::OsString;

pub struct Git {
    pub dir: PathBuf,
    pub repo: OnceCell<gix::Repository>,
}

macro_rules! git_cmd {
    ( $dir:expr $(, $arg:expr )* $(,)? ) => {
        {
            let safe = format!("safe.directory={}", $dir.display());
            sanitize_git_env(cmd!("git", "-c", $crate::git::github_credential_config("github.com"), "-c", $crate::git::github_credential_config("github.com:443"), "-C", $dir, "-c", safe, "-c", "core.autocrlf=false" $(, $arg)*))
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

    /// The remote to fetch from. Clones made with `clone.defaultRemoteName`
    /// set (or by gix honoring it) have no `origin`, so fall back to the only
    /// remote the repo has.
    fn remote_name(&self) -> String {
        // Read through gix first so a lookup does not spawn a process.
        let remotes = match self.repo() {
            Ok(repo) => repo
                .remote_names()
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            Err(_) => git_cmd_read!(&self.dir, "remote").unwrap_or_default(),
        };
        pick_remote_name(&remotes)
    }

    pub fn is_repo(&self) -> bool {
        self.dir.join(".git").is_dir()
    }

    pub fn update(&self, gitref: Option<String>) -> Result<(String, String)> {
        match gitref {
            Some(gitref) => {
                let remote_ref_kind = self.remote_ref_kind(&gitref)?;
                if remote_ref_kind.is_none() && looks_like_abbreviated_sha(&gitref) {
                    return Err(eyre!(
                        "{gitref} is not a branch or tag on the remote, and abbreviated commit SHAs are not supported; use the full commit SHA"
                    ));
                }
                self.update_ref(gitref, remote_ref_kind)
            }
            None => self.update_ref(self.current_branch()?, None),
        }
    }

    pub fn update_tag(&self, gitref: String) -> Result<(String, String)> {
        self.update_ref(gitref, Some(RemoteRefKind::Tag))
    }

    fn remote_ref_kind(&self, gitref: &str) -> Result<Option<RemoteRefKind>> {
        if gitref.starts_with("refs/") || looks_like_sha(gitref) {
            return Ok(None);
        }

        let branch_ref = format!("refs/heads/{gitref}");
        let tag_ref = format!("refs/tags/{gitref}");
        let output = git_cmd_read!(
            &self.dir,
            "ls-remote",
            "--refs",
            &self.remote_name(),
            &branch_ref,
            &tag_ref
        )?;

        // Match git's usual disambiguation: a same-named branch wins over a
        // tag, while callers can select the tag with `refs/tags/<name>`.
        Ok(remote_ref_kind(&output, &branch_ref, &tag_ref))
    }

    /// Detached `git checkout --force <ref>` with no fetch. Used after `clone`
    /// when the caller asked to land on a specific SHA — the clone has already
    /// pulled all reachable objects, so a fetch with a `<sha>:<sha>` refspec
    /// would be redundant (and on servers without
    /// `uploadpack.allowReachableSHA1InWant`, would fail).
    fn checkout(&self, gitref: &str) -> Result<()> {
        let cmd = git_cmd!(
            &self.dir,
            "-c",
            "advice.detachedHead=false",
            "-c",
            "advice.objectNameWarning=false",
            "checkout",
            "--force",
            gitref,
        );
        let res = cmd
            .stderr_to_stdout()
            .stdout_capture()
            .unchecked()
            .run()
            .map_err(|err| eyre!("git failed: {cmd:?} {err:#}"))?;
        if !res.status.success() {
            return Err(eyre!(
                "git failed: {cmd:?} {}",
                String::from_utf8_lossy(&res.stdout)
            ));
        }
        touch_dir(&self.dir)?;
        Ok(())
    }

    fn update_ref(
        &self,
        gitref: String,
        remote_ref_kind: Option<RemoteRefKind>,
    ) -> Result<(String, String)> {
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

        let qualified_ref = remote_ref_kind.map(|kind| qualify_remote_ref(&gitref, kind));
        let refspec = qualified_ref
            .as_ref()
            .map_or_else(|| format!("{gitref}:{gitref}"), |r| format!("{r}:{r}"));
        exec(git_cmd!(
            &self.dir,
            "fetch",
            "--prune",
            "--update-head-ok",
            &self.remote_name(),
            &refspec
        ))?;
        let prev_rev = self.current_sha()?;
        let checkout_ref = match (remote_ref_kind, qualified_ref.as_deref()) {
            (Some(RemoteRefKind::Tag), Some(tag_ref)) => tag_ref,
            _ => &gitref,
        };
        exec(git_cmd!(
            &self.dir,
            "-c",
            "advice.detachedHead=false",
            "-c",
            "advice.objectNameWarning=false",
            "checkout",
            "--force",
            &checkout_ref
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
        // check out the SHA explicitly. gix in particular panics
        // ("we map by name only and have no object-id in refspec") if a SHA
        // is fed to `with_ref_name`.
        let sha_branch = options.branch.as_deref().filter(|b| looks_like_sha(b));
        let revision = options.revision.as_deref().or(sha_branch);
        let named_branch = options
            .branch
            .as_deref()
            .filter(|b| !looks_like_sha(b) && revision.is_none());
        if (Settings::get().libgit2 || Settings::get().gix)
            && std::env::var_os("MISE_GITHUB_RELAY_SOCKET").is_none()
        {
            debug!("cloning {} to {} with gix", url, self.dir.display());
            let mut prepare_clone = gix::prepare_clone(url, &self.dir)?
                .with_in_memory_config_overrides([
                    github_credential_config("github.com"),
                    github_credential_config("github.com:443"),
                ]);

            if let Some(branch) = named_branch {
                prepare_clone = prepare_clone.with_ref_name(Some(branch))?;
            }

            let (mut prepare_checkout, _) = prepare_clone
                .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            prepare_checkout
                .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)?;

            if let Some(revision) = revision {
                self.checkout(revision)?;
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

        let mut cmd = sanitize_git_cmd_runner(
            CmdLineRunner::new("git")
                .arg("-c")
                .arg(github_credential_config("github.com"))
                .arg("-c")
                .arg(github_credential_config("github.com:443"))
                .arg("clone")
                .arg("-q")
                .arg("-o")
                .arg("origin")
                .arg("-c")
                .arg("core.autocrlf=false"),
        );
        // `--depth 1` is incompatible with checking out an arbitrary revision
        // later, so do a full clone when the caller supplied one.
        if revision.is_none() {
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

        if let Some(revision) = revision {
            self.checkout(revision)?;
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
        debug!("current branch for {}: {}", self.dir.display(), branch);
        Ok(branch)
    }
    pub fn current_sha(&self) -> Result<String> {
        let dir = &self.dir;
        if let Ok(repo) = self.repo() {
            let head = repo.head()?;
            let sha = head
                .id()
                .ok_or_else(|| eyre::eyre!("repository {} has no commit at HEAD", dir.display()))?
                .to_string();
            debug!("current sha for {dir:?}: {sha}");
            return Ok(sha);
        }
        let sha = git_cmd_read!(&self.dir, "rev-parse", "HEAD")?;
        debug!("current sha for {}: {}", self.dir.display(), sha);
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
        debug!("current abbrev ref for {}: {}", self.dir.display(), aref);
        Ok(aref)
    }

    pub fn get_remote_url(&self) -> Option<String> {
        let dir = &self.dir;
        if !self.exists() {
            return None;
        }
        let remote_name = self.remote_name();
        if let Ok(repo) = self.repo()
            && let Ok(remote) = repo.find_remote(remote_name.as_str())
            && let Some(url) = remote.url(gix::remote::Direction::Fetch)
        {
            trace!("remote url for {dir:?}: {url}");
            return Some(url.to_string());
        }
        let res = git_cmd_read!(
            &self.dir,
            "config",
            "--get",
            &format!("remote.{remote_name}.url")
        );
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

    /// Resolves `rev` to a commit SHA using only local objects and refs.
    /// Returns `None` when the repository has no such commit.
    pub fn resolve_commit(&self, rev: &str) -> Result<Option<String>> {
        validate_revision("revision", rev)?;
        let spec = format!("{rev}^{{commit}}");
        let output = git_cmd!(&self.dir, "rev-parse", "--verify", "--quiet", &spec)
            .stdout_capture()
            .stderr_null()
            .unchecked()
            .run()
            .wrap_err_with(|| format!("git rev-parse {spec} failed"))?;
        if !output.status.success() {
            return Ok(None);
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok((!sha.is_empty()).then_some(sha))
    }

    pub fn remote_sha(&self, branch: &str) -> Result<Option<String>> {
        let output = git_cmd_read!(&self.dir, "ls-remote", &self.remote_name(), branch)?;
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

    /// Returns paths changed between the merge base of two revisions.
    ///
    /// Rename detection is disabled so moves report both the old and new path.
    /// Paths are relative to `self.dir`, and changes outside it are excluded.
    pub fn changed_paths(&self, base: &str, head: &str) -> Result<BTreeSet<PathBuf>> {
        validate_revisions(base, head)?;
        let range = format!("{base}...{head}");
        let output = git_cmd!(
            &self.dir,
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--relative",
            &range,
            "--",
            "."
        )
        .stdout_capture()
        .run()
        .wrap_err_with(|| format!("git diff for {range} failed"))?;
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(path_from_git_bytes)
            .collect()
    }

    /// Returns the merge base used by a triple-dot comparison.
    pub fn merge_base(&self, base: &str, head: &str) -> Result<String> {
        validate_revisions(base, head)?;
        Ok(git_cmd_read!(&self.dir, "merge-base", "--", base, head)?
            .trim()
            .to_string())
    }

    /// Reads a UTF-8 file at a Git revision, returning `None` when it does not exist there.
    pub fn file_at_revision(&self, revision: &str, path: &Path) -> Result<Option<String>> {
        validate_revision("revision", revision)?;
        let Some(path) = path.to_str() else {
            return Ok(None);
        };
        let object = format!("{revision}:{}", path.replace('\\', "/"));
        let output = git_cmd!(&self.dir, "show", "--no-textconv", &object)
            .stdout_capture()
            .stderr_capture()
            .unchecked()
            .run()
            .wrap_err_with(|| format!("git show for {object:?} failed"))?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(String::from_utf8(output.stdout).wrap_err_with(
            || format!("Git file {path:?} at {revision:?} is not UTF-8"),
        )?))
    }

    pub fn get_path<P: AsRef<Path>>(path: P) -> eyre::Result<PathBuf> {
        let root = Self::get_root()?;
        let path = cmd!("git", "-C", &root, "rev-parse", "--git-path", path.as_ref()).read()?;
        let path = PathBuf::from(path.trim());
        Ok(if path.is_absolute() {
            path
        } else {
            root.join(path)
        })
    }
}

fn validate_revisions(base: &str, head: &str) -> Result<()> {
    validate_revision("base", base)?;
    validate_revision("head", head)
}

fn validate_revision(name: &str, revision: &str) -> Result<()> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err(eyre!("invalid Git {name} revision {revision:?}"));
    }
    Ok(())
}

fn path_from_git_bytes(path: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(OsString::from_vec(path.to_vec()).into())
    }
    #[cfg(not(unix))]
    {
        Ok(String::from_utf8(path.to_vec())
            .wrap_err("Git returned a non-UTF-8 path")?
            .into())
    }
}

/// Command-local configuration: mise does not write credentials or the helper
/// into repository configuration. Existing Git helpers retain their own policy.
/// Git invokes the helper only when authentication is needed. The helper also
/// validates the protocol and host before resolving any credentials.
pub fn github_credential_config(host: &str) -> String {
    let executable = crate::env::MISE_BIN.to_string_lossy();
    #[cfg(windows)]
    let executable = std::borrow::Cow::Owned(executable.replace('\\', "/"));
    let executable = shell_escape::unix::escape(executable);
    format!("credential.https://{host}.helper=!{executable} token github --git-credential")
}

fn get_git_version() -> Result<String> {
    let version = cmd!("git", "--version").read()?;
    Ok(version.trim().into())
}

fn sanitize_git_env(cmd: Expression) -> Expression {
    GIT_CONTEXT_ENV
        .iter()
        .fold(cmd, |cmd, env| cmd.env_remove(env))
}

fn sanitize_git_cmd_runner<'a>(cmd: CmdLineRunner<'a>) -> CmdLineRunner<'a> {
    GIT_CONTEXT_ENV
        .iter()
        .fold(cmd, |cmd, env| cmd.env_remove(env))
}

pub fn sanitize_git_command(cmd: &mut std::process::Command) {
    for env in GIT_CONTEXT_ENV {
        cmd.env_remove(env);
    }
}

const GIT_CONTEXT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRefKind {
    Branch,
    Tag,
}

fn pick_remote_name(remotes: &str) -> String {
    let mut names = remotes.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = names.next();
    if first == Some("origin") || names.any(|n| n == "origin") {
        return "origin".to_string();
    }
    first.unwrap_or("origin").to_string()
}

fn qualify_remote_ref(gitref: &str, kind: RemoteRefKind) -> String {
    let prefix = match kind {
        RemoteRefKind::Branch => "refs/heads/",
        RemoteRefKind::Tag => "refs/tags/",
    };
    if gitref.starts_with(prefix) {
        gitref.to_string()
    } else {
        format!("{prefix}{gitref}")
    }
}

fn remote_ref_kind(output: &str, branch_ref: &str, tag_ref: &str) -> Option<RemoteRefKind> {
    let has_ref = |expected: &str| {
        output.lines().any(|line| {
            line.split_once(char::is_whitespace)
                .is_some_and(|(_, name)| name == expected)
        })
    };

    if has_ref(branch_ref) {
        Some(RemoteRefKind::Branch)
    } else if has_ref(tag_ref) {
        Some(RemoteRefKind::Tag)
    } else {
        None
    }
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

/// Whether `s` looks like a commit SHA shortened below its full length.
///
/// Only checked after the remote reports no branch or tag by that name, so a
/// hex-only branch name such as `cafe123` still resolves as a branch.
fn looks_like_abbreviated_sha(s: &str) -> bool {
    (7..40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// If `path` is inside a linked git worktree, returns the equivalent path in
/// the repository's main checkout, e.g. `/repo-wt/sub/mise.toml` →
/// `/repo/sub/mise.toml`. Returns None for paths in a main checkout, outside
/// any git repository, or in worktrees of a bare repository.
///
/// Detection is filesystem-only (no git subprocess): a linked worktree root
/// contains a `.git` *file* pointing at `<main>/.git/worktrees/<name>`.
pub fn main_checkout_equivalent(path: &Path) -> Option<PathBuf> {
    static CACHE: LazyLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> =
        LazyLock::new(Default::default);
    for wt_root in path.ancestors() {
        let dotgit = wt_root.join(".git");
        if dotgit.is_dir() {
            // A `.git` directory is either the main checkout or a nested
            // independent repository. Either way stop: a nested repo's
            // contents are not derived from the outer repo's history, so its
            // configs must keep their own trust records rather than
            // inheriting the outer main checkout's via path mapping.
            return None;
        }
        if dotgit.is_file() {
            // `.git` files are used by both linked worktrees and submodules.
            // Only a linked worktree resolves to a main checkout here; for
            // anything else (submodule, bare-repo worktree) keep walking up —
            // a submodule may itself live inside a linked worktree.
            let main_root = CACHE
                .lock()
                .unwrap()
                .entry(wt_root.to_path_buf())
                .or_insert_with(|| main_checkout_root(&dotgit))
                .clone();
            if let Some(main_root) = main_root {
                let equiv = main_root.join(path.strip_prefix(wt_root).ok()?);
                return (equiv != path).then_some(equiv);
            }
        }
    }
    None
}

/// Where a path sits in a git repository: which checkout holds it, and which
/// repository that checkout belongs to.
///
/// Both are needed to name a working copy. The repository identifies the
/// project across all of its checkouts, and the linked worktree, when there is
/// one, distinguishes this copy from the others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checkout {
    /// The checkout that names the project: an ordinary checkout's own
    /// directory, or, for a linked worktree, the main checkout every worktree
    /// of the repository shares. None outside any git repository.
    ///
    /// A worktree of a bare repository names itself here, because a bare
    /// repository has no working tree to stand for the project and the
    /// directory holding it usually holds unrelated ones too. Pitchfork
    /// resolves it the same way, and a hostname the two disagreed about would
    /// be exported by mise and routed by nothing.
    pub repository: Option<PathBuf>,
    /// This linked worktree's own directory, when `path` is inside one.
    pub worktree: Option<PathBuf>,
}

/// Resolve `path`'s checkout without running git.
pub fn checkout_of(path: &Path) -> Checkout {
    for dir in path.ancestors() {
        let dotgit = dir.join(".git");
        if dotgit.is_dir() {
            // The main checkout, or a nested independent repository; either way
            // the search stops rather than consulting an outer repository.
            return Checkout {
                repository: Some(dir.to_path_buf()),
                worktree: None,
            };
        }
        if !dotgit.is_file() {
            continue;
        }
        if worktree_gitdir(&dotgit).is_some() {
            // Without a main checkout there is nothing above this worktree to
            // name the project, so it stands on its own: one label, and no
            // worktree component to distinguish it from siblings it has none of.
            return match main_checkout_root(&dotgit) {
                Some(repository) => Checkout {
                    repository: Some(repository),
                    worktree: Some(dir.to_path_buf()),
                },
                None => Checkout {
                    repository: Some(dir.to_path_buf()),
                    worktree: None,
                },
            };
        }
        // A submodule belongs to whatever checkout contains it, so keep
        // walking: the same submodule in two worktrees is two working copies.
        // Any other `.git` file is an independent repository and ends the walk
        // exactly as a `.git` directory does.
        if !is_submodule_gitdir(&dotgit) {
            return Checkout {
                repository: Some(dir.to_path_buf()),
                worktree: None,
            };
        }
    }
    Checkout::default()
}

/// Whether `path` sits inside a linked git worktree.
///
/// Unlike [`main_checkout_equivalent`] this also accepts worktrees of a bare
/// repository. They have no main checkout to map onto, but they are still
/// distinct checkouts, which is what matters to a caller allocating a resource
/// per working copy rather than mapping a path.
pub fn in_linked_worktree(path: &Path) -> bool {
    for wt_root in path.ancestors() {
        let dotgit = wt_root.join(".git");
        if dotgit.is_dir() {
            // Main checkout or a nested independent repository; either way the
            // search stops rather than consulting an outer repository.
            return false;
        }
        if dotgit.is_file() {
            if worktree_gitdir(&dotgit).is_some() {
                return true;
            }
            // A submodule belongs to whatever checkout contains it, and the
            // same submodule in two worktrees is two working copies, so keep
            // walking. Any other `.git` file is an independent repository, so
            // stop exactly as a nested `.git` directory does.
            if !is_submodule_gitdir(&dotgit) {
                return false;
            }
        }
    }
    false
}

/// The private git dir of a linked worktree, or None when this `.git` file
/// belongs to a submodule or a `--separate-git-dir` clone.
///
/// Linked worktrees keep their private git dir at `<common>/worktrees/<name>`.
/// Submodule git dirs live under `modules/`, so the parent directory's name is
/// what distinguishes them; a path that merely contains a `worktrees` component
/// elsewhere is not a worktree.
fn worktree_gitdir(dotgit_file: &Path) -> Option<PathBuf> {
    let gitdir = read_gitdir(dotgit_file)?;
    if gitdir.parent()?.file_name() != Some(OsStr::new("worktrees")) {
        return None;
    }
    // The private dir must resolve a real `commondir`, so neither a stale
    // `gitdir:` nor a hand-made directory that merely sits under one named
    // `worktrees` is mistaken for a checkout.
    worktree_common_dir(&gitdir).is_some().then_some(gitdir)
}

/// The shared git dir a worktree's private dir points at, or None when
/// `commondir` is missing or does not name an existing directory.
///
/// For a worktree of an ordinary repository this is `<main>/.git`; for one of a
/// bare repository it is the bare repo itself.
fn worktree_common_dir(gitdir: &Path) -> Option<PathBuf> {
    let common = PathBuf::from(
        std::fs::read_to_string(gitdir.join("commondir"))
            .ok()?
            .trim(),
    );
    let common = if common.is_relative() {
        gitdir.join(common)
    } else {
        common
    };
    let common = common.canonicalize().ok()?;
    // git puts a worktree's private dir at `<common>/worktrees/<name>`, so the
    // pointer must lead back to the directory it sits under. Without this a
    // forged `commondir` could name an unrelated repository.
    if common != gitdir.parent()?.parent()?.canonicalize().ok()? {
        return None;
    }
    // `HEAD` marks a git common dir, present both in an ordinary `.git` and at
    // the top of a bare repository. Requiring it stops an arbitrary existing
    // directory from passing as the repository a worktree belongs to.
    common.join("HEAD").is_file().then_some(common)
}

/// Whether a `.git` file names a submodule's git dir.
///
/// git keeps a submodule's git dir at `<enclosing>/modules/<name>`, where the
/// enclosing dir is the superproject's git dir, or that superproject worktree's
/// private dir when the submodule sits inside a worktree. Requiring the path
/// before `modules` to be a git dir is what separates a real submodule from an
/// unrelated repository that merely has `modules` somewhere in its path.
fn is_submodule_gitdir(dotgit_file: &Path) -> bool {
    let Some(gitdir) = read_gitdir(dotgit_file) else {
        return false;
    };
    // A marker naming a git dir that is not there says nothing about what
    // encloses this directory, so it is not taken as a submodule.
    if !gitdir.is_dir() {
        return false;
    }
    // Every `modules` component is a candidate, not just the innermost. A
    // submodule whose path is itself `modules/foo` lands at
    // `.git/modules/modules/foo`, so stopping at the first match would inspect
    // the container `.git/modules` and wrongly conclude this is not a submodule.
    let mut dir = gitdir.as_path();
    while let Some(parent) = dir.parent() {
        if dir.file_name() == Some(OsStr::new("modules")) && parent.join("HEAD").is_file() {
            return true;
        }
        dir = parent;
    }
    false
}

/// The git dir a `.git` *file* names, resolved against that file's own
/// directory when the recorded path is relative.
fn read_gitdir(dotgit_file: &Path) -> Option<PathBuf> {
    let contents = std::fs::read_to_string(dotgit_file).ok()?;
    let gitdir = PathBuf::from(contents.strip_prefix("gitdir:")?.trim());
    Some(if gitdir.is_relative() {
        dotgit_file.parent()?.join(gitdir)
    } else {
        gitdir
    })
}

/// Whether this linked worktree's repository has other worktrees beside it.
///
/// Asked only of a checkout that names itself, where the hostname carries no
/// worktree component: a namespace such a checkout shares with a sibling then
/// resolves to one hostname for both, and each runs in its own process, so
/// neither load can see the other to report it. `false` for anything that is
/// not a linked worktree, which has no siblings by this definition.
pub fn has_sibling_worktrees(worktree_root: &Path) -> bool {
    let Some(gitdir) = worktree_gitdir(&worktree_root.join(".git")) else {
        return false;
    };
    // `worktree_gitdir` already established that this is `<common>/worktrees/
    // <name>`, so the parent holds one entry per worktree of the repository.
    let Some(registry) = gitdir.parent() else {
        return false;
    };
    std::fs::read_dir(registry)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            // Removing a worktree's directory leaves its entry here until
            // someone prunes it, and a checkout that no longer exists cannot
            // inherit a namespace. Git decides that by the `gitdir` pointer
            // still naming something, so this does too, rather than sending a
            // user off to change a namespace nothing else shares.
            let Ok(pointer) = std::fs::read_to_string(entry.path().join("gitdir")) else {
                return false;
            };
            // `worktree.useRelativePaths` writes the pointer relative to the
            // entry holding it, as `commondir` and a worktree's own `.git` are
            // written. Resolving it against the process directory instead
            // would make every live worktree of such a repository look pruned.
            let pointer = Path::new(pointer.trim());
            match pointer.is_relative() {
                true => entry.path().join(pointer).exists(),
                false => pointer.exists(),
            }
        })
        .nth(1)
        .is_some()
}

/// Resolves a linked worktree's `.git` file to the root of the main checkout.
///
/// Two questions share this answer: which working copy may share trust
/// records, and which checkout names the project for [`Checkout`]. Both refuse
/// a bare repository, having no working copy to point at.
fn main_checkout_root(dotgit_file: &Path) -> Option<PathBuf> {
    let common = worktree_common_dir(&worktree_gitdir(dotgit_file)?)?;
    if common.file_name() == Some(OsStr::new(".git")) {
        common.parent().map(|p| p.to_path_buf())
    } else {
        None // bare repository — no main checkout to share trust with
    }
}

/// A git binary mise can run unattended for its own plumbing, or None.
///
/// On macOS `/usr/bin/git` is a shim that opens the Xcode Command Line Tools
/// installer dialog when the tools are absent, so it only counts once
/// `xcode-select -p` confirms an installation. Any other git on PATH
/// (Homebrew, MacPorts) is taken as-is.
pub fn plumbing_binary() -> Option<&'static Path> {
    static BIN: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
        // spawnable as-is: on Windows that is `git.exe`, which a plain
        // lookup of `git` does not find
        let git = crate::file::which_spawnable("git")?;
        if cfg!(target_os = "macos") && git == Path::new("/usr/bin/git") {
            let installed = std::process::Command::new("xcode-select")
                .arg("-p")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !installed {
                return None;
            }
        }
        Some(git)
    });
    BIN.as_deref()
}

/// One invocation of git plumbing against a [`GitPlumbing`] repository.
#[derive(Debug, Default)]
pub struct PlumbingCall<'a> {
    pub args: Vec<OsString>,
    /// Adds `--work-tree=<path>`.
    pub work_tree: Option<&'a Path>,
    /// Sets `GIT_INDEX_FILE` so the call never touches the repository's own
    /// index. Applied after [`sanitize_git_command`], which strips it.
    pub index_file: Option<&'a Path>,
    pub cwd: Option<&'a Path>,
    pub stdin: Option<&'a [u8]>,
    /// Environment overrides for a local plumbing invocation only.
    pub env: Vec<(OsString, OsString)>,
}

impl<'a> PlumbingCall<'a> {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    pub fn work_tree(mut self, path: &'a Path) -> Self {
        self.work_tree = Some(path);
        self
    }

    pub fn index_file(mut self, path: &'a Path) -> Self {
        self.index_file = Some(path);
        self
    }

    pub fn stdin(mut self, bytes: &'a [u8]) -> Self {
        self.stdin = Some(bytes);
        self
    }

    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// Runs git plumbing against a repository mise owns (a bare "shadow" repo),
/// isolated from the user's git configuration.
///
/// Every call ignores the system and global gitconfig, so `filter.*`
/// (git-crypt, LFS), `core.hooksPath`, `core.excludesFile`, aliases, and
/// credential helpers from the user's setup cannot act on mise's repository,
/// and defaults to a fixed committer identity. Only plumbing commands should be run
/// through it; porcelain (`commit`, `checkout`) would consult hooks.
#[derive(Debug)]
pub struct GitPlumbing {
    git_dir: PathBuf,
    disabled_hooks: std::sync::Mutex<Option<tempfile::TempDir>>,
}

impl GitPlumbing {
    pub fn new(git_dir: impl Into<PathBuf>) -> Self {
        Self {
            git_dir: git_dir.into(),
            disabled_hooks: std::sync::Mutex::new(None),
        }
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Whether the repository has been initialised.
    pub fn exists(&self) -> bool {
        self.git_dir.join("HEAD").is_file()
    }

    /// Creates the bare repository if needed. Idempotent.
    pub fn init_bare(&self) -> Result<()> {
        if self.exists() {
            return Ok(());
        }
        if let Some(parent) = self.git_dir.parent() {
            crate::file::create_dir_all(parent)?;
        }
        let mut cmd = self.base_command()?;
        cmd.args(["init", "--bare", "--quiet"]).arg(&self.git_dir);
        run_plumbing(cmd, None)?;
        for (key, value) in [
            ("core.autocrlf", "false"),
            ("core.logAllRefUpdates", "false"),
            ("gc.auto", "0"),
            #[cfg(unix)]
            ("core.symlinks", "true"),
        ] {
            self.run(PlumbingCall::new(["config", key, value]))?;
        }
        Ok(())
    }

    /// Runs the call, failing on a non-zero exit with stderr in the error.
    pub fn run(&self, call: PlumbingCall<'_>) -> Result<()> {
        self.output(call).map(|_| ())
    }

    /// Runs the call and returns its stdout bytes.
    pub fn output(&self, call: PlumbingCall<'_>) -> Result<Vec<u8>> {
        let cmd = self.command(&call)?;
        run_plumbing(cmd, call.stdin)
    }

    /// Inspect a blob without buffering its full contents. Reap the reader
    /// after the prefix so large ordinary files need no encryption-size cap.
    pub fn blob_starts_with(&self, oid: &str, prefix: &[u8]) -> Result<bool> {
        use std::io::Read;
        use std::process::Stdio;
        let mut child = self
            .command(&PlumbingCall::new(["cat-file", "blob", oid]))?
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut bytes = Vec::new();
        // Keep the pipe open until Git has exited: dropping it before kill
        // can make a large blob emit a spurious broken-pipe error on stderr.
        let mut output = child
            .stdout
            .take()
            .expect("stdout was piped")
            .take(prefix.len() as u64);
        let read = output.read_to_end(&mut bytes);
        let complete = bytes.len() == prefix.len();
        if complete || read.is_err() {
            let _ = child.kill();
        }
        let status = child.wait()?;
        read?;
        if !complete && !status.success() {
            eyre::bail!("failed to inspect Git blob {oid}");
        }
        Ok(bytes == prefix)
    }

    /// Runs the call and returns its full output without treating a non-zero
    /// exit as an error, for commands whose status carries meaning.
    pub fn output_unchecked(&self, call: PlumbingCall<'_>) -> Result<std::process::Output> {
        let cmd = self.command(&call)?;
        spawn_plumbing(cmd, call.stdin)
    }

    /// Runs the call with stdout and stderr inherited, for output that goes
    /// straight to the terminal (a patch can be as large as a snapshot), and
    /// returns its status without treating a non-zero exit as an error.
    pub fn status_inherited(&self, call: PlumbingCall<'_>) -> Result<std::process::ExitStatus> {
        let mut cmd = self.command(&call)?;
        cmd.stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .stdin(std::process::Stdio::null());
        cmd.status()
            .wrap_err_with(|| format!("failed to run {}", describe_plumbing(&cmd)))
    }

    /// Runs the call and returns its trimmed stdout.
    pub fn output_str(&self, call: PlumbingCall<'_>) -> Result<String> {
        let out = self.output(call)?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }

    /// Runs a network command (`fetch`, `push`, `ls-remote`) against this
    /// repository with the user's normal git configuration: credential
    /// helpers, ssh settings, and URL rewrites apply, while hooks, filters,
    /// and aliases still cannot act on mise's repository because only
    /// plumbing runs here. Prompts are disabled when nobody is attending.
    pub fn network_output(&self, call: PlumbingCall<'_>) -> Result<std::process::Output> {
        eyre::ensure!(
            call.work_tree.is_none()
                && call.index_file.is_none()
                && call.stdin.is_none()
                && call.env.is_empty(),
            "network Git calls do not accept a work tree, alternate index, stdin, or environment overrides"
        );
        let git =
            plumbing_binary().ok_or_else(|| eyre!("no unattended git executable is available"))?;
        let mut cmd = std::process::Command::new(git);
        sanitize_git_command(&mut cmd);
        if !console::user_attended_stderr() {
            cmd.env("GIT_TERMINAL_PROMPT", "0");
        }
        cmd.args([
            "-c",
            &github_credential_config("github.com"),
            "-c",
            &github_credential_config("github.com:443"),
        ]);
        cmd.env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null());
        let mut git_dir = OsString::from("--git-dir=");
        git_dir.push(&self.git_dir);
        cmd.arg(git_dir);
        // A private empty directory is an unambiguous hooks path on every
        // platform, unlike Unix null-device spellings on native Windows.
        let hooks = tempfile::tempdir()?;
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(hooks.path());
        cmd.arg("-c")
            .arg(hooks_config)
            .args(["-c", "advice.fetchShowForcedUpdates=false"]);
        cmd.args(&call.args);
        if let Some(cwd) = call.cwd {
            cmd.current_dir(cwd);
        }
        spawn_plumbing(cmd, None)
    }

    fn base_command(&self) -> Result<std::process::Command> {
        let git =
            plumbing_binary().ok_or_else(|| eyre!("no unattended git executable is available"))?;
        let mut cmd = std::process::Command::new(git);
        sanitize_git_command(&mut cmd);
        // Internal object and ref operations must not inherit configuration
        // injected by the calling shell. Network commands intentionally keep
        // these variables for credential helpers and the GitHub relay.
        cmd.env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS");
        let mut hooks = self
            .disabled_hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if hooks.is_none() {
            *hooks = Some(tempfile::tempdir()?);
        }
        let mut hooks_config = OsString::from("core.hooksPath=");
        hooks_config.push(hooks.as_ref().expect("hooks directory initialized").path());
        cmd.arg("-c").arg(hooks_config);
        // Git for Windows maps the literal `/dev/null` to its null device;
        // its UCRT builds (arm64, and x64 since 2.56) reject `NUL` as a config path.
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_AUTHOR_NAME", "mise")
            .env("GIT_AUTHOR_EMAIL", "mise@localhost")
            .env("GIT_COMMITTER_NAME", "mise")
            .env("GIT_COMMITTER_EMAIL", "mise@localhost")
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null());
        Ok(cmd)
    }

    fn command(&self, call: &PlumbingCall<'_>) -> Result<std::process::Command> {
        let mut cmd = self.base_command()?;
        let mut git_dir = OsString::from("--git-dir=");
        git_dir.push(&self.git_dir);
        cmd.arg(git_dir);
        if let Some(work_tree) = call.work_tree {
            let mut arg = OsString::from("--work-tree=");
            arg.push(work_tree);
            cmd.arg(arg);
        }
        cmd.args([
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.quotePath=false",
            "-c",
            "advice.addEmbeddedRepo=false",
        ]);
        cmd.args(&call.args);
        if let Some(index) = call.index_file {
            cmd.env("GIT_INDEX_FILE", index);
        }
        if let Some(cwd) = call.cwd {
            cmd.current_dir(cwd);
        }
        for (key, value) in &call.env {
            cmd.env(key, value);
        }
        if call.stdin.is_some() {
            cmd.stdin(std::process::Stdio::piped());
        }
        Ok(cmd)
    }
}

fn describe_plumbing(cmd: &std::process::Command) -> String {
    let args = cmd
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    format!("git {}", args.join(" "))
}

fn spawn_plumbing(
    mut cmd: std::process::Command,
    stdin: Option<&[u8]>,
) -> Result<std::process::Output> {
    use std::io::Write;

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd
        .spawn()
        .wrap_err_with(|| format!("failed to spawn {}", describe_plumbing(&cmd)))?;
    if let Some(bytes) = stdin {
        let mut pipe = child.stdin.take().expect("stdin was piped");
        // git may exit before reading everything (a bad pathspec, say); that
        // shows up in its status, so a broken pipe here is not the error.
        let _ = pipe.write_all(bytes);
        drop(pipe);
    }
    child
        .wait_with_output()
        .wrap_err_with(|| format!("failed to run {}", describe_plumbing(&cmd)))
}

fn run_plumbing(cmd: std::process::Command, stdin: Option<&[u8]>) -> Result<Vec<u8>> {
    let describe = describe_plumbing(&cmd);
    let output = spawn_plumbing(cmd, stdin)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(eyre!(
            "{describe} failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }
    Ok(output.stdout)
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
    revision: Option<String>,
}

impl<'a> CloneOptions<'a> {
    pub fn pr(mut self, pr: &'a dyn SingleReport) -> Self {
        self.pr = Some(pr);
        self
    }

    pub fn branch(mut self, branch: &str) -> Self {
        self.branch = Some(branch.to_string());
        self.revision = None;
        self
    }

    /// Clone the complete repository and check out a revision afterwards.
    ///
    /// Unlike `branch`, this accepts abbreviated commit IDs and avoids passing
    /// them to `git clone -b` or gix's ref-name-only clone API.
    pub fn revision(mut self, revision: &str) -> Self {
        self.branch = None;
        self.revision = Some(revision.to_string());
        self
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod plumbing_tests;
