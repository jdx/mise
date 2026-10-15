//! Network settings that also depend on the command being run: `--offline`,
//! prefer-offline commands, and the remote-version lookup budget.

use std::sync::atomic::Ordering;
use std::time::Duration;

use mise_settings::Settings;

pub fn fetch_remote_versions_timeout(settings: &Settings) -> Duration {
    let timeout = configured_fetch_remote_versions_timeout(settings);
    if bound_remote_version_lookups(settings) {
        timeout.min(Duration::from_secs(3))
    } else {
        timeout
    }
}

pub fn configured_fetch_remote_versions_timeout(settings: &Settings) -> Duration {
    crate::duration::parse_duration(&settings.fetch_remote_versions_timeout).unwrap()
}

/// Whether remote-version lookups should use the aggressive fast-path budget
/// (a single ~3s attempt with no retries). This is on under `prefer_offline`
/// so shims and shell activation never stall — but NOT for commands whose
/// whole job is to enumerate remote versions/tags (`mise lock`, `ls-remote`,
/// `outdated`, `upgrade`), which must honor the full configured
/// `fetch_remote_versions_timeout` and retry budget even when
/// `prefer_offline` is set.
///
/// See <https://github.com/jdx/mise/discussions/11185>.
pub fn bound_remote_version_lookups(settings: &Settings) -> bool {
    prefer_offline(settings) && !crate::env::REMOTE_FETCH_COMMAND.load(Ordering::Relaxed)
}

/// The default for `fetch_remote_versions_cache`.
pub const DEFAULT_FETCH_REMOTE_VERSIONS_CACHE: Duration = Duration::from_secs(60 * 60);

/// duration that remote version cache is kept for
/// for "fast" commands (represented by PREFER_OFFLINE), these are always
/// cached. For "slow" commands like `mise ls-remote` or `mise install`:
/// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is set, use that
/// - if MISE_FETCH_REMOTE_VERSIONS_CACHE is not set, use HOURLY
///
/// A command that only needs a matching version may keep a dated version list
/// longer when this is not set; see [`release_age_list_window_applies`].
pub fn fetch_remote_versions_cache(settings: &Settings) -> Option<Duration> {
    if prefer_offline(settings) {
        None
    } else {
        Some(
            settings
                .fetch_remote_versions_cache
                .as_deref()
                .map(|cache| crate::duration::parse_duration(cache).unwrap())
                .unwrap_or(DEFAULT_FETCH_REMOTE_VERSIONS_CACHE),
        )
    }
}

/// Whether a cached version list may stay fresh for as long as the tool's
/// release-age cutoff instead of `fetch_remote_versions_cache`.
///
/// Not when the user set `fetch_remote_versions_cache` (that value then
/// applies everywhere), under prefer-offline (cached lists never expire), or
/// for a command whose job is reporting what is newest (`outdated`,
/// `ls-remote`, …), which keeps the shorter window.
pub fn release_age_list_window_applies(settings: &Settings) -> bool {
    settings.fetch_remote_versions_cache.is_none()
        && !prefer_offline(settings)
        && !crate::env::REMOTE_FETCH_COMMAND.load(Ordering::Relaxed)
}

pub fn http_timeout(settings: &Settings) -> Duration {
    crate::duration::parse_duration(&settings.http_timeout).unwrap()
}

pub fn http_download_timeout(settings: &Settings) -> Duration {
    crate::duration::parse_duration(&settings.http_download_timeout).unwrap()
}

/// Fast-path commands should make at most one network attempt before falling
/// back to cached/local behavior. In particular, shims must not multiply a
/// stalled resolver timeout by the configured retry count.
pub fn http_retries(settings: &Settings) -> i64 {
    if bound_remote_version_lookups(settings) {
        0
    } else {
        settings.http_retries
    }
}

/// Returns true if offline mode is enabled via setting or CLI flag/env var.
pub fn offline(settings: &Settings) -> bool {
    settings.offline || *crate::env::OFFLINE
}

/// Returns true if prefer-offline mode is enabled via setting, env var, or
/// because the current command is a "fast" command (hook-env, activate, etc.).
/// Also returns true if offline mode is enabled (offline implies prefer-offline).
pub fn prefer_offline(settings: &Settings) -> bool {
    offline(settings)
        || settings.prefer_offline
        || crate::env::PREFER_OFFLINE.load(Ordering::Relaxed)
}
