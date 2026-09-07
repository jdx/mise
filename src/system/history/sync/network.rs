//! Network operations against the setup repository, run from mise's own
//! bare repository with the user's normal git configuration (credential
//! helpers, ssh, URL rewrites). Only explicit refspecs are ever pushed:
//! never `--mirror`, never `--all`.

use eyre::{Result, bail};

use crate::system::history::shadow::HistoryRepo;

/// The fetched setup branch head.
pub(crate) const UPSTREAM_REF: &str = "refs/setup/upstream";
/// This machine's publication commits (parent chain on upstream).
pub(crate) const PUBLISH_REF: &str = "refs/setup/publish";
/// Fetched machine recovery refs: `refs/machines/<machine-id>/<uuid>`.
pub(crate) const MACHINES_PREFIX: &str = "refs/machines/";
/// Where machine recovery refs live on the remote.
pub(crate) const REMOTE_MACHINES_PREFIX: &str = "refs/mise-history/";

/// Authentication belongs in a credential helper or SSH agent, never in
/// persisted connection URLs or the errors recorded in history health.
pub(crate) fn validate_url(value: &str) -> Result<()> {
    if let Ok(url) = url::Url::parse(value) {
        let http = matches!(url.scheme(), "http" | "https");
        if url.password().is_some()
            || (http
                && (!url.username().is_empty()
                    || url.query().is_some()
                    || url.fragment().is_some()))
        {
            bail!(
                "setup repository URLs must not contain credentials, query parameters, or fragments; use a Git credential helper or SSH agent"
            );
        }
    }
    Ok(())
}

pub(crate) struct Remote<'a> {
    repo: &'a HistoryRepo,
    url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PushOutcome {
    Done,
    /// The lease failed: someone else moved the branch.
    Rejected(String),
}

impl<'a> Remote<'a> {
    pub(crate) fn new(repo: &'a HistoryRepo, url: &str) -> Self {
        Self {
            repo,
            url: url.to_string(),
        }
    }

    /// Fetches the setup branch into `refs/setup/upstream` and every
    /// machine recovery ref into `refs/machines/`. A branch that does not
    /// exist is not an error (the repository may be empty): `false`, with
    /// `refs/setup/upstream` left as it was.
    pub(crate) fn fetch(&self, branch: &str) -> Result<bool> {
        self.fetch_with(branch, false)
    }

    /// The same, dropping machine refs the repository no longer has: what
    /// a sync with the connected repository does, never a look at another.
    pub(crate) fn fetch_pruning(&self, branch: &str) -> Result<bool> {
        self.fetch_with(branch, true)
    }

    fn fetch_with(&self, branch: &str, prune: bool) -> Result<bool> {
        validate_url(&self.url)?;
        let mut args = vec!["fetch", "--quiet", "--no-tags"];
        if prune {
            args.push("--prune");
        }
        let machines = format!("+{REMOTE_MACHINES_PREFIX}*:{MACHINES_PREFIX}*");
        args.extend([self.url.as_str(), machines.as_str()]);
        let output = self.repo.network(args)?;
        if !output.status.success() {
            bail!(
                "fetching {}: {}",
                self.url,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let output = self.repo.network([
            "fetch",
            "--quiet",
            "--no-tags",
            &self.url,
            &format!("+refs/heads/{branch}:{UPSTREAM_REF}"),
        ])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("couldn't find remote ref")
                || stderr.contains("Couldn't find remote ref")
            {
                // an empty repository, or a branch not yet created
                return Ok(false);
            }
            bail!("fetching {}: {}", self.url, stderr.trim());
        }
        Ok(true)
    }

    /// Pushes explicit refspecs. With `lease`, the remote branch must still
    /// be at the expected commit (or absent when `None`).
    pub(crate) fn push(
        &self,
        refspecs: &[String],
        lease: Option<(&str, Option<&str>)>,
    ) -> Result<PushOutcome> {
        validate_url(&self.url)?;
        let mut args = vec!["push".to_string(), "--quiet".to_string()];
        if let Some((branch, expected)) = lease {
            args.push(format!(
                "--force-with-lease=refs/heads/{branch}:{}",
                expected.unwrap_or("")
            ));
        }
        args.push(self.url.clone());
        args.extend(refspecs.iter().cloned());
        let output = self.repo.network(args.iter().map(String::as_str))?;
        if output.status.success() {
            return Ok(PushOutcome::Done);
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.contains("stale info")
            || stderr.contains("rejected")
            || stderr.contains("fetch first")
        {
            return Ok(PushOutcome::Rejected(stderr));
        }
        bail!("pushing to {}: {stderr}", self.url)
    }

    /// Deletes remote refs (`:refs/...`).
    pub(crate) fn delete(&self, refs: &[String]) -> Result<()> {
        if refs.is_empty() {
            return Ok(());
        }
        let refspecs: Vec<String> = refs.iter().map(|name| format!(":{name}")).collect();
        match self.push(&refspecs, None)? {
            PushOutcome::Done => Ok(()),
            PushOutcome::Rejected(reason) => bail!("deleting refs on {}: {reason}", self.url),
        }
    }

    /// The remote's refs: `(oid, name)`.
    pub(crate) fn ls_remote(&self) -> Result<Vec<(String, String)>> {
        let output = self.repo.network(["ls-remote", "--quiet", &self.url])?;
        if !output.status.success() {
            bail!(
                "listing {}: {}",
                self.url,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (oid, name) = line.split_once('\t')?;
                Some((oid.to_string(), name.to_string()))
            })
            .collect())
    }
}
