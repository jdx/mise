//! Opt-in updates for tools configured in global config.
//!
//! A tool whose global `[tools]` entry sets `auto_update` is upgraded within
//! its configured version when its check interval has elapsed: by the
//! `tool-update` service when it is running, otherwise when a shim or `mise x`
//! is about to launch it. The upgrade runs in `mise __tool-update`; this module
//! decides which tool is eligible and when, and keeps the state that
//! rate-limits checks and reports failures to `mise doctor`.

use std::path::PathBuf;
use std::time::Duration;

use eyre::{Result, bail};

use crate::config::{Settings, SettingsExt, is_global_config};
use crate::toolset::{ToolOptionSource, ToolRequest, ToolSource, ToolVersion, Toolset};
use crate::{dirs, duration, file, hash, lock_file};

const STATE_DIR: &str = "tool-update";

/// Set in `mise __tool-update`'s environment. A hook of that upgrade that
/// launches another opted-in tool must not start a nested update: it would
/// wait for the update lock its own updater holds.
pub const UPDATING_ENV: &str = "__MISE_TOOL_UPDATE";

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
fn global_auto_update(request: &ToolRequest) -> Option<String> {
    let ToolSource::MiseToml(path) = request.source() else {
        return None;
    };
    // Options in the entry's own table are `InlineBackendArg`, and in its
    // version spec `Request`; both were written in this file.
    let from_entry = matches!(
        request.option_source("auto_update"),
        Some(ToolOptionSource::Request | ToolOptionSource::InlineBackendArg)
    );
    if !from_entry || !is_global_config(path) {
        return None;
    }
    request.options().get("auto_update").map(str::to_string)
}

/// Whether `request`, from a global config file, has `auto_update` enabled.
pub fn enabled(request: &ToolRequest) -> bool {
    global_auto_update(request)
        .and_then(|value| parse_auto_update(&value).ok().flatten())
        .is_some()
}

/// Whether any tool in `toolset` opted in. This is in memory only, so a
/// launch can rule out updates before doing any lookup or I/O.
pub fn any_opted_in(toolset: &Toolset) -> bool {
    toolset
        .list_current_versions()
        .iter()
        .any(|(_, tv)| global_auto_update(&tv.request).is_some())
}

/// Who is asking to update a tool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Updater {
    /// A shim or `mise x` about to launch the tool; it leaves updates to the
    /// service while one runs.
    Launch,
    /// The `tool-update` service.
    Service,
}

/// Claim the update check for `tv` if it opted in, is not an exact version,
/// and its interval has elapsed, returning its tool id. The check is recorded
/// as made right away, so concurrent claims start one update and a failed or
/// offline update is not retried until the next interval.
pub fn claim_due(tv: &ToolVersion, updater: Updater) -> Option<String> {
    if !updatable(tv) {
        return None;
    }
    claim_due_request(&tv.request, updater)
}

/// The check interval `request`'s `auto_update` asks for, if it enables it.
pub fn interval(request: &ToolRequest) -> Option<Duration> {
    global_auto_update(request).and_then(|value| parse_auto_update(&value).ok().flatten())
}

/// Whether `tv` can move to a newer version: exact pins can't.
pub fn updatable(tv: &ToolVersion) -> bool {
    !tv.request_pinned_this_version()
}

/// [`claim_due`] for a request that couldn't be resolved, so a lookup failure
/// is reported once per interval like an update's, not on every pass.
pub fn claim_due_request(request: &ToolRequest, updater: Updater) -> Option<String> {
    let (tool_id, interval) = eligible(request, updater)?;
    match claim(&tool_id, interval) {
        Ok(true) => Some(tool_id),
        Ok(false) => None,
        Err(err) => {
            debug!("tool-update: {tool_id}: {err:#}");
            None
        }
    }
}

/// Whether [`claim_due_request`] would claim `request` now, without claiming
/// it: lets the service skip version lookups that can't lead to an update.
pub fn is_due(request: &ToolRequest, updater: Updater) -> bool {
    eligible(request, updater)
        .is_some_and(|(tool_id, interval)| !checked_within(&tool_id, interval))
}

/// The tool id and check interval of a request that may be updated now, apart
/// from whether its interval has elapsed.
fn eligible(request: &ToolRequest, updater: Updater) -> Option<(String, Duration)> {
    if updater == Updater::Launch && std::env::var_os(UPDATING_ENV).is_some() {
        return None;
    }
    let value = global_auto_update(request)?;
    let settings = Settings::get();
    // Only what describes this machine right now. Settings about a project's
    // lockfile or remote lookups don't apply: the update runs on global config
    // alone, and checks the global `locked` setting itself.
    if settings.offline() || settings.ci || ci_info::is_ci() || locked_by_command() {
        return None;
    }
    if updater == Updater::Launch && service_running() {
        return None;
    }
    let tool_id = request.ba().full_without_opts();
    match parse_auto_update(&value) {
        Ok(interval) => Some((tool_id, interval?)),
        Err(err) => {
            debug!("tool-update: {tool_id}: {err:#}");
            None
        }
    }
}

