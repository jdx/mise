//! Ordinary branch ancestry. Reconciliation supplies a fully validated tree;
//! this layer preserves commit identities and never constructs another history.

use eyre::{Result, bail};

use super::network::UPSTREAM_REF;
use crate::system::history::shadow::HistoryRepo;

#[derive(Debug)]
pub(crate) struct Heads {
    pub local: Option<String>,
    pub remote: Option<String>,
    pub base: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Candidate {
    pub commit: String,
    expected_local: Option<String>,
    expected_remote: Option<String>,
}

impl Heads {
    pub(crate) fn read(repo: &HistoryRepo) -> Result<Self> {
        let local = repo.ref_oid(HistoryRepo::HISTORY_REF)?;
        let remote = repo.ref_oid(UPSTREAM_REF)?;
        let base = match (&local, &remote) {
            (Some(local), Some(remote)) => {
                let bases = repo.merge_bases(local, remote)?;
                if bases.is_empty() {
                    bail!(
                        "local and origin histories are unrelated; use a fresh setup store to adopt origin, or reconcile the repositories explicitly with Git. Neither history was replaced"
                    );
                }
                // A criss-cross merge has shared ancestry but no unique base.
                // It needs a merge commit, never an arbitrary fast-forward.
                (bases.len() == 1).then(|| bases[0].clone())
            }
            _ => None,
        };
        Ok(Self {
            local,
            remote,
            base,
        })
    }

    /// Preparing a candidate does not advance a branch or write live files.
    /// The caller must finish its complete application before adopting it.
    pub(crate) fn candidate(&self, repo: &HistoryRepo, tree: &str) -> Result<Option<Candidate>> {
        let commit = match (&self.local, &self.remote) {
            (None, None) => return Ok(None),
            (Some(local), None) => {
                if repo.output_tree_of(local)? != tree {
                    bail!("saved files changed while preparing publication; plan again");
                }
                local.clone()
            }
            (None, Some(remote)) => {
                if repo.output_tree_of(remote)? != tree {
                    bail!("incoming files changed while preparing adoption; plan again");
                }
                remote.clone()
            }
            (Some(local), Some(remote)) => {
                if self.base.as_ref() == Some(remote) {
                    if repo.output_tree_of(local)? != tree {
                        bail!("saved files changed while preparing publication; plan again");
                    }
                    local.clone()
                } else if self.base.as_ref() == Some(local) && repo.output_tree_of(remote)? == tree
                {
                    remote.clone()
                } else {
                    repo.commit_tree(tree, vec![local, remote], "merge origin dotfiles")?
                }
            }
        };
        Ok(Some(Candidate {
            commit,
            expected_local: self.local.clone(),
            expected_remote: self.remote.clone(),
        }))
    }
}

impl Candidate {
    pub(crate) fn adopt(&self, repo: &HistoryRepo) -> Result<()> {
        if repo.ref_oid(UPSTREAM_REF)? != self.expected_remote {
            bail!("origin changed while applying this setup; reconcile again before publication");
        }
        // Compare-and-swap also verifies no local boundary/save was inserted
        // during application. The caller must recompute instead of dropping it.
        repo.update_history_head(&self.commit, self.expected_local.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_adoption_keeps_remote_identity_and_rejects_a_changed_fetch() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let incoming_tree = tree(&repo, b"incoming");
        let incoming = repo
            .commit_tree(&incoming_tree, vec![], "incoming")
            .unwrap();
        repo.update_ref(UPSTREAM_REF, &incoming, None).unwrap();
        let candidate = Heads::read(&repo)
            .unwrap()
            .candidate(&repo, &incoming_tree)
            .unwrap()
            .unwrap();
        let newer = repo
            .commit_tree(&incoming_tree, vec![&incoming], "new boundary")
            .unwrap();
        repo.update_ref(UPSTREAM_REF, &newer, Some(&incoming))
            .unwrap();
        assert!(candidate.adopt(&repo).is_err());
        assert_eq!(repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap(), None);
        let candidate = Heads::read(&repo)
            .unwrap()
            .candidate(&repo, &incoming_tree)
            .unwrap()
            .unwrap();
        candidate.adopt(&repo).unwrap();
        assert_eq!(candidate.commit, newer);
        assert_eq!(repo.ref_oid("HEAD").unwrap(), Some(newer));
    }

    fn tree(repo: &HistoryRepo, bytes: &[u8]) -> String {
        repo.compose(
            &repo.empty_object("tree").unwrap(),
            &[crate::system::history::shadow::Overlay {
                path: "home/.zshrc".into(),
                object: Some(("100644".into(), repo.hash_blob(bytes).unwrap())),
            }],
        )
        .unwrap()
    }

    #[test]
    fn delayed_publication_reuses_all_local_commits() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let first_tree = tree(&repo, b"one");
        let first = repo.commit_tree(&first_tree, vec![], "first save").unwrap();
        let second_tree = tree(&repo, b"two");
        let second = repo
            .commit_tree(&second_tree, vec![&first], "second save")
            .unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &second, None)
            .unwrap();
        let candidate = Heads::read(&repo)
            .unwrap()
            .candidate(&repo, &second_tree)
            .unwrap()
            .unwrap();
        assert_eq!(candidate.commit, second);
        candidate.adopt(&repo).unwrap();
        assert_eq!(
            repo.rev_list(&candidate.commit, usize::MAX).unwrap(),
            vec![second, first]
        );
        assert!(repo.list_refs("refs/setup/").unwrap().is_empty());
    }

