//! Replaceable local synchronization bookkeeping, never a Git history.
//! These records describe the last application and pending reconciliation.

use std::collections::BTreeMap;

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::system::history::shadow::HistoryRepo;

fn path(repo: &HistoryRepo) -> std::path::PathBuf {
    repo.dir().join("mise-sync-state.json")
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SyncRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<super::reconcile::Object>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciled: Option<super::reconcile::Object>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<super::reconcile::Object>,
    /// The upstream commit `reconciled` came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_commit: Option<String>,
}

/// Per branch path.
pub(crate) type SyncState = BTreeMap<String, SyncRecord>;

/// Read the current operational state, not historical versions.
pub(crate) fn load(repo: &HistoryRepo) -> Result<SyncState> {
    match std::fs::read(path(repo)) {
        Ok(bytes) => serde_json::from_slice(&bytes).wrap_err_with(|| {
            format!(
                "cannot read synchronization bookkeeping at {}; preserve this file and repair its JSON before retrying (it may contain unapplied reconciliation state)",
                path(repo).display()
            )
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(SyncState::new()),
        Err(error) => Err(error.into()),
    }
}

/// Replace bookkeeping atomically; callers hold the synchronization lock.
pub(crate) fn save(repo: &HistoryRepo, state: &SyncState, _message: &str) -> Result<()> {
    crate::system::history::store::write_json(&path(repo), state)
}

pub(crate) fn clear(repo: &HistoryRepo) -> Result<()> {
    save(repo, &SyncState::new(), "disconnected")
}

impl SyncRecord {
    /// Whether the path was ever reconciled with upstream.
    pub(crate) fn is_new(&self) -> bool {
        self.acknowledged.is_none() && self.reconciled.is_none() && self.applied.is_none()
    }
}
