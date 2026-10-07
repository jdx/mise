//! Opt-in background updates for tools configured in global config.
//!
//! After a foreground command (`mise x`, a shim, or a task) has selected its
//! tool versions, [`schedule`] looks for the ones whose global `[tools]` entry
//! sets `auto_update`. For each whose check interval has elapsed it records the
//! attempt and starts a detached `mise __tool-update <tool>`, which upgrades the
//! tool within its configured request. The current launch keeps the version it
//! selected; later resolutions pick up the new one.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use eyre::{Result, bail};

use crate::config::{Settings, SettingsExt, is_global_config};
use crate::toolset::{ToolOptionSource, ToolSource, ToolVersion, Toolset};
use crate::{dirs, duration, env, file, hash, lock_file};

const STATE_DIR: &str = "tool-update";

/// Shorter intervals are raised to this, so a misconfigured interval cannot
/// start an updater on every shim call.
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
/// to start background network and install work on a user's machine.
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

/// Start detached updates for the opted-in tools in `toolset` that are due.
/// This runs on the shim path, so it returns before any I/O unless some
/// selected tool came from a global entry with `auto_update` set.
pub fn schedule(toolset: &Toolset) {
    let opted_in = toolset
        .list_current_versions()
        .into_iter()
        .filter_map(|(_, tv)| global_auto_update(&tv).map(|value| (tv, value)))
        .collect::<Vec<_>>();
    if opted_in.is_empty() {
        return;
    }
    let settings = Settings::get();
    if settings.offline()
        || settings.prefer_offline
        || settings.locked
        || settings.ci
        || ci_info::is_ci()
    {
        debug!("tool-update: skipped in offline, locked, or CI mode");
        return;
    }
    let mut seen = HashSet::new();
    for (tv, value) in opted_in {
        let tool_id = tv.ba().full_without_opts();
        if !seen.insert(tool_id.clone()) || tv.request_pinned_this_version() {
            continue;
        }
        let interval = match parse_auto_update(&value) {
            Ok(Some(interval)) => interval,
            Ok(None) => continue,
            Err(err) => {
                debug!("tool-update: {tool_id}: {err:#}");
                continue;
            }
        };
        if let Err(err) = schedule_one(&tv.ba().short, &tool_id, interval) {
            debug!("tool-update: {tool_id}: {err:#}");
        }
    }
}

fn schedule_one(tool: &str, tool_id: &str, interval: Duration) -> Result<()> {
    let paths = StatePaths::new(tool_id);
    // The claim makes check-and-mark atomic across concurrent foreground
    // commands, so only one of them starts an updater per interval.
    let Some(claim) = lock_file::LockFile::at(&paths.claim)
        .with_pid()
        .try_lock()?
    else {
        return Ok(());
    };
    if file::modified_duration(&paths.marker)
        .is_ok_and(|age| age < interval.max(MIN_CHECK_DURATION))
    {
        return Ok(());
    }
    file::write_atomic(&paths.marker, "")?;
    drop(claim);
    // Here rather than on every run, so it shows at most once per interval.
    if interval < MIN_CHECK_DURATION {
        warn!("auto_update interval for {tool_id} is below the 1h minimum, using 1h instead");
    }

    debug!("tool-update: starting background update for {tool_id}");
    let log = std::fs::File::create(&paths.log)?;
    let mut command = Command::new(&*env::MISE_BIN);
    command
        .args(["__tool-update", tool])
        // Run outside the project, so the updater never loads its config.
        .current_dir(*dirs::HOME)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    spawn_detached(&mut command)?;
    Ok(())
}

/// Files under `$MISE_STATE_DIR/tool-update` for one tool.
pub struct StatePaths {
    /// Touched when an update is started; its age is the time since the last check.
    pub marker: PathBuf,
    /// Held by a foreground command while it checks and touches the marker.
    pub claim: PathBuf,
    /// Held by the updater for the whole upgrade.
    pub lock: PathBuf,
    /// The last update's error, removed when an update succeeds.
    pub failure: PathBuf,
    /// The last update's output.
    pub log: PathBuf,
}

impl StatePaths {
    pub fn new(tool_id: &str) -> Self {
        let dir = state_dir();
        let key = hash::hash_to_str(&tool_id);
        Self {
            marker: dir.join(&key),
            claim: dir.join(format!("{key}.claim")),
            lock: dir.join(format!("{key}.lock")),
            failure: dir.join(format!("{key}.failed.json")),
            log: dir.join(format!("{key}.log")),
        }
    }
}

fn state_dir() -> PathBuf {
    dirs::STATE.join(STATE_DIR)
}

/// The recorded failure of a tool's last background update.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Failure {
    pub tool: String,
    pub error: String,
    pub log: PathBuf,
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
                log: StatePaths::new(tool_id).log,
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

/// Failures recorded by background updates that have not succeeded since.
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

/// Start `command` fully detached from the foreground process.
#[cfg(unix)]
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    // `mise x` and shims replace themselves with the tool through exec, which
    // would leave the updater a child of that tool: an unreaped zombie for as
    // long as it runs. Instead the forked child starts a new session and forks
    // again; the intermediate exits at once and is reaped below, and the
    // updater is adopted by init.
    unsafe {
        command.pre_exec(|| {
            use nix::libc;
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            match libc::fork() {
                -1 => Err(std::io::Error::last_os_error()),
                0 => Ok(()),
                _ => libc::_exit(0),
            }
        });
    }
    command.spawn()?.wait()?;
    Ok(())
}

#[cfg(windows)]
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

    // No console and its own process group, so Ctrl+C in the foreground does
    // not reach it. Windows has no zombies, so the handle can just be dropped.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    command.spawn()?;
    Ok(())
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
        assert_ne!(claude.claim, claude.lock);
    }
}