    #[test]
    fn fast_forward_keeps_remote_commit_and_stale_candidates_preserve_edits() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let before_tree = tree(&repo, b"before");
        let before = repo.commit_tree(&before_tree, vec![], "before").unwrap();
        let incoming_tree = tree(&repo, b"incoming");
        let incoming = repo
            .commit_tree(&incoming_tree, vec![&before], "incoming")
            .unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &before, None)
            .unwrap();
        repo.update_ref(UPSTREAM_REF, &incoming, None).unwrap();
        let candidate = Heads::read(&repo)
            .unwrap()
            .candidate(&repo, &incoming_tree)
            .unwrap()
            .unwrap();
        assert_eq!(candidate.commit, incoming);
        let edit = repo
            .commit_tree(&tree(&repo, b"later edit"), vec![&before], "later edit")
            .unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &edit, Some(&before))
            .unwrap();
        assert!(candidate.adopt(&repo).is_err());
        assert_eq!(repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap(), Some(edit));
    }

    #[test]
    fn divergent_merge_keeps_both_parent_histories() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let initial = tree(&repo, b"initial");
        let base = repo.commit_tree(&initial, vec![], "base").unwrap();
        let local = repo
            .commit_tree(&tree(&repo, b"local"), vec![&base], "local")
            .unwrap();
        let remote = repo
            .commit_tree(&tree(&repo, b"remote"), vec![&base], "remote")
            .unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &local, None)
            .unwrap();
        repo.update_ref(UPSTREAM_REF, &remote, None).unwrap();
        let merged = tree(&repo, b"resolved");
        let candidate = Heads::read(&repo)
            .unwrap()
            .candidate(&repo, &merged)
            .unwrap()
            .unwrap();
        candidate.adopt(&repo).unwrap();
        let history = repo.rev_list(&candidate.commit, usize::MAX).unwrap();
        assert_eq!(history.len(), 4);
        assert!(history.contains(&local) && history.contains(&remote) && history.contains(&base));
        assert_eq!(repo.output_tree_of(&candidate.commit).unwrap(), merged);
    }

    #[test]
    fn unrelated_histories_are_never_adopted_over_local_commits() {
        let temporary = tempfile::tempdir().unwrap();
        let repo = HistoryRepo::open_or_init_in(temporary.path())
            .unwrap()
            .unwrap();
        let tree = tree(&repo, b"same contents are not shared ancestry");
        let local = repo.commit_tree(&tree, vec![], "local root").unwrap();
        let remote = repo.commit_tree(&tree, vec![], "remote root").unwrap();
        repo.update_ref(HistoryRepo::HISTORY_REF, &local, None)
            .unwrap();
        repo.update_ref(UPSTREAM_REF, &remote, None).unwrap();
        assert!(
            Heads::read(&repo)
                .unwrap_err()
                .to_string()
                .contains("unrelated")
        );
        assert_eq!(repo.ref_oid(HistoryRepo::HISTORY_REF).unwrap(), Some(local));
        assert_eq!(repo.ref_oid(UPSTREAM_REF).unwrap(), Some(remote));
    }
}
