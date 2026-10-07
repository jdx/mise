//! Opt-in updates for tools configured in global config.
//!
//! A tool whose global `[tools]` entry sets `auto_update` is upgraded within
//! its configured version when a shim or `mise x` is about to launch it and
//! its check interval has elapsed. The upgrade runs in `mise __tool-update`;
//! this module decides which tool is eligible and when, and keeps the state
//! that rate-limits checks and reports failures to `mise doctor`.

use std::path::PathBuf;
use std::time::Duration;

use eyre::{Result, bail};

use crate::config::{Settings, SettingsExt, is_global_config};
use crate::toolset::{ToolOptionSource, ToolSource, ToolVersion, Toolset};
use crate::{dirs, duration, file, hash, lock_file};

const STATE_DIR: &str = "tool-update";

/// Shorter intervals are raised to this, so a misconfigured interval cannot
/// turn every launch into an update check.
pub const MIN_CHECK_DURATION: Duration = Duration::from_secs(60 * 60);

/// Parse an `auto_update` value: `false` disables it, `true` uses
/// `tool_update.check_duration`, and a duration sets the tool's own interval.
pub fn parse_auto_update(value: &str) -> Result<Option<Duration>> {
    match value {
        "false" => Ok(None),
        "true" => Ok(Some(Settings::get().tool_update_check_duration()?)),
        _ => match duration::parse_duration(value) {
            Ok(duration) => Ok(Some(duration)),
            Err(_) => bail!("expected true, false, or a duration, got {value:?}"),
        },
    }
}

/// The `auto_update` value of a request written in a global config file.
/// Options layered on from anywhere else (a runtime argument, an env var, a
/// project's tool alias, the registry) never count: a project must not be able
/// to make a user's commands download and install tools.
fn global_auto_update(tv: &ToolVersion) -> Option<String> {
    let ToolSource::MiseToml(path) = tv.request.source() else {
        return None;
    };
    // Options in the entry's own table are `InlineBackendArg`, and in its
    // version spec `Request`; both were written in this file.
    let from_entry = matches!(
        tv.request.option_source("auto_update"),
        Some(ToolOptionSource::Request | ToolOptionSource::InlineBackendArg)
    );
    if !from_entry || !is_global_config(path) {
        return None;
    }
    tv.request.options().get("auto_update").map(str::to_string)
}

/// Whether any tool in `toolset` opted in. This is in memory only, so a
/// launch can rule out updates before doing any lookup or I/O.
pub fn any_opted_in(toolset: &Toolset) -> bool {
    toolset
        .list_current_versions()
        .iter()
        .any(|(_, tv)| global_auto_update(tv).is_some())
}

/// Claim the update check for `tv` if it opted in, is not an exact version,
/// and its interval has elapsed, returning its tool id. The check is recorded
/// as made right away, so concurrent launches start one update and a failed or
/// offline update is not retried until the next interval.
pub fn claim_due(tv: &ToolVersion) -> Option<String> {
    let value = global_auto_update(tv)?;
    let settings = Settings::get();
    if tv.request_pinned_this_version()
        || settings.offline()
        || settings.prefer_offline
        || settings.locked
        || settings.ci
        || ci_info::is_ci()
    {
        return None;
    }
    let tool_id = tv.ba().full_without_opts();
    let interval = match parse_auto_update(&value) {
        Ok(interval) => interval?,
        Err(err) => {
            debug!("tool-update: {tool_id}: {err:#}");
            return None;
        }
    };
    match claim(&tool_id, interval) {
        Ok(true) => Some(tool_id),
        Ok(false) => None,
        Err(err) => {
            debug!("tool-update: {tool_id}: {err:#}");
            None
        }
    }
}

fn claim(tool_id: &str, interval: Duration) -> Result<bool> {
    let paths = StatePaths::new(tool_id);
    let Some(_claim) = lock_file::LockFile::at(&paths.claim).try_lock()? else {
        return Ok(false);
    };
    if file::modified_duration(&paths.marker)
        .is_ok_and(|age| age < interval.max(MIN_CHECK_DURATION))
    {
        return Ok(false);
    }
    file::write_atomic(&paths.marker, "")?;
    // Here rather than on every launch, so it shows at most once per interval.
    if interval < MIN_CHECK_DURATION {
        warn!("auto_update interval for {tool_id} is below the 1h minimum, using 1h instead");
    }
    Ok(true)
}

/// Take the lock every update holds, waiting for one already running. Updates
/// of different tools rewrite the same global lockfile, so they run one at a
/// time, each from config read after the previous one finished.
pub fn lock_for_update() -> Result<fslock::LockFile> {
    lock_file::LockFile::at(&state_dir().join("update.lock"))
        .with_pid()
        .lock()
}

/// Files under `$MISE_STATE_DIR/tool-update` for one tool.
struct StatePaths {
    /// Touched when a check is made; its age is the time since the last check.
    marker: PathBuf,
    /// Held while a launch checks and touches the marker.
    claim: PathBuf,
    /// The last update's error, removed when an update succeeds.
    failure: PathBuf,
}

impl StatePaths {
    fn new(tool_id: &str) -> Self {
        let dir = state_dir();
        let key = hash::hash_to_str(&tool_id);
        Self {
            marker: dir.join(&key),
            claim: dir.join(format!("{key}.claim")),
            failure: dir.join(format!("{key}.failed.json")),
        }
    }
}

fn state_dir() -> PathBuf {
    dirs::STATE.join(STATE_DIR)
}

/// The recorded failure of a tool's last update.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Failure {
    pub tool: String,
    pub error: String,
}

/// Record how the last update went, for `mise doctor`.
pub fn record_result(tool_id: &str, result: &Result<()>) {
    let path = StatePaths::new(tool_id).failure;
    let recorded = match result {
        Ok(()) if path.exists() => file::remove_file(&path),
        Ok(()) => Ok(()),
        Err(err) => {
            let failure = Failure {
                tool: tool_id.to_string(),
                error: format!("{err:#}"),
            };
            serde_json::to_string(&failure)
                .map_err(eyre::Report::from)
                .and_then(|json| file::write_atomic(&path, json))
        }
    };
    if let Err(err) = recorded {
        debug!("tool-update: could not record the result for {tool_id}: {err:#}");
    }
}

/// Failures recorded by updates that have not succeeded since.
pub fn failures() -> Vec<Failure> {
    let Ok(entries) = std::fs::read_dir(state_dir()) else {
        return vec![];
    };
    entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().ends_with(".failed.json"))
        .filter_map(|path| serde_json::from_str(&file::read_to_string(&path).ok()?).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{StatePaths, parse_auto_update};

    #[test]
    fn parses_auto_update_values() {
        assert_eq!(parse_auto_update("false").unwrap(), None);
        assert!(parse_auto_update("true").unwrap().is_some());
        assert_eq!(
            parse_auto_update("6h").unwrap(),
            Some(Duration::from_secs(6 * 60 * 60))
        );
        assert!(parse_auto_update("minor").is_err());
    }

    #[test]
    fn state_paths_are_distinct_per_tool() {
        let claude = StatePaths::new("claude");
        assert_eq!(claude.marker, StatePaths::new("claude").marker);
        assert_ne!(claude.marker, StatePaths::new("codex").marker);
        assert_ne!(claude.marker, claude.claim);
    }
}
