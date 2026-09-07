//! Machine recovery refs: this machine's checkpoints pushed as
//! parentless wrapper commits to `refs/mise-history/<machine-id>/<uuid>`
//! on the origin, rebuilt with every policy-excluded and private path
//! removed and the record masked. Journal blobs never travel: journals
//! are data on another machine, never executed. Retention removes only
//! this machine's remote refs for checkpoints it pruned.

use std::collections::{BTreeMap, BTreeSet};

use eyre::Result;

use super::network::{MACHINES_PREFIX, PushOutcome, REMOTE_MACHINES_PREFIX, Remote};
use super::privacy::mask_checkpoint;
use crate::system::history::shadow::{HistoryRepo, Overlay};
use crate::system::history::store::{Entry, OperationStatus};
use crate::system::history::tracked::display_to_tree_path;

/// Whether a checkpoint may leave the machine at all: it has content and
/// at least one entry with `backup = true`.
pub(crate) fn eligible(entry: &Entry) -> bool {
    entry.checkpoint.tree.available
        && entry.checkpoint.status() != Some(OperationStatus::Pending)
        && entry
            .checkpoint
            .tree
            .coverage
            .entries
            .iter()
            .any(|coverage| coverage.backup && coverage.mode != "private")
}

/// The paths a backup must not carry.
fn excluded_paths(entry: &Entry) -> BTreeSet<String> {
    entry
        .checkpoint
        .tree
        .coverage
        .entries
        .iter()
        .filter(|coverage| !coverage.backup || coverage.mode == "private")
        .map(|coverage| coverage.path.clone())
        .collect()
}

/// A wrapper commit holding only what may be backed up.
pub(crate) fn filtered_commit(repo: &HistoryRepo, entry: &Entry) -> Result<String> {
    let excluded = excluded_paths(entry);
    let snapshot = match &entry.checkpoint.tree.snapshot {
        Some(snapshot) if excluded.is_empty() => Some(snapshot.clone()),
        Some(snapshot) => {
            let overlays: Vec<Overlay> = excluded
                .iter()
                .map(|path| Overlay {
                    path: display_to_tree_path(path),
                    object: None,
                })
                .collect();
            Some(repo.compose(snapshot, &overlays)?)
        }
        None => None,
    };
    let masked = mask_checkpoint(&entry.checkpoint, &excluded);
    // a commit of its own: the local checkpoint ref keeps naming the full
    // wrapper, private and unbacked files included
    repo.write_checkpoint_commit(snapshot.as_deref(), &masked, &BTreeMap::new())
}

pub(crate) fn remote_ref(machine_id: &str, uuid: &str) -> String {
    format!("{REMOTE_MACHINES_PREFIX}{machine_id}/{uuid}")
}

/// The mirrored remote ref of one of this machine's checkpoints, as the
/// sync's fetch left it: present means the origin holds the backup, whatever
/// the local record remembers.
fn mirrored(repo: &HistoryRepo, machine_id: &str, uuid: &str) -> Result<Option<String>> {
    repo.ref_oid(&format!("{MACHINES_PREFIX}{machine_id}/{uuid}"))
}

/// Uploads every eligible checkpoint the origin does not hold yet. Returns
/// how many were pushed. The origin's refs decide, not only the local record
/// (a lost `sync.json` must not re-push, and a backup already there is never
/// replaced): a checkpoint mirrored by the fetch is recorded as uploaded, and
/// one the origin refuses (a differing backup under the same name) is left as
/// it is on the origin and reported, while the others go on.
pub(crate) fn upload(
    remote: &Remote<'_>,
    repo: &HistoryRepo,
    entries: &[Entry],
    machine_id: &str,
    uploaded: &mut BTreeSet<String>,
) -> Result<usize> {
    let mut count = 0;
    for entry in entries {
        let uuid = &entry.checkpoint.uuid;
        if uploaded.contains(uuid) || !eligible(entry) {
            continue;
        }
        if mirrored(repo, machine_id, uuid)?.is_some() {
            uploaded.insert(uuid.clone());
            continue;
        }
        let commit = filtered_commit(repo, entry)?;
        let refspec = format!("{commit}:{}", remote_ref(machine_id, uuid));
        match remote.push(&[refspec], None)? {
            PushOutcome::Done => {
                uploaded.insert(uuid.clone());
                count += 1;
            }
            PushOutcome::Rejected(reason) => {
                warn!(
                    "history sync: the origin already holds a different backup of checkpoint {uuid}; keeping the origin's ({reason})"
                );
            }
        }
    }
    Ok(count)
}

