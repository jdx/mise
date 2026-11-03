//! Background health, persisted under `$MISE_STATE_DIR/history/health.json`
//! by the watcher and read by `mise doctor` and `mise dot
//! status`. This is pull-based visibility: nothing here gets the user's
//! attention on its own. Readers distinguish stale information (the last
//! update is older than the watcher's reconcile period while a watcher
//! still holds the lock) from confirmed current health.

use std::path::{Path, PathBuf};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::store;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Health {
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
pub struct WatcherHealth {
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
    /// The mise executable this watcher started from is gone, so the
    /// process runs old code that only a service restart replaces. Not a
    /// degraded watch: captures still run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub executable_gone: bool,
}

/// What to tell the user when `WatcherHealth::executable_gone` is set.
pub const EXECUTABLE_GONE_ADVICE: &str = "the mise executable this watcher runs from is gone, so it keeps running the old version; run `mise bootstrap services apply` to restart it on the installed one";

/// What to tell the user when a watcher predates the executable replacement
/// check and cannot read a newer enrollment record.
pub const STALE_WATCHER_SCHEMA_ADVICE: &str = "the history watcher is running an older mise that cannot read current dotfile tracking metadata; run `mise bootstrap services apply` to restart it on the installed version";

/// Whether a capture error came from a watcher whose enrollment schema
/// predates tracked-entry exclusions, includes, and plaintext selection.
///
/// This is deliberately narrower than every serde unknown-field error: a
/// future manifest must remain a capture failure, not an invitation to restart
/// a healthy current watcher. The old enrollment record had exactly these four
/// fields, so its error is enough to identify the recovery path.
pub fn is_stale_watcher_schema_error(error: &str) -> bool {
    let legacy_enrollment = ["path", "autosave", "encrypt", "variants"];
    let newer_field = ["exclude", "include", "allow_plaintext"];
    let unknown_field = |field: &str| {
        [
            format!("unknown field `{field}`"),
            format!("unknown field '{field}'"),
            format!("unknown field \"{field}\""),
        ]
        .iter()
        .any(|prefix| error.contains(prefix))
    };
    error.contains("unknown field")
        && error.contains("expected one of")
        && legacy_enrollment.iter().all(|field| error.contains(field))
        && newer_field.iter().any(|field| unknown_field(field))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThrottledPath {
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

impl Health {
    /// The last capture failure of the watcher that wrote this record, if the
    /// capture after it has not succeeded. A watcher starts from the previous
    /// one's record, so an error from before this run started is inherited,
    /// not this watcher's failure.
    pub fn failing_capture(&self) -> Option<&str> {
        let watcher = &self.watcher;
        let error = watcher.last_error.as_deref()?;
        let parse = |at: &Option<String>| {
            at.as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        };
        match (parse(&watcher.last_error_at), parse(&watcher.started_at)) {
            (Some(failed), Some(started)) if failed < started => None,
            _ => Some(error),
        }
    }
}

pub(crate) fn path_in(state_dir: &Path) -> PathBuf {
    store::store_dir_in(state_dir).join("health.json")
}

pub fn read(state_dir: &Path) -> Option<Health> {
    let text = std::fs::read_to_string(path_in(state_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn write(state_dir: &Path, health: &mut Health) -> Result<()> {
    health.updated_at = store::now_rfc3339();
    store::write_json(&path_in(state_dir), health)
}

/// How old a record is, in seconds, if its timestamp parses.
pub fn age_secs(health: &Health) -> Option<u64> {
    let updated = chrono::DateTime::parse_from_rfc3339(&health.updated_at).ok()?;
    let age = chrono::Utc::now().signed_duration_since(updated);
    u64::try_from(age.num_seconds()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(started: Option<&str>, failed: Option<&str>) -> Health {
        Health {
            watcher: WatcherHealth {
                started_at: started.map(str::to_string),
                last_error: failed.map(|_| "boom".to_string()),
                last_error_at: failed.map(str::to_string),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn an_error_from_before_this_watcher_started_is_not_its_failure() {
        let started = "2026-09-29T22:10:00+00:00";
        let earlier = "2026-09-29T22:05:00+00:00";
        let later = "2026-09-29T22:15:00+00:00";
        assert_eq!(health(Some(started), Some(earlier)).failing_capture(), None);
        assert_eq!(
            health(Some(started), Some(later)).failing_capture(),
            Some("boom")
        );
        // without a start time to compare to, the error is reported
        assert_eq!(health(None, Some(earlier)).failing_capture(), Some("boom"));
        assert_eq!(health(Some(started), None).failing_capture(), None);
    }

    #[test]
    fn identifies_a_watcher_that_cannot_read_newer_enrollment_fields() {
        for field in ["exclude", "include", "allow_plaintext"] {
            let error = format!(
                "unknown field `{field}`, expected one of `path`, `autosave`, `encrypt`, `variants` at line 79 column 15"
            );
            assert!(is_stale_watcher_schema_error(&error), "{error}");
        }
        assert!(is_stale_watcher_schema_error(
            "unknown field \"exclude\", expected one of `path`, `autosave`, `encrypt`, `variants`"
        ));

        assert!(!is_stale_watcher_schema_error(
            "unknown field `future_field`, expected one of `path`, `autosave`, `encrypt`, `variants` at line 79 column 15"
        ));
        assert!(!is_stale_watcher_schema_error(
            "unknown field `future_field`, expected one of `path`, `autosave`, `encrypt`, `variants`; an include list was configured"
        ));
        assert!(!is_stale_watcher_schema_error(
            "unknown field `exclude`, expected one of `format`, `enrollment`, `recipients` at line 1 column 1"
        ));
    }
}
