//! The persisted report of throttled paths, written by the watcher and
//! listed by `mise bootstrap dotfiles paths --noisy`. A noisy path is never
//! excluded automatically and never switched to manual-save; the report
//! says what the watcher stretched and lets the user decide.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NoisyRecord {
    #[serde(default)]
    pub paths: BTreeMap<String, NoisyPath>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct NoisyPath {
    /// The stretched autosave interval.
    pub interval_secs: u64,
    /// Changes seen since the last save.
    #[serde(default)]
    pub pending_changes: u32,
    pub last_seen: String,
}

pub(crate) fn read(path: &Path) -> NoisyRecord {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn write(path: &Path, record: &NoisyRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(record)?)
}
