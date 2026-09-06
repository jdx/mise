//! Background health, persisted under `$MISE_STATE_DIR/history/health.json`
//! by the watcher and read by `mise doctor` and `mise bootstrap dotfiles
//! status`. This is pull-based visibility: nothing here gets the user's
//! attention on its own. Readers distinguish stale information (the last
//! update is older than the watcher's reconcile period while a watcher
//! still holds the lock) from confirmed current health.

use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::store;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Health {
    /// When this record was written (RFC 3339).
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub watcher: WatcherHealth,
    /// Paths whose autosave interval is stretched by sustained churn.
    #[serde(default)]
    pub throttled: Vec<ThrottledPath>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct WatcherHealth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_capture: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reconcile: Option<String>,
    /// The last capture failure, with when it happened; cleared by the
    /// next successful capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_at: Option<String>,
    /// Consecutive capture failures.
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default)]
    pub degraded: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ThrottledPath {
    pub path: String,
    pub interval_secs: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_saved: Option<String>,
    /// Changes seen since the last save.
    /// Changes seen since the last save (events, not verified content
    /// differences).
    pub pending_changes: u32,
    /// The interval reached the heavy-throttling mark.
    pub heavy: bool,
}

pub(crate) fn path_in(state_dir: &Path) -> PathBuf {
    store::store_dir_in(state_dir).join("health.json")
}

pub(crate) fn read(state_dir: &Path) -> Option<Health> {
    let text = std::fs::read_to_string(path_in(state_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn write(state_dir: &Path, health: &mut Health) -> Result<()> {
    health.updated_at = store::now_rfc3339();
    store::write_json(&path_in(state_dir), health)
}

/// How old a record is, in seconds, if its timestamp parses.
pub(crate) fn age_secs(health: &Health) -> Option<u64> {
    let updated = chrono::DateTime::parse_from_rfc3339(&health.updated_at).ok()?;
    let age = chrono::Utc::now().signed_duration_since(updated);
    u64::try_from(age.num_seconds()).ok()
}
