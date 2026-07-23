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
//! matches. Origins are compared transport-agnostically for common network
//! forms, so an ssh origin matches its https equivalent.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use eyre::{Result, bail, eyre};
use serde::Deserialize;
use url::Url;

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
    pub fn from_toml(
        path_raw: String,
        config: RepoTomlConfig,
        project_root: Option<&Path>,
    ) -> Result<Self> {
        if path_raw.starts_with('~') && !path_raw.starts_with("~/") {
            bail!(
                "repo path `{path_raw}` cannot start with `~`; use `~/` for a home-relative path"
            );
        }
        let path = file::replace_path(&path_raw);
        let path = if path.is_absolute() {
            path
        } else {
            let Some(root) = project_root else {
                bail!(
                    "relative repo paths are only allowed in a project config; use an absolute path or a `~/` path"
                );
            };
            if path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            }) {
                bail!(
                    "relative repo path `{path_raw}` must stay within the project root (no `..` or absolute segments)"
                );
            }
            if !path
                .components()
                .any(|component| matches!(component, Component::Normal(_)))
            {
                bail!(
                    "relative repo path `{path_raw}` must name a directory inside the project root"
                );
            }
            // Join only the Normal segments so `./foobar` resolves to
            // `<root>/foobar` rather than `<root>/./foobar` — a `.` component
            // survives `Path::join` and leaks into every displayed path.
            let mut resolved = root.to_path_buf();
            for component in path.components() {
                if let Component::Normal(segment) = component {
                    resolved.push(segment);
                }
            }
            resolved
        };
        let Some(url) = config.url.map(|s| s.trim().to_string()) else {
            bail!("must set `url`");
        };
        if url.is_empty() {
            bail!("must set a non-empty `url`");
        }
        if url.starts_with('-') {
            bail!("`url` must not start with `-`");
        }
        let git_ref = config.git_ref.map(|s| s.trim().to_string());
        let git_ref = match git_ref {
            Some(git_ref) if git_ref.is_empty() => bail!("`ref` must not be empty"),
            Some(git_ref) if git_ref.starts_with('-') => bail!("`ref` must not start with `-`"),
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

pub fn preflight_statuses(statuses: &[RepoStatus]) -> Result<()> {
    for status in statuses {
        match &status.state {
            RepoState::Dirty => {
                bail!(
                    "repos: {} has local changes; commit, stash, or clean them before bootstrap",
                    status.request
                );
            }
            RepoState::Conflict(reason) => {
                bail!("repos: {}: {reason}", status.request);
            }
            RepoState::Current | RepoState::Missing | RepoState::Differs => {}
        }
    }
    Ok(())
}

/// Apply statuses previously validated with [`preflight_statuses`].
pub(crate) fn apply_statuses(statuses: &[RepoStatus], dry_run: bool) -> Result<()> {
    for status in statuses {
        match &status.state {
            RepoState::Current => {
                info!("repos: {} already current", status.request);
            }
            RepoState::Missing => clone_repo(&status.request, dry_run)?,
            RepoState::Differs => update_repo(&status.request, dry_run)?,
            RepoState::Dirty | RepoState::Conflict(_) => unreachable!("preflighted above"),
        }
    }
    Ok(())
}

/// Update statuses previously validated with [`preflight_statuses`].
pub(crate) fn update_statuses(statuses: &[RepoStatus], dry_run: bool) -> Result<()> {
    for status in statuses {
        match &status.state {
            RepoState::Missing => clone_repo(&status.request, dry_run)?,
            RepoState::Differs => update_repo(&status.request, dry_run)?,
            RepoState::Current if status.request.git_ref.is_none() => {
                update_unpinned_repo(&status.request, dry_run)?
            }
            RepoState::Current => {
                info!("repos: {} already current", status.request);
            }
            RepoState::Dirty | RepoState::Conflict(_) => unreachable!("preflighted above"),
        }
    }
    Ok(())
}

pub fn exec(
    requests: &[RepoRequest],
    command: &[String],
    dry_run: bool,
    continue_on_error: bool,
) -> Result<()> {
    let Some((program, args)) = command.split_first() else {
        bail!("repos: command is required");
    };
    let statuses = status(requests)?;
    let mut failures = vec![];
    for status in statuses {
        match &status.state {
            RepoState::Missing => {
                warn!("repos: {}: missing, skipping", status.request);
                continue;
            }
            RepoState::Conflict(reason) => {
                warn!("repos: {}: {reason}, skipping", status.request);
                continue;
            }
            RepoState::Current | RepoState::Differs | RepoState::Dirty => {}
        }

        miseprintln!("repo: {}", status.request);
        if dry_run {
            let mut parts = vec!["cd".to_string(), status.request.path.display().to_string()];
            parts.push("&&".to_string());
            parts.extend(command.iter().cloned());
            miseprintln!("{}", shell_words::join(parts));
            continue;
        }

        debug!(
            "$ (cd {} && {})",
            status.request,
            shell_words::join(command)
        );
        let result = Command::new(program)
            .args(args)
            .current_dir(&status.request.path)
            .status();
        let failed = match result {
            Ok(exit) => !exit.success(),
            Err(err) => {
                warn!("repos: {}: command failed to start: {err}", status.request);
                true
            }
        };
        if failed {
            failures.push(status.request.to_string());
            if !continue_on_error {
                bail!("repos: command failed in {}", status.request);
            }
        }
    }
    if !failures.is_empty() {
        bail!("repos: command failed in {}", failures.join(", "));
    }
    Ok(())
}

fn status_one(request: &RepoRequest) -> Result<RepoStatus> {
    if !request.path.exists() {
        return Ok(missing_status(request));
    }
    if !request.path.is_dir() {
        return Ok(conflict_status(
            request,
            "path exists and is not a directory".to_string(),
        ));
    }
    if !is_git_repo(&request.path) {
        if is_dir_empty(&request.path)? {
            return Ok(missing_status(request));
        }
        return Ok(conflict_status(
            request,
            "path exists and is not a git repository".to_string(),
        ));
    }

    let origin = git_output(&request.path, &["config", "--get", "remote.origin.url"]).ok();
    if !origin_matches_config(origin.as_deref(), &request.url) {
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

fn missing_status(request: &RepoRequest) -> RepoStatus {
    RepoStatus {
        request: request.clone(),
        origin: None,
        current_ref: None,
        current_sha: None,
        state: RepoState::Missing,
    }
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
        miseprintln!("{}", shell_words::join(clone_command_parts(request)));
        if let Some(git_ref) = checkout_after_clone_ref(request) {
            print_git_command(&request.path, &["checkout", checkout_ref_for(git_ref)])?;
        }
        return Ok(());
    }

    if let Some(parent) = request.path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut cmd = Command::new("git");
    for arg in clone_command_parts(request).into_iter().skip(1) {
        cmd.arg(arg);
    }
    run_command(&mut cmd)?;
    if let Some(git_ref) = checkout_after_clone_ref(request) {
        git_run(&request.path, &["checkout", checkout_ref_for(git_ref)])?;
    }
    Ok(())
}

fn update_repo(request: &RepoRequest, dry_run: bool) -> Result<()> {
    let Some(git_ref) = &request.git_ref else {
        return Ok(());
    };
    if dry_run {
        print_git_command(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
        print_git_command(&request.path, &["checkout", checkout_ref_for(git_ref)])?;
        if should_pull_after_checkout(&request.path, git_ref) {
            print_git_command(
                &request.path,
                &["pull", "--ff-only", "origin", pull_ref_for(git_ref)],
            )?;
        }
        return Ok(());
    }
    if !is_clean(&request.path)? {
        bail!(
            "repos: {} has local changes; commit, stash, or clean them before bootstrap",
            request
        );
    }
    git_run(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
    git_run(&request.path, &["checkout", checkout_ref_for(git_ref)])?;
    if should_pull_after_checkout(&request.path, git_ref) {
        git_run(
            &request.path,
            &["pull", "--ff-only", "origin", pull_ref_for(git_ref)],
        )?;
    }
    Ok(())
}

fn update_unpinned_repo(request: &RepoRequest, dry_run: bool) -> Result<()> {
    if !is_clean(&request.path)? {
        bail!(
            "repos: {} has local changes; commit, stash, or clean them before bootstrap",
            request
        );
    }
    let branch = current_ref(&request.path)?;
    if dry_run {
        print_git_command(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
    } else {
        git_run(&request.path, &["fetch", "--prune", "--tags", "origin"])?;
    }
    if branch == "HEAD" {
        warn!("repos: {} has detached HEAD, skipping", request);
        return Ok(());
    }
    if dry_run {
        print_git_command(&request.path, &["pull", "--ff-only", "origin", &branch])?;
        return Ok(());
    }
    git_run(&request.path, &["pull", "--ff-only", "origin", &branch])
}

fn ref_is_current(
    path: &Path,
    git_ref: &str,
    current_ref: Option<&str>,
    current_sha: Option<&str>,
) -> bool {
    if current_ref == Some(git_ref) {
        return remote_ref_matches_head(path, git_ref, current_sha).unwrap_or(false);
    }
    if current_sha.is_some_and(|sha| sha == git_ref) {
        return true;
    }
    if let (Some(sha), Ok(local_sha)) = (current_sha, local_ref_sha(path, git_ref))
        && sha == local_sha
    {
        return remote_ref_matches_head(path, git_ref, current_sha).unwrap_or(false);
    }
    false
}

fn remote_ref_matches_head(path: &Path, git_ref: &str, current_sha: Option<&str>) -> Result<bool> {
    let Some(current_sha) = current_sha else {
        return Ok(false);
    };
    match remote_ref_sha(path, git_ref)? {
        Some(remote_sha) => Ok(remote_sha == current_sha),
        None => Ok(false),
    }
}

fn origin_matches_config(origin: Option<&str>, config_url: &str) -> bool {
    let Some(origin) = origin else {
        return false;
    };
    if origin == config_url || normalize_remote_url(origin) == normalize_remote_url(config_url) {
        return true;
    }
    match (repo_identity(origin), repo_identity(config_url)) {
        (Some(origin), Some(config)) => origin == config,
        _ => false,
    }
}

/// Reduces a network remote URL to a host/path identity so the same repo is
/// recognized across transports — exactly the forms agreed in #10997:
/// `git@host:path`, `ssh://git@host/path`, and `https://host/path` compare
/// equal. Everything else keeps exact matching: `http://` and `git://` stay
/// distinct so an insecure transport is never silently preserved as
/// equivalent to a configured https url; ssh forms without a user are not
/// normalized (git resolves an omitted ssh user to the current login user,
/// not `git`); URLs carrying a query or fragment, local paths, `file://`
/// URLs, and unrecognized syntax return `None`. Explicit ports and non-`git`
/// users stay part of the identity since they can address different
/// repositories.
fn repo_identity(url: &str) -> Option<String> {
    let url = normalize_remote_url(url);
    if let Some((scheme, _)) = url.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "ssh" | "https") {
            return None;
        }
        let url = Url::parse(&url).ok()?;
        if url.query().is_some() || url.fragment().is_some() {
            return None;
        }
        // https userinfo is credential material, not repository identity — it
        // must not occupy the same identity slot as an ssh user. ssh needs an
        // explicit user (an omitted one means the login user, not `git`).
        match scheme.as_str() {
            "https" if !url.username().is_empty() || url.password().is_some() => return None,
            "ssh" if url.username().is_empty() => return None,
            _ => {}
        }
        let host = url.host_str()?.to_string();
        repo_identity_parts(url.username(), &host, url.port(), url.path())
    } else if is_scp_like_url(&url) {
        let (authority, path) = url.split_once(':')?;
        if authority.contains(['[', ']']) {
            return None;
        }
        // scp-like syntax is always ssh: no user means the login user, so
        // only explicit-user forms participate in identity comparison.
        let (user, host) = authority.rsplit_once('@')?;
        if user.is_empty() {
            return None;
        }
        repo_identity_parts(user, host, None, path)
    } else {
        None
    }
}

fn repo_identity_parts(user: &str, host: &str, port: Option<u16>, path: &str) -> Option<String> {
    let path = path.trim_matches('/');
    if host.is_empty() || path.is_empty() {
        return None;
    }
    // the conventional `git` ssh user is transport metadata on forges
    let user = match user {
        "" | "git" => String::new(),
        user => format!("{user}@"),
    };
    let host = host.to_ascii_lowercase();
    let port = port.map(|port| format!(":{port}")).unwrap_or_default();
    Some(format!("{user}{host}{port}/{path}"))
}

fn normalize_remote_url(url: &str) -> String {
    let mut url = url.trim().trim_end_matches('/').to_string();
    if should_strip_git_suffix(&url) && url.ends_with(".git") {
        url.truncate(url.len() - 4);
    }
    url
}

fn should_strip_git_suffix(url: &str) -> bool {
    if url.starts_with("file://")
        || url.starts_with('/')
        || url.starts_with("./")
        || url.starts_with("../")
        || is_windows_absolute_path(url)
    {
        return false;
    }
    url.contains("://") || is_scp_like_url(url)
}

fn is_scp_like_url(url: &str) -> bool {
    if url.contains("://") || is_windows_absolute_path(url) {
        return false;
    }
    let Some(colon) = url.find(':') else {
        return false;
    };
    url.find('/').is_none_or(|slash| colon < slash)
}

fn is_windows_absolute_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

fn clone_command_parts(request: &RepoRequest) -> Vec<String> {
    let mut parts = vec!["git".to_string(), "clone".to_string()];
    if let Some(git_ref) = clone_with_ref(request) {
        parts.push("--branch".to_string());
        parts.push(git_ref.to_string());
    }
    parts.push(request.url.clone());
    parts.push(request.path.display().to_string());
    parts
}

fn clone_with_ref(request: &RepoRequest) -> Option<&str> {
    request
        .git_ref
        .as_deref()
        .filter(|git_ref| can_clone_with_ref(git_ref))
}

fn checkout_after_clone_ref(request: &RepoRequest) -> Option<&str> {
    let clone_ref = clone_with_ref(request);
    request
        .git_ref
        .as_deref()
        .filter(|git_ref| Some(*git_ref) != clone_ref)
}

fn can_clone_with_ref(git_ref: &str) -> bool {
    !is_full_sha(git_ref) && !git_ref.starts_with("refs/")
}

fn is_full_sha(git_ref: &str) -> bool {
    matches!(git_ref.len(), 40 | 64) && git_ref.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_git_repo(path: &Path) -> bool {
    let Ok(top_level) = git_output(path, &["rev-parse", "--show-toplevel"]) else {
        return false;
    };
    paths_equal(path, Path::new(&top_level))
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn is_dir_empty(path: &Path) -> Result<bool> {
    Ok(std::fs::read_dir(path)?.next().is_none())
}

fn is_clean(path: &Path) -> Result<bool> {
    Ok(git_output(path, &["status", "--porcelain=v1"])?.is_empty())
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
    if git_ref.starts_with("refs/") {
        return remote_exact_ref_sha(path, git_ref);
    }

    let branch_ref = format!("refs/heads/{git_ref}");
    if let Some(sha) = remote_exact_ref_sha(path, &branch_ref)? {
        return Ok(Some(sha));
    }

    let tag_ref = format!("refs/tags/{git_ref}");
    remote_exact_ref_sha(path, &tag_ref)
}

fn remote_exact_ref_sha(path: &Path, git_ref: &str) -> Result<Option<String>> {
    let out = git_output(path, &["ls-remote", "origin", git_ref])?;
    Ok(parse_remote_exact_ref_sha(&out, git_ref))
}

fn parse_remote_exact_ref_sha(out: &str, git_ref: &str) -> Option<String> {
    let deref_ref = format!("{git_ref}^{{}}");
    let mut direct = None;
    for line in out.lines() {
        let Some((sha, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if name == deref_ref {
            return Some(sha.to_string());
        }
        if name == git_ref {
            direct = Some(sha.to_string());
        }
    }
    direct
}

fn should_pull_after_checkout(path: &Path, git_ref: &str) -> bool {
    local_branch_exists(path, git_ref).unwrap_or(false)
        || remote_branch_exists(path, git_ref).unwrap_or(false)
}

fn local_branch_exists(path: &Path, git_ref: &str) -> Result<bool> {
    let git_ref = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    let branch_ref = format!("refs/heads/{git_ref}");
    git_success(path, &["show-ref", "--verify", "--quiet", &branch_ref])
}

fn remote_branch_exists(path: &Path, git_ref: &str) -> Result<bool> {
    let git_ref = git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref);
    let branch_ref = format!("refs/heads/{git_ref}");
    Ok(!git_output(path, &["ls-remote", "--heads", "origin", &branch_ref])?.is_empty())
}

fn checkout_ref_for(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
}

fn pull_ref_for(git_ref: &str) -> &str {
    git_ref.strip_prefix("refs/heads/").unwrap_or(git_ref)
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

fn git_success(path: &Path, args: &[&str]) -> Result<bool> {
    let safe = format!("safe.directory={}", path.display());
    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .arg("-c")
        .arg(safe)
        .arg("-c")
        .arg("core.autocrlf=false")
        .args(args)
        .status()
        .map_err(|err| eyre!("git failed: {err:#}"))?;
    Ok(status.success())
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
    let status = cmd.status().map_err(|err| eyre!("git failed: {err:#}"))?;
    if !status.success() {
        bail!("git failed with status {status}");
    }
    Ok(())
}

fn print_git_command(path: &Path, args: &[&str]) -> Result<()> {
    let mut parts = vec![
        "git".to_string(),
        "-C".to_string(),
        path.display().to_string(),
        "-c".to_string(),
        format!("safe.directory={}", path.display()),
        "-c".to_string(),
        "core.autocrlf=false".to_string(),
    ];
    parts.extend(args.iter().map(|arg| arg.to_string()));
    miseprintln!("{}", shell_words::join(parts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            shell_words::join(args),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(path: &Path) {
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(path)
            .status()
            .unwrap();
        fs::write(path.join("version.txt"), "v1").unwrap();
        test_git(path, &["add", "."]);
        test_git(
            path,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-q",
                "-m",
                "v1",
            ],
        );
    }

    fn commit_version(path: &Path, version: &str) {
        fs::write(path.join("version.txt"), version).unwrap();
        test_git(path, &["add", "."]);
        test_git(
            path,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-q",
                "-m",
                version,
            ],
        );
    }

    #[test]
    fn validates_repo_request() {
        let request = RepoRequest::from_toml(
            "~/src/dotfiles".to_string(),
            RepoTomlConfig {
                url: Some(" https://example.com/dotfiles.git ".to_string()),
                git_ref: Some(" main ".to_string()),
            },
            None,
        )
        .unwrap();
        assert!(request.path.is_absolute());
        assert_eq!(request.url, "https://example.com/dotfiles.git");
        assert_eq!(request.git_ref.as_deref(), Some("main"));
        assert!(
            RepoRequest::from_toml(
                "~/src/empty".to_string(),
                RepoTomlConfig {
                    url: Some("".to_string()),
                    git_ref: None
                },
                None
            )
            .is_err()
        );
        assert!(
            RepoRequest::from_toml(
                "~/src/bad-url".to_string(),
                RepoTomlConfig {
                    url: Some("--upload-pack=sh".to_string()),
                    git_ref: None
                },
                None
            )
            .is_err()
        );
        assert!(
            RepoRequest::from_toml(
                "~/src/bad-ref".to_string(),
                RepoTomlConfig {
                    url: Some("https://example.com/repo.git".to_string()),
                    git_ref: Some("--detach".to_string())
                },
                None
            )
            .is_err()
        );
    }

    #[test]
    fn resolves_relative_repo_paths() {
        let root = Path::new("/home/u/proj");
        let config = || RepoTomlConfig {
            url: Some("https://example.com/tool.git".to_string()),
            git_ref: None,
        };

        let request = RepoRequest::from_toml("vendor/tool".to_string(), config(), Some(root))
            .expect("relative path with a project root should resolve");
        assert_eq!(request.path, Path::new("/home/u/proj/vendor/tool"));
        assert_eq!(request.path_raw, "vendor/tool");

        let request = RepoRequest::from_toml("./foobar".to_string(), config(), Some(root))
            .expect("`./`-prefixed relative path should resolve");
        assert_eq!(
            request.path,
            Path::new("/home/u/proj/foobar"),
            "a leading `./` should not leak into the resolved path"
        );
        assert_eq!(request.path_raw, "./foobar");

        assert!(
            RepoRequest::from_toml("vendor/tool".to_string(), config(), None).is_err(),
            "relative path without a project root should error"
        );
        assert!(
            RepoRequest::from_toml("../evil".to_string(), config(), Some(root)).is_err(),
            "relative path escaping the project root should error"
        );
        assert!(
            RepoRequest::from_toml("".to_string(), config(), Some(root)).is_err(),
            "empty path resolving to the project root itself should error"
        );
        assert!(
            RepoRequest::from_toml(".".to_string(), config(), Some(root)).is_err(),
            "`.` path resolving to the project root itself should error"
        );
        assert!(
            RepoRequest::from_toml("~".to_string(), config(), Some(root)).is_err(),
            "bare `~` should error instead of becoming a literal `~` directory"
        );
        assert!(
            RepoRequest::from_toml("~user/x".to_string(), config(), Some(root)).is_err(),
            "`~user/...` should error instead of becoming a literal `~user` directory"
        );

        #[cfg(unix)]
        {
            let request =
                RepoRequest::from_toml("/abs/elsewhere".to_string(), config(), Some(root))
                    .expect("absolute path should be accepted");
            assert_eq!(
                request.path,
                Path::new("/abs/elsewhere"),
                "absolute path should be used verbatim, not joined under the project root"
            );
        }

        let request = RepoRequest::from_toml("~/src/dotfiles".to_string(), config(), Some(root))
            .expect("~/ path should be accepted");
        assert!(request.path.is_absolute());
        assert!(
            !request.path.starts_with(root),
            "~/ path should expand to home, not be joined under the project root"
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

    #[test]
    fn origin_urls_allow_common_git_suffix_equivalence() {
        assert!(origin_matches_config(
            Some("https://github.com/jdx/mise.git"),
            "https://github.com/jdx/mise"
        ));
        assert!(origin_matches_config(
            Some("git@github.com:jdx/mise.git"),
            "git@github.com:jdx/mise/"
        ));
        assert!(!origin_matches_config(
            Some("file:///tmp/source-repo.git"),
            "file:///tmp/source-repo"
        ));
        assert_eq!(
            normalize_remote_url(r"C:\repos\foo.git"),
            r"C:\repos\foo.git"
        );
        assert_eq!(normalize_remote_url("C:/repos/foo.git"), "C:/repos/foo.git");
        assert!(!is_scp_like_url(r"C:\repos\foo.git"));
        assert!(is_scp_like_url("git@github.com:jdx/mise.git"));
        assert!(!origin_matches_config(None, "https://github.com/jdx/mise"));
    }

    #[test]
    fn origin_urls_match_the_same_repo_across_transports() {
        let https = "https://github.com/jdx/mise.git";
        for origin in [
            "git@github.com:jdx/mise.git",
            "git@github.com:jdx/mise",
            "ssh://git@github.com/jdx/mise.git",
            "ssh://git@github.com/jdx/mise",
            "https://GitHub.com/jdx/mise.git",
        ] {
            assert!(
                origin_matches_config(Some(origin), https),
                "{origin} should match {https}"
            );
            assert!(
                origin_matches_config(Some(https), origin),
                "{https} should match {origin}"
            );
        }
        // the same non-git user on both sides is the same identity
        assert!(origin_matches_config(
            Some("alice@example.com:team/repo.git"),
            "ssh://alice@example.com/team/repo"
        ));
        // scp paths compare without their optional leading slash
        assert!(origin_matches_config(
            Some("git@example.com:/srv/git/repo.git"),
            "ssh://git@example.com/srv/git/repo"
        ));
        // explicit ports match when they agree
        assert!(origin_matches_config(
            Some("ssh://git@example.com:2222/team/repo.git"),
            "ssh://git@example.com:2222/team/repo"
        ));
    }

    #[test]
    fn origin_urls_conflict_across_hosts_users_ports_and_paths() {
        let https = "https://github.com/jdx/mise.git";
        for origin in [
            "git@gitlab.com:jdx/mise.git",            // different host
            "git@github-work:jdx/mise.git",           // ssh alias
            "ssh://git@github.com:2222/jdx/mise.git", // explicit port
            "git@github.com:jdx/other.git",           // different repo
            "git@github.com:other/mise.git",          // different owner
            "alice@github.com:jdx/mise.git",          // non-git ssh user
            "ssh://alice@github.com/jdx/mise.git",    // non-git ssh user (url form)
            "ssh://github.com/jdx/mise.git",          // userless ssh = login user, not git
            "github.com:jdx/mise.git",                // userless scp = login user, not git
            "https://github.com/jdx/mise?tenant=a",   // query strings are not identity
            "http://github.com/jdx/mise.git",         // insecure transport is not equivalent
            "git://github.com/jdx/mise.git",          // insecure transport is not equivalent
            "https://alice@github.com/jdx/mise.git",  // https userinfo is credentials, not identity
            "ftp://github.com/jdx/mise.git",          // unrecognized scheme
        ] {
            assert!(
                !origin_matches_config(Some(origin), https),
                "{origin} should not match {https}"
            );
        }
        assert!(!origin_matches_config(
            Some("ssh://git@github.com:2222/jdx/mise.git"),
            "ssh://git@github.com/jdx/mise.git"
        ));
        assert!(!origin_matches_config(
            Some("alice@github.com:jdx/mise.git"),
            "git@github.com:jdx/mise.git"
        ));
        // userless ssh and query-carrying URLs only match themselves exactly
        assert!(origin_matches_config(
            Some("ssh://github.com/jdx/mise.git"),
            "ssh://github.com/jdx/mise.git"
        ));
        assert!(!origin_matches_config(
            Some("ssh://github.com/jdx/mise.git"),
            "git@github.com:jdx/mise.git"
        ));
        assert!(!origin_matches_config(
            Some("https://github.com/jdx/mise?tenant=a"),
            "https://github.com/jdx/mise?tenant=b"
        ));
        // https userinfo never occupies the ssh-user identity slot
        assert!(!origin_matches_config(
            Some("ssh://alice@example.com/team/repo.git"),
            "https://alice@example.com/team/repo.git"
        ));
        // local and file:// forms keep exact matching
        assert!(!origin_matches_config(
            Some("/srv/git/mise.git"),
            "file:///srv/git/mise.git"
        ));
        assert!(!origin_matches_config(
            Some(r"C:\repos\mise"),
            "https://github.com/jdx/mise"
        ));
    }

    #[test]
    fn status_recognizes_origin_across_transports() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        test_git(
            &repo,
            &["remote", "add", "origin", "git@github.com:jdx/mise.git"],
        );

        let request = RepoRequest {
            path_raw: repo.display().to_string(),
            path: repo.clone(),
            url: "https://github.com/jdx/mise.git".to_string(),
            git_ref: None,
        };
        assert_eq!(status_one(&request).unwrap().state, RepoState::Current);

        test_git(
            &repo,
            &[
                "remote",
                "set-url",
                "origin",
                "git@github.com:other/mise.git",
            ],
        );
        assert_eq!(
            status_one(&request).unwrap().state,
            RepoState::Conflict("origin does not match configured url".to_string())
        );
    }

    #[test]
    fn preflight_statuses_blocks_dirty_repos_before_mutation() {
        let tmp = tempfile::tempdir().unwrap();
        let missing_path = tmp.path().join("missing");
        let missing_request = RepoRequest {
            path_raw: missing_path.display().to_string(),
            path: missing_path.clone(),
            url: "file:///does/not/matter.git".to_string(),
            git_ref: None,
        };
        let dirty_request = RepoRequest {
            path_raw: tmp.path().join("dirty").display().to_string(),
            path: tmp.path().join("dirty"),
            url: "file:///does/not/matter.git".to_string(),
            git_ref: None,
        };
        let dirty_status = RepoStatus {
            request: dirty_request,
            origin: None,
            current_ref: None,
            current_sha: None,
            state: RepoState::Dirty,
        };

        let err =
            preflight_statuses(&[missing_status(&missing_request), dirty_status]).unwrap_err();

        assert!(format!("{err:#}").contains("local changes"));
        assert!(!missing_path.exists());
    }

    #[test]
    fn ref_less_update_pulls_current_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        init_repo(&source);
        let url = format!("file://{}", source.display());
        test_git(tmp.path(), &["clone", "-q", &url, target.to_str().unwrap()]);
        commit_version(&source, "v2");
        let request = RepoRequest {
            path_raw: target.display().to_string(),
            path: target.clone(),
            url,
            git_ref: None,
        };

        let statuses = status(&[request]).unwrap();
        assert_eq!(statuses[0].state, RepoState::Current);
        update_statuses(&statuses, false).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("version.txt")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn ref_less_update_skips_detached_head() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        init_repo(&source);
        let url = format!("file://{}", source.display());
        test_git(tmp.path(), &["clone", "-q", &url, target.to_str().unwrap()]);
        test_git(&target, &["checkout", "-q", "--detach"]);
        commit_version(&source, "v2");
        let request = RepoRequest {
            path_raw: target.display().to_string(),
            path: target.clone(),
            url,
            git_ref: None,
        };

        update_statuses(&status(&[request]).unwrap(), false).unwrap();

        assert_eq!(
            fs::read_to_string(target.join("version.txt")).unwrap(),
            "v1"
        );
        assert_eq!(
            local_ref_sha(&target, "origin/main").unwrap(),
            current_sha(&source).unwrap()
        );
    }

    #[test]
    fn update_repo_rechecks_clean_worktree_before_mutating() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&repo)
            .status()
            .unwrap();
        fs::write(repo.join("tracked.txt"), "v1").unwrap();
        test_git(&repo, &["add", "."]);
        test_git(
            &repo,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-q",
                "-m",
                "v1",
            ],
        );
        fs::write(repo.join("local.txt"), "local").unwrap();

        let request = RepoRequest {
            path_raw: repo.display().to_string(),
            path: repo,
            url: "file:///does/not/matter.git".to_string(),
            git_ref: Some("main".to_string()),
        };

        let err = update_repo(&request, false).unwrap_err();

        assert!(format!("{err:#}").contains("local changes"));
    }

    #[test]
    fn update_unpinned_repo_rechecks_clean_worktree_before_mutating() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("source");
        let target = tmp.path().join("target");
        init_repo(&source);
        let url = format!("file://{}", source.display());
        test_git(tmp.path(), &["clone", "-q", &url, target.to_str().unwrap()]);
        let request = RepoRequest {
            path_raw: target.display().to_string(),
            path: target.clone(),
            url,
            git_ref: None,
        };
        let statuses = status(&[request]).unwrap();
        let original_origin_sha = local_ref_sha(&target, "origin/main").unwrap();
        commit_version(&source, "v2");
        fs::write(target.join("local.txt"), "local").unwrap();

        let err = update_unpinned_repo(&statuses[0].request, false).unwrap_err();

        assert!(format!("{err:#}").contains("local changes"));
        assert_eq!(
            local_ref_sha(&target, "origin/main").unwrap(),
            original_origin_sha
        );
    }

    #[test]
    fn exec_rejects_an_empty_command() {
        let err = exec(&[], &[], false, false).unwrap_err();
        assert!(format!("{err:#}").contains("command is required"));
    }

    #[test]
    fn clone_command_uses_branch_flag_except_for_sha_refs() {
        let mut request = RepoRequest {
            path_raw: "/tmp/repo".to_string(),
            path: PathBuf::from("/tmp/repo"),
            url: "https://github.com/jdx/mise.git".to_string(),
            git_ref: Some("main".to_string()),
        };
        assert_eq!(
            clone_command_parts(&request),
            vec![
                "git",
                "clone",
                "--branch",
                "main",
                "https://github.com/jdx/mise.git",
                "/tmp/repo"
            ]
        );
        assert_eq!(checkout_after_clone_ref(&request), None);

        let sha = "0123456789abcdef0123456789abcdef01234567";
        request.git_ref = Some(sha.to_string());
        assert_eq!(
            clone_command_parts(&request),
            vec![
                "git",
                "clone",
                "https://github.com/jdx/mise.git",
                "/tmp/repo"
            ]
        );
        assert_eq!(checkout_after_clone_ref(&request), Some(sha));
    }

    #[test]
    fn branch_refs_use_branch_name_for_checkout_and_pull() {
        assert_eq!(checkout_ref_for("refs/heads/main"), "main");
        assert_eq!(pull_ref_for("refs/heads/main"), "main");
        assert_eq!(checkout_ref_for("refs/tags/v1"), "refs/tags/v1");
        assert_eq!(pull_ref_for("refs/tags/v1"), "refs/tags/v1");
        assert_eq!(checkout_ref_for("main"), "main");
        assert_eq!(pull_ref_for("main"), "main");
    }

    #[test]
    fn remote_ref_parser_uses_exact_refs() {
        let out = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/release
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/release
cccccccccccccccccccccccccccccccccccccccc\trefs/tags/release^{}
";

        assert_eq!(
            parse_remote_exact_ref_sha(out, "refs/heads/release").as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            parse_remote_exact_ref_sha(out, "refs/tags/release").as_deref(),
            Some("cccccccccccccccccccccccccccccccccccccccc")
        );
        assert_eq!(parse_remote_exact_ref_sha(out, "refs/heads/missing"), None);
    }

    #[test]
    fn empty_directory_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("target");
        fs::create_dir(&path).unwrap();
        let request = RepoRequest {
            path_raw: path.display().to_string(),
            path,
            url: "https://github.com/jdx/mise.git".to_string(),
            git_ref: None,
        };
        let status = status(&[request]).unwrap();
        assert_eq!(status[0].state, RepoState::Missing);
    }

    #[test]
    fn nested_directory_inside_parent_repo_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("parent");
        let nested = parent.join("nested");
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&parent)
            .status()
            .unwrap();
        fs::create_dir(&nested).unwrap();

        assert!(!is_git_repo(&nested));
        let request = RepoRequest {
            path_raw: nested.display().to_string(),
            path: nested,
            url: "https://github.com/jdx/mise.git".to_string(),
            git_ref: None,
        };
        let status = status(&[request]).unwrap();
        assert_eq!(status[0].state, RepoState::Missing);
    }

    #[test]
    fn branch_ref_is_not_current_when_remote_check_fails() {
        assert!(!ref_is_current(
            Path::new("/path/that/does/not/exist"),
            "main",
            Some("main"),
            Some("abc123")
        ));
    }

    #[test]
    fn missing_remote_ref_is_not_current() {
        let tmp = tempfile::tempdir().unwrap();
        let origin = tmp.path().join("origin.git");
        let work = tmp.path().join("work");

        Command::new("git")
            .args(["init", "-q", "--bare"])
            .arg(&origin)
            .status()
            .unwrap();
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&work)
            .status()
            .unwrap();
        fs::write(work.join("version.txt"), "v1").unwrap();
        test_git(&work, &["add", "."]);
        test_git(
            &work,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test User",
                "commit",
                "-q",
                "-m",
                "v1",
            ],
        );
        test_git(
            &work,
            &["remote", "add", "origin", origin.to_str().unwrap()],
        );
        test_git(&work, &["push", "-q", "origin", "main"]);
        test_git(&work, &["checkout", "-q", "-b", "local-only"]);

        let sha = current_sha(&work).unwrap();
        assert!(!ref_is_current(
            &work,
            "local-only",
            Some("local-only"),
            Some(&sha)
        ));
    }
}
