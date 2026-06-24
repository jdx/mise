//! Git-backed repo checkouts for `[bootstrap.repos]`.
//!
//! Entries are keyed by target path:
//!
//! ```toml
//! [bootstrap.repos]
//! "~/src/dotfiles" = { url = "git@github.com:jdx/dotfiles.git", ref = "main" }
//! ```
//!
//! Repos are applied only during explicit bootstrap commands. Existing repos
//! are updated only when the worktree is clean and the configured origin
//! matches.

use std::path::{Path, PathBuf};
use std::process::Command;

use eyre::{Result, bail, eyre};
use serde::Deserialize;

use crate::file;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct RepoTomlConfig {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, rename = "ref")]
    pub git_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRequest {
    pub path_raw: String,
    pub path: PathBuf,
    pub url: String,
    pub git_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoState {
    Current,
    Missing,
    Differs,
    Dirty,
    Conflict(String),
}

impl RepoState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Missing => "missing",
            Self::Differs => "differs",
            Self::Dirty => "dirty",
            Self::Conflict(_) => "conflict",
        }
    }

    pub fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoStatus {
    pub request: RepoRequest,
    pub origin: Option<String>,
    pub current_ref: Option<String>,
    pub current_sha: Option<String>,
    pub state: RepoState,
}

impl RepoRequest {
    pub fn from_toml(path_raw: String, config: RepoTomlConfig) -> Result<Self> {
        let path = file::replace_path(&path_raw);
        if path.is_relative() {
            bail!("path must be absolute or start with ~/");
        }
        let Some(url) = config.url.map(|s| s.trim().to_string()) else {
            bail!("must set `url`");
        };
        if url.is_empty() {
            bail!("must set a non-empty `url`");
        }
        let git_ref = config.git_ref.map(|s| s.trim().to_string());
        let git_ref = match git_ref {
            Some(git_ref) if git_ref.is_empty() => bail!("`ref` must not be empty"),
            other => other,
        };
        Ok(Self {
            path_raw,
            path,
            url,
            git_ref,
        })
    }
}

impl std::fmt::Display for RepoRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", file::display_path(&self.path))
    }
}

pub fn status(requests: &[RepoRequest]) -> Result<Vec<RepoStatus>> {
    requests.iter().map(status_one).collect()
}

pub fn apply(requests: &[RepoRequest], dry_run: bool) -> Result<()> {
    for status in status(requests)? {
        match status.state {
            RepoState::Current => {
                info!("repos: {} already current", status.request);
            }
            RepoState::Missing => clone_repo(&status.request, dry_run)?,
            RepoState::Differs => update_repo(&status.request, dry_run)?,
            RepoState::Dirty => {
                bail!(
                    "repos: {} has local changes; commit, stash, or clean them before bootstrap",
                    status.request
                );
            }
            RepoState::Conflict(reason) => {
                bail!("repos: {}: {reason}", status.request);
            }
        }
    }
    Ok(())
}

fn status_one(request: &RepoRequest) -> Result<RepoStatus> {
    if !request.path.exists() {
        return Ok(RepoStatus {
            request: request.clone(),
            origin: None,
            current_ref: None,
            current_sha: None,
            state: RepoState::Missing,
        });
    }
    if !request.path.is_dir() {
        return Ok(conflict_status(
            request,
            "path exists and is not a directory".to_string(),
        ));
    }
    if !is_git_repo(&request.path) {
        return Ok(conflict_status(
            request,
            "path exists and is not a git repository".to_string(),
        ));
    }

    let origin = git_output(&request.path, &["config", "--get", "remote.origin.url"]).ok();
    if origin.as_deref() != Some(request.url.as_str()) {
        return Ok(RepoStatus {
            request: request.clone(),
            origin,
            current_ref: current_ref(&request.path).ok(),
            current_sha: current_sha(&request.path).ok(),
            state: RepoState::Conflict("origin does not match configured url".to_string()),
        });
    }

    let current_ref = current_ref(&request.path).ok();
    let current_sha = current_sha(&request.path).ok();
    if !is_clean(&request.path)? {
        return Ok(RepoStatus {
            request: request.clone(),
            origin,
            current_ref,
            current_sha,
            state: RepoState::Dirty,
        });
    }

    let state = match &request.git_ref {
        None => RepoState::Current,
        Some(git_ref) => {
            if ref_is_current(
                &request.path,
                git_ref,
                current_ref.as_deref(),
                current_sha.as_deref(),
            ) {
                RepoState::Current
            } else {
                RepoState::Differs
            }
        }
    };
    Ok(RepoStatus {
        request: request.clone(),
        origin,
        current_ref,
        current_sha,
        state,
    })
}

fn conflict_status(request: &RepoRequest, reason: String) -> RepoStatus {
    RepoStatus {
        request: request.clone(),
        origin: None,
        current_ref: None,
        current_sha: None,
        state: RepoState::Conflict(reason),
    }
}

fn clone_repo(request: &RepoRequest, dry_run: bool) -> Result<()> {
    if dry_run {
        miseprintln!(
            "{}",
            shell_words::join([
                "git".to_string(),
                "clone".to_string(),
                request.url.clone(),
                request.path.display().to_string(),
            ])
        );
        if let Some(git_ref) = &request.git_ref {
            print_git_command(&request.path, &["checkout", git_ref])?;
        }
        return Ok(());
    }

    if let Some(parent) = request.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run_command(
        Command::new("git")
            .arg("clone")
            .arg(&request.url)
            .arg(&request.path),
    )?;
    if let Some(git_ref) = &request.git_ref {
        git_run(&request.path, &["checkout", git_ref])?;
    }
    Ok(())
}