/// Whether `tool_id` was checked within `interval` (at least an hour).
fn checked_within(tool_id: &str, interval: Duration) -> bool {
    file::modified_duration(&StatePaths::new(tool_id).marker)
        .is_ok_and(|age| age < interval.max(MIN_CHECK_DURATION))
}

/// Whether the launching command itself asked for `--locked` (or
/// `MISE_LOCKED`). A project's `locked` setting doesn't stop global updates,
/// and the updater checks the global one; but the updater can't see the flag.
fn locked_by_command() -> bool {
    crate::env::var_is_true("MISE_LOCKED")
        || !*crate::env::IS_RUNNING_AS_SHIM
            && crate::env::ARGS
                .read()
                .unwrap()
                .iter()
                .take_while(|arg| *arg != "--")
                .any(|arg| arg == "--locked")
}

fn claim(tool_id: &str, interval: Duration) -> Result<bool> {
    let paths = StatePaths::new(tool_id);
    let Some(_claim) = lock_file::LockFile::at(&paths.claim).try_lock()? else {
        return Ok(false);
    };
    if checked_within(tool_id, interval) {
        return Ok(false);
    }
    file::write_atomic(&paths.marker, "")?;
    // Here rather than on every launch, so it shows at most once per interval.
    if interval < MIN_CHECK_DURATION {
        warn!("auto_update interval for {tool_id} is below the 1h minimum, using 1h instead");
    }
    Ok(true)
}

/// Whether the `tool-update` service is running: it holds this lock for as
/// long as it runs.
fn service_running() -> bool {
    matches!(
        lock_file::LockFile::at(&service_lock_path()).try_lock(),
        Ok(None)
    )
}

/// Take the service lock, waiting while another service holds it. Waiting,
/// not giving up, so a launch briefly checking the lock can't make a starting
/// service think another one is running.
pub fn lock_service() -> Result<fslock::LockFile> {
    lock_file::LockFile::at(&service_lock_path())
        .with_pid()
        .lock()
}

/// One service lock per global config directory and set of active
/// environments (`MISE_ENV`): a watcher checks only those global files, so a
/// launch using others must not leave its updates to it.
fn service_lock_path() -> PathBuf {
    let scope = (dirs::CONFIG.to_path_buf(), crate::env::mise_env().join(","));
    state_dir().join(format!("service-{}.lock", hash::hash_to_str(&scope)))
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

/// One run of the `tool-update` service's update pass (`mise __tool-update
/// --due`), started in its own process group (a job object on Windows) so a
/// timeout or shutdown stops everything it started, hooks and downloads too.
pub struct Tick {
    child: std::process::Child,
    #[cfg(windows)]
    job: crate::windows_job::Job,
    done: bool,
}

impl Tick {
    pub fn start(mut command: std::process::Command) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Nested mise (installers, hooks) skip their own process group
            // when this is set, so killing the pass's group reaches them.
            command.env("MISE_TASK_PGID_MANAGED", "1").process_group(0);
            Ok(Self {
                child: command.spawn()?,
                done: false,
            })
        }
        #[cfg(windows)]
        {
            let (child, job) = crate::windows_job::spawn(&mut command, 0)?;
            Ok(Self {
                child,
                job,
                done: false,
            })
        }
    }

    /// Wait for the pass, stopping it once it runs longer than `timeout`.
    pub async fn wait(&mut self, timeout: Duration) -> Result<()> {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.done = true;
                if !status.success() {
                    bail!("updating due tools failed ({status})");
                }
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                self.kill();
                bail!("updating due tools took longer than {timeout:?}; stopped it");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Stop the pass and everything it started.
    pub fn kill(&mut self) {
        if self.done {
            return;
        }
        self.done = true;
        #[cfg(unix)]
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(self.child.id() as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
        #[cfg(windows)]
        self.job.kill();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A pass the watcher stops waiting for (its future dropped, say) must not
/// keep running on its own.
impl Drop for Tick {
    fn drop(&mut self) {
        self.kill();
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

/// Failures recorded by updates that have not succeeded since, for tools whose
/// global config still enables `auto_update`. `global` is the global-only
/// toolset without environment overrides: a tool that no longer opts in will
/// never update again to clear its failure, and a shell's `MISE_<TOOL>_VERSION`
/// must not hide one.
pub fn failures(global: &Toolset) -> Vec<Failure> {
    let opted_in = global
        .versions
        .iter()
        .filter(|(_, versions)| versions.requests.iter().any(enabled))
        .map(|(ba, _)| ba.full_without_opts())
        .collect::<std::collections::HashSet<_>>();
    recorded_failures()
        .into_iter()
        .filter(|failure| opted_in.contains(&failure.tool))
        .collect()
}

fn recorded_failures() -> Vec<Failure> {
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
