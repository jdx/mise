//! Opt-in scheduling for background tool upgrades.
//!
//! This lives in core rather than the CLI so both `mise x` and task execution
//! can schedule an update only after they have fixed the foreground tool paths.

use std::collections::HashSet;
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::Arc;

use eyre::Result;
use semver::Version;

use crate::args::BackendArg;
use crate::config::{Config, Settings, SettingsExt};
use crate::toolset::{ToolRequest, Toolset};
use crate::{env, hash, lockfile};

/// Starts detached, per-tool update attempts after the foreground path has
/// already selected its current installed versions. This path deliberately
/// does no network I/O, installation, waiting, or locking, keeping the caller
/// on its normal foreground path.
pub fn schedule(config: &Arc<Config>, toolset: &Toolset) {
    let settings = Settings::get();
    if settings
        .tool_update
        .tools
        .as_ref()
        .is_none_or(|tools| tools.is_empty())
        || settings.offline()
        || settings.prefer_offline()
        || settings.ci
        || ci_info::is_ci()
    {
        return;
    }

    let mut scheduled = HashSet::new();
    for (_, tool_version) in toolset.list_current_versions() {
        let tool = tool_version.ba();
        if tool_version.request_pinned_this_version()
            || update_policy(&settings, tool).is_none()
            || request_has_lockfile(config, &tool_version.request)
        {
            continue;
        }

        // A tool can have several configured selectors. One detached updater
        // receives the normal `mise upgrade <tool>` behavior for all of them.
        let tool_id = tool.full_without_opts();
        if !scheduled.insert(tool_id.clone()) {
            continue;
        }

        let result = Command::new(&*env::MISE_BIN)
            .arg("__tool-update")
            .arg(&tool_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        if let Err(err) = result {
            debug!("failed to start background update for {tool_id}: {err}");
        }
    }
}

/// The maximum semantic-version change a background update may make.
///
/// The configured request remains the primary constraint. This policy is an
/// additional filter applied after normal backend, registry/mirror, range,
/// and minimum-release-age resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdatePolicy {
    Major,
    Minor,
    Patch,
}

impl UpdatePolicy {
    pub fn allows(self, current: &str, candidate: &str) -> bool {
        if self == Self::Major {
            return true;
        }
        let Some(current) = parse_semver(current) else {
            return false;
        };
        let Some(candidate) = parse_semver(candidate) else {
            return false;
        };

        match self {
            Self::Minor => current.major == candidate.major,
            Self::Patch => current.major == candidate.major && current.minor == candidate.minor,
            Self::Major => unreachable!(),
        }
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

/// Returns the explicit background update policy for this backend.
///
/// A fully-qualified backend key wins over a short name so two backends can
/// expose the same tool name without sharing a policy.
pub fn update_policy(settings: &Settings, tool: &BackendArg) -> Option<UpdatePolicy> {
    let tools = settings.tool_update.tools.as_ref()?;
    let tool_id = tool.full_without_opts();
    let value = tools.get(&tool_id).or_else(|| tools.get(&tool.short))?;
    match value.parse() {
        Ok(policy) => Some(policy),
        Err(err) => {
            warn!("ignoring invalid background update policy {value:?} for {tool_id}: {err:#}");
            None
        }
    }
}

fn parse_semver(version: &str) -> Option<Version> {
    Version::parse(version.strip_prefix('v').unwrap_or(version)).ok()
}

/// Returns whether this backend has a configured request protected by a lockfile.
pub async fn tool_has_lockfile(config: &Arc<Config>, tool_id: &str) -> Result<bool> {
    let requests = config.get_tool_request_set().await?;
    Ok(requests
        .iter()
        .flat_map(|(_, requests, _)| requests)
        .filter(|request| request.ba().full_without_opts() == tool_id)
        .any(|request| request_has_lockfile(config, request)))
}

/// A background upgrade must never change a lockfile, even if the matching
/// tool has not yet gained an entry. Existing lockfiles therefore opt the
/// whole request out, rather than just checking whether a current binding can
/// be read from one.
pub fn request_has_lockfile(config: &Config, request: &ToolRequest) -> bool {
    lockfile::lockfile_path_for_tool_source(config, request.source())
        .is_some_and(|(path, _)| path.exists())
        || request.lockfile_resolve(config).ok().flatten().is_some()
}

/// Stable filename component for a backend's background-update state.
pub fn tool_update_key(tool_id: &str) -> String {
    hash::hash_to_str(&tool_id)
}

#[cfg(test)]
mod tests {
    use super::{UpdatePolicy, tool_update_key};

    #[test]
    fn tool_key_is_stable_and_distinct() {
        assert_eq!(tool_update_key("claude"), tool_update_key("claude"));
        assert_ne!(tool_update_key("claude"), tool_update_key("codex"));
    }

    #[test]
    fn update_policy_respects_semver_boundaries() {
        assert!(UpdatePolicy::Major.allows("1.2.3", "2.0.0"));
        assert!(UpdatePolicy::Major.allows("rolling", "nightly"));
        assert!(UpdatePolicy::Minor.allows("1.2.3", "1.9.0"));
        assert!(!UpdatePolicy::Minor.allows("1.2.3", "2.0.0"));
        assert!(UpdatePolicy::Patch.allows("v1.2.3", "1.2.9"));
        assert!(!UpdatePolicy::Patch.allows("1.2.3", "1.3.0"));
        assert!(!UpdatePolicy::Patch.allows("rolling", "1.2.4"));
    }
}