fn update_repo(request: &RepoRequest, dry_run: bool) -> Result<()> {
    let Some(git_ref) = &request.git_ref else {
        return Ok(());
    };
    if dry_run {
        print_git_command(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
        print_git_command(&request.path, &["checkout", git_ref])?;
        print_git_command(&request.path, &["pull", "--ff-only", "origin", git_ref])?;
        return Ok(());
    }
    git_run(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
    git_run(&request.path, &["checkout", git_ref])?;
    if current_ref(&request.path).ok().as_deref() == Some(git_ref.as_str()) {
        git_run(&request.path, &["pull", "--ff-only", "origin", git_ref])?;
    }
    Ok(())
}

fn ref_is_current(
    path: &Path,
    git_ref: &str,
    current_ref: Option<&str>,
    current_sha: Option<&str>,
) -> bool {
    if current_ref == Some(git_ref) {
        return remote_ref_matches_head(path, git_ref, current_sha).unwrap_or(true);
    }
    if current_sha.is_some_and(|sha| sha == git_ref) {
        return true;
    }
    if let (Some(sha), Ok(local_sha)) = (current_sha, local_ref_sha(path, git_ref))
        && sha == local_sha
    {
        return remote_ref_matches_head(path, git_ref, current_sha).unwrap_or(true);
    }
    false
}

fn remote_ref_matches_head(path: &Path, git_ref: &str, current_sha: Option<&str>) -> Result<bool> {
    let Some(current_sha) = current_sha else {
        return Ok(false);
    };
    match remote_ref_sha(path, git_ref)? {
        Some(remote_sha) => Ok(remote_sha == current_sha),
        None => Ok(true),
    }
}

fn is_git_repo(path: &Path) -> bool {
    git_output(path, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|out| out == "true")
}

fn is_clean(path: &Path) -> Result<bool> {
    Ok(git_output(path, &["status", "--porcelain=v1", "--untracked-files=all"])?.is_empty())
}

fn current_ref(path: &Path) -> Result<String> {
    git_output(path, &["rev-parse", "--abbrev-ref", "HEAD"])
}

fn current_sha(path: &Path) -> Result<String> {
    git_output(path, &["rev-parse", "HEAD"])
}

fn local_ref_sha(path: &Path, git_ref: &str) -> Result<String> {
    git_output(
        path,
        &["rev-parse", "--verify", &format!("{git_ref}^{{commit}}")],
    )
}

fn remote_ref_sha(path: &Path, git_ref: &str) -> Result<Option<String>> {
    let out = git_output(path, &["ls-remote", "origin", git_ref])?;
    let mut fallback = None;
    for line in out.lines() {
        let Some((sha, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if name.ends_with("^{}") {
            return Ok(Some(sha.to_string()));
        }
        fallback = Some(sha.to_string());
    }
    Ok(fallback)
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let safe = format!("safe.directory={}", path.display());
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("-c")
        .arg(safe)
        .arg("-c")
        .arg("core.autocrlf=false")
        .args(args)
        .output()
        .map_err(|err| eyre!("git failed: {err:#}"))?;
    if !output.status.success() {
        bail!(
            "git -C {} {} failed: {}",
            path.display(),
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_run(path: &Path, args: &[&str]) -> Result<()> {
    let safe = format!("safe.directory={}", path.display());
    run_command(
        Command::new("git")
            .arg("-C")
            .arg(path)
            .arg("-c")
            .arg(safe)
            .arg("-c")
            .arg("core.autocrlf=false")
            .args(args),
    )
}

fn run_command(cmd: &mut Command) -> Result<()> {
    debug!("$ {:?}", cmd);
    let output = cmd.output().map_err(|err| eyre!("git failed: {err:#}"))?;
    if !output.status.success() {
        bail!(
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn print_git_command(path: &Path, args: &[&str]) -> Result<()> {
    let mut parts = vec![
        "git".to_string(),
        "-C".to_string(),
        path.display().to_string(),
    ];
    parts.extend(args.iter().map(|arg| arg.to_string()));
    miseprintln!("{}", shell_words::join(parts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_repo_request() {
        let request = RepoRequest::from_toml(
            "~/src/dotfiles".to_string(),
            RepoTomlConfig {
                url: Some(" https://example.com/dotfiles.git ".to_string()),
                git_ref: Some(" main ".to_string()),
            },
        )
        .unwrap();
        assert!(request.path.is_absolute());
        assert_eq!(request.url, "https://example.com/dotfiles.git");
        assert_eq!(request.git_ref.as_deref(), Some("main"));
        assert!(RepoRequest::from_toml("relative".to_string(), Default::default()).is_err());
        assert!(
            RepoRequest::from_toml(
                "~/src/empty".to_string(),
                RepoTomlConfig {
                    url: Some("".to_string()),
                    git_ref: None
                }
            )
            .is_err()
        );
    }

    #[test]
    fn state_names_are_stable() {
        assert_eq!(RepoState::Current.as_str(), "current");
        assert_eq!(RepoState::Missing.as_str(), "missing");
        assert_eq!(RepoState::Differs.as_str(), "differs");
        assert_eq!(RepoState::Dirty.as_str(), "dirty");
        assert_eq!(
            RepoState::Conflict("reason".to_string()).as_str(),
            "conflict"
        );
    }
}
