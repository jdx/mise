//! Opt-in scheduling for background tool upgrades.
//!
//! The scheduler runs only after a foreground command has selected its tool
//! paths. It does one filesystem-only rate-limit check, records a due attempt,
//! and then detaches an internal command. The detached command owns all
//! network, resolution, installation, and per-tool exclusion locking.

use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;

use eyre::Result;

use crate::args::BackendArg;
use crate::config::{Config, Settings, SettingsExt};
use crate::toolset::{ConfigScope, ToolRequest, Toolset, ToolsetBuilder};
use crate::{dirs, env, file, hash, lock_file};

pub const STATE_DIR: &str = "tool-update";

/// The largest component boundary a background update may cross.
///
/// Boundaries are intentionally based on numeric components, not a SemVer
/// parser: backends may use dates, vendor prefixes, or other version syntaxes.
/// `minor` keeps the first numeric component and `patch` keeps the first two.
/// If there are no numeric components, the configured request stays
/// authoritative instead of making that tool silently ineligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdatePolicy {
    Major,
    Minor,
    Patch,
}

impl UpdatePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }

    /// Return a version selector that constrains candidate lookup before a
    /// backend chooses its newest match. `None` means an opaque current
    /// version has no safe numeric boundary, so the normal configured request
    /// remains authoritative.
    pub fn candidate_selector(self, current: &str) -> Option<String> {
        if self == Self::Major {
            return Some("latest".into());
        }

        let component_count = match self {
            Self::Major => unreachable!(),
            Self::Minor => 1,
            Self::Patch => 2,
        };
        numeric_component_selector(current, component_count)
    }
}

/// Derive a prefix query from the first `component_count` numeric components.
/// Separators deliberately include dots, dashes, and underscores: dates such
/// as `2026-10-06` need the same bounded behavior as `1.2.3`, and names like
/// `go1.23.4` keep their meaningful nonnumeric prefix.
fn numeric_component_selector(version: &str, component_count: usize) -> Option<String> {
    let mut index = version.find(|character: char| character.is_ascii_digit())?;
    for component in 0..component_count {
        let start = index;
        while version
            .as_bytes()
            .get(index)
            .is_some_and(|character| character.is_ascii_digit())
        {
            index += 1;
        }
        if index == start {
            return None;
        }
        if component + 1 == component_count {
            return Some(version[..index].to_string());
        }
        if !version
            .as_bytes()
            .get(index)
            .is_some_and(|character| matches!(character, b'.' | b'-' | b'_'))
        {
            return None;
        }
        index += 1;
        if !version
            .as_bytes()
            .get(index)
            .is_some_and(|character| character.is_ascii_digit())
        {
            return None;
        }
    }
    None
}

impl std::fmt::Display for UpdatePolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for UpdatePolicy {
    type Err = eyre::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            "patch" => Ok(Self::Patch),
            _ => eyre::bail!("expected one of: major, minor, patch"),
        }
    }
}

