//! Promote only enrolled manual-file preimages into the ordinary before commit.
use super::*;
use crate::system::history::{journal::JournalEntry, journal::PathSnapshot, manifest::Manifest};

impl Store {
    pub(crate) fn protect_manual_preimage(
        &self,
        before: &str,
        tracked: &TrackedSet,
        path: &Path,
        prior: &PathSnapshot,
        previous: &[JournalEntry],
    ) -> Result<Option<Box<Entry>>> {
        let _lock = self.lock()?;
        let repo = self
            .repo
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Git is unavailable"))?;
        let mut checkpoint = repo.read_meta(before)?;
        let tree = repo.output_tree_of(before)?;
        let head = repo
            .ref_oid(HistoryRepo::HISTORY_REF)?
            .ok_or_else(|| eyre::eyre!("history branch disappeared"))?;
        eyre::ensure!(
            repo.output_tree_of(&head)? == tree,
            "history changed during the operation; cannot replace its before state"
        );
        let mut promotion = Promotion {
            store: self,
            repo,
            tracked,
            previous,
            overlays: BTreeMap::new(),
            modes: checkpoint.tree.modes.clone(),
        };
        promotion.snapshot(path, prior, &tree)?;
        if promotion.overlays.is_empty() && promotion.modes == checkpoint.tree.modes {
            return Ok(None);
        }
        let overlays: Vec<_> = promotion
            .overlays
            .into_iter()
            .map(|(path, object)| Overlay { path, object })
            .collect();
        let next = repo.compose(&tree, &overlays)?;
        let mut manifest = Manifest::read(repo, &next)?
            .ok_or_else(|| eyre::eyre!("before commit is missing enrollment metadata"))?;
        manifest.capture_permissions(&tracked.entries, &promotion.modes)?;
        let next = manifest.write(repo, &next)?;
        if next == tree && promotion.modes == checkpoint.tree.modes {
            return Ok(None);
        }
        checkpoint.changes = changes_from(repo, Some(&tree), &next, Some(checkpoint.uuid.clone()))?;
        checkpoint.uuid = store::new_uuid();
        checkpoint.created_at = store::now_rfc3339();
        checkpoint.tree.snapshot = Some(next);
        checkpoint.tree.modes = promotion.modes;
        checkpoint.operation = None;
        let mut index = store::load_index_in(&self.state_dir)?;
        if index.entries.last().map(|entry| entry.commit.as_str()) != Some(head.as_str()) {
            index = self.rebuild_index_locked()?;
        }
        self.commit_record_locked(checkpoint, index, None).map(Some)
    }
}

struct Promotion<'a> {
    store: &'a Store,
    repo: &'a HistoryRepo,
    tracked: &'a TrackedSet,
    previous: &'a [JournalEntry],
    overlays: BTreeMap<String, Option<(String, String)>>,
    modes: BTreeMap<String, u32>,
}