/// Removes this machine's remote refs for checkpoints no longer retained
/// locally. Returns how many were deleted.
pub(crate) fn prune_remote(
    remote: &Remote<'_>,
    entries: &[Entry],
    machine_id: &str,
    uploaded: &mut BTreeSet<String>,
) -> Result<usize> {
    let retained: BTreeSet<&str> = entries
        .iter()
        .map(|entry| entry.checkpoint.uuid.as_str())
        .collect();
    let prefix = format!("{REMOTE_MACHINES_PREFIX}{machine_id}/");
    let stale: Vec<String> = remote
        .ls_remote()?
        .into_iter()
        .filter_map(|(_, name)| {
            let uuid = name.strip_prefix(&prefix)?;
            (!retained.contains(uuid)).then(|| name.clone())
        })
        .collect();
    remote.delete(&stale)?;
    for name in &stale {
        if let Some(uuid) = name.strip_prefix(&prefix) {
            uploaded.remove(uuid);
        }
    }
    Ok(stale.len())
}

/// Every one of this machine's remote refs, for `origin --purge`.
pub(crate) fn remote_refs(remote: &Remote<'_>, machine_id: &str) -> Result<Vec<String>> {
    let prefix = format!("{REMOTE_MACHINES_PREFIX}{machine_id}/");
    Ok(remote
        .ls_remote()?
        .into_iter()
        .filter(|(_, name)| name.starts_with(&prefix))
        .map(|(_, name)| name)
        .collect())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::system::history::checkpoint::test_checkpoint;
    use crate::system::history::store::CoverageEntry;

    fn repo(tmp: &std::path::Path) -> HistoryRepo {
        HistoryRepo::open_or_init_in(&tmp.join("state"))
            .unwrap()
            .expect("git is required for these tests")
    }

    fn origin(tmp: &std::path::Path) -> String {
        let path = tmp.join("origin.git");
        let status = Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        format!("file://{}", path.display())
    }

    fn origin_refs(url: &str) -> Vec<String> {
        let output = Command::new("git")
            .args(["ls-remote", "--quiet", url])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.split('\t').nth(1).map(str::to_string))
            .collect()
    }

    /// An eligible checkpoint: content, and one entry with `backup = true`.
    fn entry(repo: &HistoryRepo, uuid: &str) -> Entry {
        let blob = repo.hash_blob(b"alias ll='ls -l'\n").unwrap();
        let tree = repo
            .mktree(&format!("100644 blob {blob}\t.zshrc\n"))
            .unwrap();
        let mut checkpoint = test_checkpoint(uuid, Some(&tree));
        checkpoint.tree.coverage.entries.push(CoverageEntry {
            path: "~/.zshrc".into(),
            mode: "track".into(),
            variant: None,
            source: None,
            autosave: true,
            share: true,
            backup: true,
            state: "live".into(),
            promotion: None,
            private: None,
            declared_in: None,
        });
        let commit = repo
            .write_checkpoint_commit(Some(&tree), &checkpoint, &BTreeMap::new())
            .unwrap();
        Entry {
            id: 1,
            commit,
            checkpoint,
        }
    }

    #[test]
    fn a_backup_the_fetch_mirrored_is_recorded_not_pushed() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        let entry = entry(&repo, "u1");
        repo.update_ref(&format!("{MACHINES_PREFIX}m/u1"), &entry.commit, None)
            .unwrap();
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(&remote, &repo, &[entry], "m", &mut uploaded).unwrap(),
            0
        );
        assert_eq!(uploaded, BTreeSet::from(["u1".to_string()]));
        assert!(origin_refs(&url).is_empty());
    }

    #[test]
    fn a_forgotten_upload_is_found_on_the_origin_instead_of_pushed_again() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        let entry = entry(&repo, "u1");
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(
                &remote,
                &repo,
                std::slice::from_ref(&entry),
                "m",
                &mut uploaded
            )
            .unwrap(),
            1
        );
        assert_eq!(origin_refs(&url), vec![remote_ref("m", "u1")]);
        // the local record is lost; the next sync's fetch mirrors the ref
        uploaded.clear();
        remote.fetch("main").unwrap();
        assert_eq!(
            upload(&remote, &repo, &[entry], "m", &mut uploaded).unwrap(),
            0
        );
        assert_eq!(uploaded, BTreeSet::from(["u1".to_string()]));
        assert_eq!(origin_refs(&url), vec![remote_ref("m", "u1")]);
    }

    #[test]
    fn a_differing_backup_on_the_origin_is_kept() {
        if crate::git::plumbing_binary().is_none() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let repo = repo(tmp.path());
        let url = origin(tmp.path());
        let remote = Remote::new(&repo, &url);
        // another build of the same checkpoint is on the origin already
        let other = entry(&repo, "u1");
        let refspec = format!("{}:{}", other.commit, remote_ref("m", "u1"));
        assert_eq!(remote.push(&[refspec], None).unwrap(), PushOutcome::Done);
        let mut entry = entry(&repo, "u1");
        entry.checkpoint.description = "rebuilt".into();
        let mut uploaded = BTreeSet::new();
        assert_eq!(
            upload(
                &remote,
                &repo,
                std::slice::from_ref(&entry),
                "m",
                &mut uploaded
            )
            .unwrap(),
            0
        );
        assert!(uploaded.is_empty());
        let output = Command::new("git")
            .args(["ls-remote", "--quiet", &url, &remote_ref("m", "u1")])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&output.stdout).starts_with(&other.commit));
    }
}
