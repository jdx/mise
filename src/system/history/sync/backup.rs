//! Machine recovery refs: this machine's checkpoints pushed as
//! parentless wrapper commits to `refs/mise-history/<machine-id>/<uuid>`
//! on the origin, rebuilt with every policy-excluded and private path
//! removed and the record masked. Journal blobs never travel: journals
//! are data on another machine, never executed. Retention removes only
//! this machine's remote refs for checkpoints it pruned.

use std::collections::{BTreeMap, BTreeSet};

use eyre::Result;

use super::network::{PushOutcome, REMOTE_MACHINES_PREFIX, Remote};
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

/// Uploads every eligible checkpoint not yet uploaded. Returns how many.
pub(crate) fn upload(
    remote: &Remote<'_>,
    repo: &HistoryRepo,
    entries: &[Entry],
    machine_id: &str,
    uploaded: &mut BTreeSet<String>,
) -> Result<usize> {
    let mut count = 0;
    for entry in entries {
        if uploaded.contains(&entry.checkpoint.uuid) || !eligible(entry) {
            continue;
        }
        let commit = filtered_commit(repo, entry)?;
        let refspec = format!(
            "{commit}:{}",
            remote_ref(machine_id, &entry.checkpoint.uuid)
        );
        match remote.push(&[refspec], None)? {
            PushOutcome::Done => {
                uploaded.insert(entry.checkpoint.uuid.clone());
                count += 1;
            }
            PushOutcome::Rejected(reason) => {
                eyre::bail!("uploading checkpoint {}: {reason}", entry.checkpoint.uuid)
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