/// Starts detached, per-tool update attempts after the foreground path has
/// already selected its current installed versions. This path deliberately
/// does no network I/O, installation, waiting, or process locking.
pub fn schedule(config: &Arc<Config>, toolset: &Toolset) {
    let settings = Settings::get();
    // `prefer_offline()` also reports the foreground command's fast-path
    // policy (`mise x`, shims, and hook-env). This scheduler runs only from
    // paths that have already selected an installed version, then launches a
    // detached child whose own command is allowed to resolve normally. Honor
    // only an explicit configured preference here.
    if settings.offline() || settings.prefer_offline || settings.ci || ci_info::is_ci() {
        debug!(
            "skipping background update scheduling in offline or CI mode: offline={}, prefer_offline={}, configured_ci={}, detected_ci={}",
            settings.offline(),
            settings.prefer_offline,
            settings.ci,
            ci_info::is_ci(),
        );
        return;
    }
    let check_duration = match settings.tool_update_check_duration() {
        Ok(duration) => duration,
        Err(err) => {
            debug!("invalid tool-update check duration: {err:#}");
            return;
        }
    };

    let mut scheduled = HashSet::new();
    for (_, tool_version) in toolset.list_current_versions() {
        let tool = tool_version.ba();
        let tool_id = tool.full_without_opts();
        if tool_version.request_pinned_this_version() {
            debug!("skipping background update for {tool_id}: request is an exact pin");
            continue;
        }
        if !scheduled.insert(tool_id.clone()) {
            continue;
        }
        let Ok(backend) = tool_version.backend() else {
            debug!("skipping background update for {tool_id}: backend is unavailable");
            continue;
        };
        if !backend.is_version_installed(config, &tool_version, true) {
            debug!("skipping background update for {tool_id}: selected version is not installed");
            continue;
        }
        let Some(policy) = global_update_policy(config, tool) else {
            debug!("skipping background update for {tool_id}: no global auto_update option");
            continue;
        };
        match request_has_lockfile(config, &tool_version.request) {
            Ok(true) => {
                debug!("skipping background update for {tool_id}: request is lockfile-bound");
                continue;
            }
            Ok(false) => {}
            Err(err) => {
                debug!(
                    "skipping background update for {tool_id}: could not inspect lockfile: {err:#}"
                );
                continue;
            }
        }

        let state_dir = dirs::STATE.join(STATE_DIR);
        let key = tool_update_key(&tool_id);
        let marker = state_dir.join(&key);
        let claim_path = state_dir.join(format!("{key}.lock"));
        let claim = match lock_file::LockFile::at(&claim_path).with_pid().try_lock() {
            Ok(Some(claim)) => claim,
            Ok(None) => continue,
            Err(err) => {
                debug!("failed to claim background update for {tool_id}: {err:#}");
                continue;
            }
        };
        if !update_check_due(&marker, check_duration) {
            continue;
        }

        // Claim the due interval before forking. The short-lived parent lock
        // makes check-and-mark atomic across concurrent foreground commands,
        // so they do not all create children that merely discover the child
        // lock. A failed spawn is still a failed attempt and is retried at the
        // next interval without ever delaying the foreground command.
        if let Err(err) = file::write_atomic(&marker, "") {
            debug!("failed to record background update attempt for {tool_id}: {err:#}");
            continue;
        }

        let mut command = Command::new(&*env::MISE_BIN);
        command
            .arg("__tool-update")
            // `tool_id` is mise's resolved backend identity (for example
            // `asdf:dummy`). The internal command accepts a normal ToolArg,
            // so it must receive the spelling from the configuration instead
            // of feeding that resolved identifier through the parser again.
            .arg(&tool.short)
            .arg("--current")
            .arg(&tool_version.version)
            .arg("--policy")
            .arg(policy.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Err(err) = spawn_detached(&mut command) {
            debug!("failed to start background update for {tool_id}: {err}");
        }
        drop(claim);
    }
}

/// Obtain the option only from the user's global config layer. Local project
/// config can still select a floating version, but can never turn background
/// installation on or change its update boundary.
fn global_update_policy(config: &Arc<Config>, tool: &BackendArg) -> Option<UpdatePolicy> {
    let global = ToolsetBuilder::new()
        .with_scope(ConfigScope::GlobalOnly)
        .build_unresolved(config)
        .ok()?;
    let exact = global
        .versions
        .iter()
        .filter(|(candidate, _)| candidate.full_without_opts() == tool.full_without_opts());
    let short = global
        .versions
        .iter()
        .filter(|(candidate, _)| candidate.short == tool.short);
    exact
        .chain(short)
        .flat_map(|(_, versions)| versions.requests.iter())
        .find_map(|request| request.options().get("auto_update").map(str::to_string))
        .and_then(|value| match value.parse() {
            Ok(policy) => Some(policy),
            Err(err) => {
                warn!(
                    "ignoring invalid auto_update option {value:?} for {}: {err:#}",
                    tool.full_without_opts()
                );
                None
            }
        })
}

/// True only when this exact request has a matching lockfile binding. An
/// unrelated entry in the same lockfile must not disable updates for every
/// tool, and a matching binding must never be rewritten by the updater.
pub fn request_has_lockfile(config: &Config, request: &ToolRequest) -> Result<bool> {
    Ok(request.lockfile_resolve(config)?.is_some())
}

pub async fn tool_has_lockfile(config: &Arc<Config>, tool_id: &str) -> Result<bool> {
    let requests = config.get_tool_request_set().await?;
    for (_, requests, _) in requests.iter() {
        for request in requests {
            if request.ba().full_without_opts() == tool_id && request_has_lockfile(config, request)?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn update_check_due(path: &std::path::Path, duration: std::time::Duration) -> bool {
    file::modified_duration(path).map_or(true, |age| age >= duration)
}

/// Stable filename component for a backend's background-update state.
pub fn tool_update_key(tool_id: &str) -> String {
    hash::hash_to_str(&tool_id)
}

#[cfg(unix)]
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    // `setsid` is async-signal-safe and releases the updater from the shell's
    // process group before it reaches exec. Its stdio is already /dev/null.
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid()
                .map(|_| ())
                .map_err(|err| std::io::Error::from_raw_os_error(err as i32))
        });
    }
    command.spawn().map(|_| ())
}

#[cfg(windows)]
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::{CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS};

    // A detached console plus a new process group keeps the updater outside
    // the foreground shim/terminal group. Do not use the kill-on-drop job used
    // for bounded child commands: this updater is intentionally independent.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    command.spawn().map(|_| ())
}

#[cfg(not(any(unix, windows)))]
fn spawn_detached(command: &mut Command) -> std::io::Result<()> {
    command.spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{UpdatePolicy, tool_update_key, update_check_due};

    #[test]
    fn tool_key_is_stable_and_distinct() {
        assert_eq!(tool_update_key("claude"), tool_update_key("claude"));
        assert_ne!(tool_update_key("claude"), tool_update_key("codex"));
    }

    #[test]
    fn policies_create_prefix_queries_without_semver_parsing() {
        assert_eq!(
            UpdatePolicy::Major.candidate_selector("1.2.3"),
            Some("latest".into())
        );
        assert_eq!(
            UpdatePolicy::Minor.candidate_selector("1.2.3"),
            Some("1".into())
        );
        assert_eq!(
            UpdatePolicy::Patch.candidate_selector("v1.2.3"),
            Some("v1.2".into())
        );
        assert_eq!(
            UpdatePolicy::Minor.candidate_selector("2026-10-06"),
            Some("2026".into())
        );
        assert_eq!(
            UpdatePolicy::Patch.candidate_selector("2026-10-06"),
            Some("2026-10".into())
        );
        assert_eq!(
            UpdatePolicy::Minor.candidate_selector("go1.23.4"),
            Some("go1".into())
        );
        assert_eq!(
            UpdatePolicy::Patch.candidate_selector("go1.23.4"),
            Some("go1.23".into())
        );
        assert_eq!(
            UpdatePolicy::Patch.candidate_selector("cpython-3.13.1"),
            Some("cpython-3.13".into())
        );
        assert_eq!(UpdatePolicy::Minor.candidate_selector("nightly"), None);
    }

    #[test]
    fn missing_check_is_due_and_fresh_check_is_not() {
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("last-check");
        assert!(update_check_due(&marker, Duration::from_secs(1)));
        std::fs::write(&marker, "").unwrap();
        assert!(!update_check_due(&marker, Duration::from_secs(3600)));
    }
}
