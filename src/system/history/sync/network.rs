//! Network operations against the setup repository, run from mise's own
//! bare repository with the user's normal git configuration (credential
//! helpers, ssh, URL rewrites). Only explicit refspecs are ever pushed:
//! never `--mirror`, never `--all`.

use eyre::{Result, bail};

use crate::system::history::shadow::HistoryRepo;

/// The fetched setup branch head.
pub(crate) const UPSTREAM_REF: &str = "refs/remotes/origin/setup";

/// A Git transport failed, as distinct from an invalid local configuration,
/// encryption policy, or reconciliation plan.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct NetworkError(pub String);

/// Authentication belongs in a credential helper or SSH agent, never in
/// persisted connection URLs or the errors recorded in history health.
pub(crate) fn validate_url(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim_start().starts_with('-') {
        bail!("setup repository URL must be nonempty and must not start with '-'");
    }
    let http_like = value
        .trim_start()
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http:"))
        || value
            .trim_start()
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https:"));
    if http_like && url::Url::parse(value).is_err() {
        bail!("invalid HTTP setup repository URL; use a Git credential helper for authentication");
    }
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

#[cfg(test)]
mod tests {
    use super::{HistoryRepo, PushOutcome, Remote, UPSTREAM_REF, validate_url};

    #[test]
    fn disposable_tip_probe_is_shallow_but_ordinary_fetch_keeps_ancestry() {
        let temp = tempfile::tempdir().unwrap();
        let source = HistoryRepo::open_or_init_in(&temp.path().join("source"))
            .unwrap()
            .unwrap();
        let tree = source.empty_object("tree").unwrap();
        let first = source.commit_tree(&tree, vec![], "first").unwrap();
        let second = source.commit_tree(&tree, vec![&first], "second").unwrap();
        source.update_ref("refs/heads/main", &second, None).unwrap();
        let url = url::Url::from_file_path(source.dir()).unwrap();
        let probe = HistoryRepo::open_or_init_in(&temp.path().join("probe"))
            .unwrap()
            .unwrap();
        Remote::new(&probe, url.as_str()).fetch_tip("main").unwrap();
        assert_eq!(
            probe.rev_list(UPSTREAM_REF, 10).unwrap(),
            vec![second.clone()]
        );
        let full = HistoryRepo::open_or_init_in(&temp.path().join("full"))
            .unwrap()
            .unwrap();
        Remote::new(&full, url.as_str()).fetch("main").unwrap();
        assert_eq!(
            full.rev_list(UPSTREAM_REF, 10).unwrap(),
            vec![second, first]
        );
        assert!(!full.dir().join("shallow").exists());
    }

    #[test]
    fn ordinary_push_preserves_ancestry_and_rejects_divergence() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let destination = HistoryRepo::open_or_init_in(&tmp.path().join("origin"))
            .unwrap()
            .unwrap();
        let local = HistoryRepo::open_or_init_in(&tmp.path().join("local"))
            .unwrap()
            .unwrap();
        let remote = Remote::new(&local, destination.dir().to_str().unwrap());
        let tree = local.empty_object("tree").unwrap();
        let first = local.commit_tree(&tree, vec![], "first").unwrap();
        let second = local.commit_tree(&tree, vec![&first], "second").unwrap();
        let third = local.commit_tree(&tree, vec![&second], "third").unwrap();
        assert_eq!(
            remote
                .push(&[format!("{third}:refs/heads/main")], Some(("main", None)))
                .unwrap(),
            PushOutcome::Done
        );
        assert_eq!(
            destination.rev_list("refs/heads/main", 10).unwrap(),
            vec![third.clone(), second, first.clone()]
        );
        let divergent = local.commit_tree(&tree, vec![&first], "divergent").unwrap();
        assert!(matches!(
            remote
                .push(
                    &[format!("{divergent}:refs/heads/main")],
                    Some(("main", Some(&third)))
                )
                .unwrap(),
            PushOutcome::Rejected(_)
        ));
        assert!(remote.fetch("main").unwrap());
        assert_eq!(local.ref_oid(UPSTREAM_REF).unwrap(), Some(third.clone()));
        assert_eq!(destination.ref_oid("refs/heads/main").unwrap(), Some(third));
        assert!(local.list_refs("refs/machines/").unwrap().is_empty());
    }

    #[test]
    fn option_like_urls_are_rejected_at_every_entry_point() {
        let tmp = tempfile::tempdir().unwrap();
        let Some(repo) = HistoryRepo::open_or_init_in(tmp.path()).unwrap() else {
            return;
        };
        for value in ["", "--upload-pack=bad", "  --upload-pack=bad"] {
            assert!(validate_url(value).is_err());
            let remote = Remote::new(&repo, value);
            assert!(remote.fetch("main").is_err());
            assert!(remote.push(&[], None).is_err());
            assert!(remote.symbolic_head().is_err());
            assert!(remote.ls_remote().is_err());
        }
    }

    #[test]
    fn malformed_http_remotes_fail_without_echoing_credentials() {
        for value in [
            "https://secret@example.com:bad/repo",
            "HTTP://secret@[broken/repo",
        ] {
            let error = validate_url(value).unwrap_err().to_string();
            assert!(!error.contains("secret"));
        }
        assert!(validate_url("git@github.com:jdx/dotfiles.git").is_ok());
        assert!(validate_url("/tmp/setup.git").is_ok());
    }
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

    /// Fetches only the ordinary setup branch into a remote-tracking ref.
    /// A missing branch returns false without changing the previous ref.
    pub(crate) fn fetch(&self, branch: &str) -> Result<bool> {
        self.fetch_with_depth(branch, false)
    }

    /// Format detection only, in a disposable repository never synchronized.
    pub(crate) fn fetch_tip(&self, branch: &str) -> Result<bool> {
        // Bundles do not support shallow fetches, but are already local.
        let local = url::Url::parse(&self.url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .unwrap_or_else(|| std::path::PathBuf::from(&self.url));
        self.fetch_with_depth(branch, !local.is_file())
    }

    fn fetch_with_depth(&self, branch: &str, shallow: bool) -> Result<bool> {
        validate_url(&self.url)?;
        let refspec = format!("+refs/heads/{branch}:{UPSTREAM_REF}");
        let mut args = vec!["fetch", "--quiet", "--no-tags"];
        if shallow {
            args.push("--depth=1");
        }
        args.extend(["--", &self.url, &refspec]);
        let output = self.repo.network(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("couldn't find remote ref")
                || stderr.contains("Couldn't find remote ref")
            {
                // an empty repository, or a branch not yet created
                return Ok(false);
            }
            return Err(NetworkError(format!("fetching {}: {}", self.url, stderr.trim())).into());
        }
        Ok(true)
    }

    /// Pushes without force. Check the observed branch before attempting the
    /// push; Git's normal fast-forward check also protects races after that
    /// check. A rejected update must be fetched and reconciled again.
    pub(crate) fn push(
        &self,
        refspecs: &[String],
        lease: Option<(&str, Option<&str>)>,
    ) -> Result<PushOutcome> {
        validate_url(&self.url)?;
        let mut args = vec!["push".to_string(), "--quiet".to_string()];
        if let Some((branch, expected)) = lease {
            let name = format!("refs/heads/{branch}");
            let refs = self.ls_remote()?;
            let observed = refs.iter().find(|(_, candidate)| candidate == &name);
            if observed.map(|(oid, _)| oid.as_str()) != expected {
                return Ok(PushOutcome::Rejected(
                    "the origin branch changed; fetch and reconcile before pushing".into(),
                ));
            }
        }
        if refspecs.iter().any(|refspec| refspec.starts_with('+')) {
            bail!("forced publication is not supported");
        }
        args.push("--".into());
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
        Err(NetworkError(format!("pushing to {}: {stderr}", self.url)).into())
    }

    /// The branch the repository's `HEAD` points at, when it says.
    pub(crate) fn symbolic_head(&self) -> Result<Option<String>> {
        validate_url(&self.url)?;
        let output =
            self.repo
                .network(["ls-remote", "--quiet", "--symref", "--", &self.url, "HEAD"])?;
        if !output.status.success() {
            return Err(NetworkError(format!(
                "listing {}: {}",
                self.url,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
            .into());
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix("ref: ")?;
                let (target, name) = rest.split_once('\t')?;
                (name == "HEAD")
                    .then(|| target.strip_prefix("refs/heads/"))
                    .flatten()
                    .map(str::to_string)
            }))
    }

    /// The remote's refs: `(oid, name)`.
    pub(crate) fn ls_remote(&self) -> Result<Vec<(String, String)>> {
        validate_url(&self.url)?;
        let output = self
            .repo
            .network(["ls-remote", "--quiet", "--", &self.url])?;
        if !output.status.success() {
            return Err(NetworkError(format!(
                "listing {}: {}",
                self.url,
                String::from_utf8_lossy(&output.stderr).trim()
            ))
            .into());
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