impl Promotion<'_> {
    fn eligible(&self, path: &Path) -> Result<bool> {
        if self.previous.iter().any(|entry| match entry {
            JournalEntry::PathChanged {
                path: earlier,
                prior,
                ..
            } => {
                path == earlier
                    || (!matches!(prior, PathSnapshot::Directory { .. })
                        && path.starts_with(earlier))
            }
            _ => false,
        }) {
            return Ok(false);
        }
        Ok(self
            .tracked
            .entry_for(path)
            .is_some_and(|entry| !entry.policy.autosave)
            && self.tracked.would_retain(path)?)
    }

    fn object(
        &mut self,
        path: &Path,
        bytes: &[u8],
        mode: &str,
        permissions: Option<u32>,
    ) -> Result<()> {
        if !self.eligible(path)? {
            return Ok(());
        }
        let entry = self
            .tracked
            .entry_for(path)
            .expect("eligible path has an owner");
        let branch_path = entry.tree_path(path)?;
        let (bytes, mode) = if entry.policy.encrypt {
            let mut names = self.tracked.manifest.recipients.clone();
            names.sort();
            names.dedup();
            let recipients = names
                .iter()
                .map(|name| {
                    crate::agecrypt::parse_recipient_mode(name, console::user_attended_stderr())?
                        .ok_or_else(|| eyre::eyre!("invalid encrypted-file recipient"))
                })
                .collect::<Result<Vec<_>>>()?;
            let scheme = crate::hash::hash_sha256_to_str(&names.join("\n"));
            (
                crate::system::history::sync::files::encode(
                    &branch_path,
                    mode,
                    bytes,
                    &scheme,
                    &recipients,
                )?,
                "100644",
            )
        } else {
            (bytes.to_vec(), mode)
        };
        self.overlays.insert(
            branch_path,
            Some((mode.into(), self.repo.hash_blob(&bytes)?)),
        );
        self.mode(path, permissions);
        Ok(())
    }

    fn mode(&mut self, path: &Path, mode: Option<u32>) {
        let display = display_path(path);
        if cfg!(unix)
            && let Some(mode) = mode
        {
            self.modes.insert(display, mode & 0o777);
        } else {
            self.modes.remove(&display);
        }
    }

    fn snapshot(&mut self, path: &Path, prior: &PathSnapshot, tree: &str) -> Result<()> {
        match prior {
            PathSnapshot::File { content, mode } => {
                if self.eligible(path)? {
                    let bytes = crate::system::history::recovery::read_blob(
                        self.store.state_dir(),
                        content,
                    )?;
                    self.object(
                        path,
                        &bytes,
                        if mode & 0o111 != 0 {
                            "100755"
                        } else {
                            "100644"
                        },
                        Some(*mode),
                    )?;
                }
            }
            PathSnapshot::Symlink { dest } => {
                self.object(path, &shadow::path_bytes(dest), "120000", None)?
            }
            PathSnapshot::Missing | PathSnapshot::Dir { .. } => {
                // Remove leaves individually: an earlier child preimage wins
                // even if a later phase replaces its entire parent directory.
                for leaf in self.repo.ls_tree(tree)? {
                    let local = crate::file::replace_path(tree_path_to_display(&leaf.path));
                    if local.starts_with(path)
                        && self.eligible(&local)?
                        && self
                            .tracked
                            .entry_for(&local)
                            .map(|entry| entry.tree_path(&local))
                            .transpose()?
                            .as_deref()
                            == Some(leaf.path.as_str())
                    {
                        self.overlays.insert(leaf.path, None);
                        self.mode(&local, None);
                    }
                }
                if let PathSnapshot::Dir {
                    files,
                    links,
                    dirs,
                    mode,
                } = prior
                {
                    if self.eligible(path)? {
                        self.mode(path, Some(*mode));
                    }
                    for dir in dirs {
                        let child = path.join(&dir.rel);
                        if self.eligible(&child)? {
                            self.mode(&child, Some(dir.mode));
                        }
                    }
                    for file in files {
                        self.snapshot(
                            &path.join(&file.rel),
                            &PathSnapshot::File {
                                content: file.content.clone(),
                                mode: file.mode,
                            },
                            tree,
                        )?;
                    }
                    for link in links {
                        self.snapshot(
                            &path.join(&link.rel),
                            &PathSnapshot::Symlink {
                                dest: link.dest.clone(),
                            },
                            tree,
                        )?;
                    }
                }
            }
            PathSnapshot::Directory { mode } => {
                if self.eligible(path)? {
                    self.mode(path, Some(*mode));
                }
            }
            PathSnapshot::Unrecorded { reason, .. } => {
                if self.eligible(path)? {
                    eyre::bail!("cannot protect {}: {reason}", display_path(path));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::files::{FileMode, FilePolicy};
    use crate::system::history::journal::Capture;

    #[test]
    fn first_preimages_survive_multiple_phases_without_saving_siblings() -> Result<()> {
        let state = tempfile::tempdir()?;
        let live =
            tempfile::tempdir_in(crate::system::history::sync::layout::Roots::current().home)?;
        let first = live.path().join("first");
        let second = live.path().join("second");
        let sibling = live.path().join("sibling");
        for path in [&first, &second, &sibling] {
            std::fs::write(path, "saved")?;
        }
        let mut policy = FilePolicy::for_mode(FileMode::Track);
        policy.autosave = false;
        let entry = TrackedEntry::new(live.path().to_owned(), "track", policy);
        let tracked = TrackedSet {
            entries: vec![entry.clone()],
            ..Default::default()
        };
        let store = Store::open_in(state.path())?;
        let mut draft = Draft::new(Trigger::Baseline);
        draft.explicit_paths.push(live.path().to_owned());
        let Outcome::Created(baseline) = store.attempt(&tracked, draft)? else {
            panic!("no baseline")
        };
        for path in [&first, &second, &sibling] {
            std::fs::write(path, "unsaved")?;
        }
        let mut before = baseline.commit;
        let mut journal = vec![];
        for path in [&first, &second, &first] {
            let prior = PathSnapshot::capture_with(state.path(), path, Capture::Full);
            if let Some(commit) =
                store.protect_manual_preimage(&before, &tracked, path, &prior, &journal)?
            {
                before = commit.commit;
            }
            journal.push(JournalEntry::PathChanged {
                part: "dotfiles".into(),
                item: "test".into(),
                path: path.clone(),
                prior,
            });
            std::fs::write(path, "deployed")?;
        }
        let repo = store.repo().unwrap();
        for (path, expected) in [
            (&first, "unsaved"),
            (&second, "unsaved"),
            (&sibling, "saved"),
        ] {
            let (_, oid) = repo.object_at(&before, &entry.tree_path(path)?)?.unwrap();
            assert_eq!(repo.cat_object(&oid)?, expected.as_bytes());
        }
        let untracked = state.path().join("private");
        std::fs::write(&untracked, "private recovery bytes")?;
        let prior = PathSnapshot::capture_with(state.path(), &untracked, Capture::Full);
        assert!(
            store
                .protect_manual_preimage(&before, &tracked, &untracked, &prior, &journal)?
                .is_none()
        );
        let oid = repo.transient_blob_id(b"private recovery bytes")?;
        assert!(
            Store::open_in(state.path())?
                .repo()
                .unwrap()
                .cat_object(&oid)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn manual_preimages_are_encrypted_before_git_storage() -> Result<()> {
        let state = tempfile::tempdir()?;
        let live =
            tempfile::tempdir_in(crate::system::history::sync::layout::Roots::current().home)?;
        let path = live.path().join("encrypted");
        std::fs::write(&path, "original")?;
        let mut policy = FilePolicy::for_mode(FileMode::Track);
        policy.autosave = false;
        policy.encrypt = true;
        let entry = TrackedEntry::new(path.clone(), "track", policy);
        let mut tracked = TrackedSet {
            entries: vec![entry.clone()],
            ..Default::default()
        };
        let identity = age::x25519::Identity::generate();
        tracked
            .manifest
            .recipients
            .push(identity.to_public().to_string());
        let store = Store::open_in(state.path())?;
        let mut draft = Draft::new(Trigger::Baseline);
        draft.explicit_paths.push(path.clone());
        let Outcome::Created(baseline) = store.attempt(&tracked, draft)? else {
            panic!("no baseline")
        };
        let secret = b"unsaved manual preimage secret";
        std::fs::write(&path, secret)?;
        let prior = PathSnapshot::capture_with(state.path(), &path, Capture::Full);
        let protected = store
            .protect_manual_preimage(&baseline.commit, &tracked, &path, &prior, &[])?
            .unwrap();
        let repo = store.repo().unwrap();
        let (_, oid) = repo
            .object_at(&protected.commit, &entry.tree_path(&path)?)?
            .unwrap();
        assert!(
            repo.cat_object(&oid)?
                .starts_with(b"mise-encrypted-file-v1\n")
        );
        let plaintext = repo.transient_blob_id(secret)?;
        assert!(
            Store::open_in(state.path())?
                .repo()
                .unwrap()
                .cat_object(&plaintext)
                .is_err()
        );
        tracked.manifest.recipients.clear();
        assert!(
            store
                .protect_manual_preimage(&protected.commit, &tracked, &path, &prior, &[])
                .is_err()
        );
        assert!(
            Store::open_in(state.path())?
                .repo()
                .unwrap()
                .cat_object(&plaintext)
                .is_err()
        );
        Ok(())
    }
}
