//! Durable per-path sync state, recorded in git under `refs/sync/state`:
//! for every `(branch path)` the three versions the transition table
//! reasons about, so a crash between a push and its bookkeeping, or a
//! rebuilt index, never republishes stale content.
//!
//! - **acknowledged** (A): the local saved version most recently folded
//!   into upstream by this machine (published, or adopted as present);
//! - **reconciled** (U): the upstream version that fold produced, the last
//!   one this machine reconciled against;
//! - **applied** (L): the version mise last wrote to the live file (or
//!   observed there at adoption).
//!
//! Each is a blob id or `None` for a known absence.

use std::collections::BTreeMap;

use eyre::Result;
use serde::{Deserialize, Serialize};

use crate::system::history::shadow::{HistoryRepo, Overlay};

pub(crate) const STATE_REF: &str = "refs/sync/state";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SyncRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciled: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<String>,
    /// The upstream commit `reconciled` came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_commit: Option<String>,
}

/// Per branch path.
pub(crate) type SyncState = BTreeMap<String, SyncRecord>;

fn record_path(branch_path: &str) -> String {
    format!("state/{branch_path}.json")
}

/// Reads every record from the state ref's tree.
pub(crate) fn load(repo: &HistoryRepo) -> Result<SyncState> {
    let mut state = SyncState::new();
    let Some(head) = repo.ref_oid(STATE_REF)? else {
        return Ok(state);
    };
    // a commit written with nothing to record has no `state/` directory;
    // anything else that fails to read is an error, never an empty state
    if repo.object_at(&head, "state")?.is_none() {
        return Ok(state);
    }
    for entry in repo.ls_tree(&format!("{head}:state"))? {
        let Some(branch_path) = entry.path.strip_suffix(".json") else {
            continue;
        };
        let Some((_, oid)) = repo.object_at(&head, &record_path(branch_path))? else {
            continue;
        };
        let record: SyncRecord = serde_json::from_slice(&repo.cat_object(&oid)?)?;
        state.insert(branch_path.to_string(), record);
    }
    Ok(state)
}

/// Writes the whole state as a new commit on the state ref (parent: the
/// previous one), so the history of sync decisions is inspectable.
pub(crate) fn save(repo: &HistoryRepo, state: &SyncState, message: &str) -> Result<()> {
    let base = repo.empty_object("tree")?;
    let mut overlays = vec![];
    for (branch_path, record) in state {
        let bytes = serde_json::to_vec_pretty(record)?;
        let oid = repo.hash_blob(&bytes)?;
        overlays.push(Overlay {
            path: record_path(branch_path),
            object: Some(("100644".into(), oid)),
        });
    }
    let tree = repo.compose(&base, &overlays)?;
    let previous = repo.ref_oid(STATE_REF)?;
    let commit = repo.commit_tree(&tree, previous.as_deref().into_iter().collect(), message)?;
    repo.update_ref(STATE_REF, &commit, previous.as_deref())?;
    Ok(())
}

impl SyncRecord {
    /// Whether the path was ever reconciled with upstream.
    pub(crate) fn is_new(&self) -> bool {
        self.acknowledged.is_none() && self.reconciled.is_none() && self.applied.is_none()
    }
}
