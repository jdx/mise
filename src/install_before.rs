use std::sync::{Arc, Mutex};
use std::time::Duration;

use eyre::Result;
use jiff::civil::date;
use jiff::{Span, Timestamp};

use crate::args::{BackendArg, split_bracketed_opts};
use crate::backend::Backend;
use crate::config::{Config, Settings};
use crate::duration::{parse_duration, parse_into_timestamp};

const DEFAULT_MINIMUM_RELEASE_AGE: &str = "24h";
const DISABLED_MINIMUM_RELEASE_AGE_CUTOFF: &str = "2099-01-01";

/// The shortest and longest a release-age window keeps a cached version list.
/// See [`release_age_list_window`].
const RELEASE_AGE_LIST_WINDOW_MIN: Duration = Duration::from_secs(60 * 60);
const RELEASE_AGE_LIST_WINDOW_MAX: Duration = Duration::from_secs(24 * 60 * 60);

/// The cutoff from the running command's `--minimum-release-age` flag, which
/// resolution receives directly rather than through tool options.
static CLI_MINIMUM_RELEASE_AGE_CUTOFF: Mutex<Option<Timestamp>> = Mutex::new(None);

/// Record the cutoff a command's `--minimum-release-age` flag resolved to, so
/// [`release_age_list_window`] sizes its window by it rather than by the
/// tool's configured cutoff.
pub fn set_cli_minimum_release_age_cutoff(cutoff: Timestamp) {
    *CLI_MINIMUM_RELEASE_AGE_CUTOFF.lock().unwrap() = Some(cutoff);
}

/// How long a command that only needs a version matching its request, such as
/// `mise install`, may keep a cached remote version list for `backend_arg` when
/// that list dates every version: the tool's effective release-age cutoff less
/// `LISTED_SNAPSHOT_LAG`, clamped to between 1 and 24 hours.
///
/// A release missing from the list was published after the snapshot it holds,
/// which can be up to `LISTED_SNAPSHOT_LAG` older than the cache file, and the
/// cutoff hides it until it reaches that age, so reusing the list for as long
/// does not delay any release beyond the cutoff. `None` means no window, and
/// the list is kept for `fetch_remote_versions_cache` as usual: when the user
/// set that, for a command that reports what is newest, under prefer-offline,
/// and for a tool with no cutoff.
pub(crate) fn release_age_list_window(
    backend_arg: &BackendArg,
    minimum_release_age: Option<&str>,
) -> Option<Duration> {
    if !mise_util::network::release_age_list_window_applies(&Settings::get()) {
        return None;
    }
    let cutoff = match *CLI_MINIMUM_RELEASE_AGE_CUTOFF.lock().unwrap() {
        Some(cutoff) => cutoff,
        None => resolve_before_date_for_tool(backend_arg, None, minimum_release_age).ok()??,
    };
    release_age_list_window_for_cutoff(cutoff)
}

/// [`release_age_list_window`] for a cutoff the caller already resolved.
pub(crate) fn release_age_list_window_for_cutoff(cutoff: Timestamp) -> Option<Duration> {
    mise_util::network::release_age_list_window_applies(&Settings::get())
        .then(|| list_window_for_cutoff(cutoff, crate::duration::process_now()))
}

/// The release-age window for a list read at `now` under `cutoff`: a cutoff in
/// the future (`--minimum-release-age 0s`) gives the shortest window.
fn list_window_for_cutoff(cutoff: Timestamp, now: Timestamp) -> Duration {
    Duration::try_from(now.duration_since(cutoff))
        .unwrap_or(Duration::ZERO)
        .saturating_sub(crate::backend::LISTED_SNAPSHOT_LAG)
        .clamp(RELEASE_AGE_LIST_WINDOW_MIN, RELEASE_AGE_LIST_WINDOW_MAX)
}

/// Where an effective release-age cutoff came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BeforeDateSource {
    /// Pre-resolved by the caller (e.g. the `--minimum-release-age` CLI flag
    /// or a `ResolveOptions` cutoff threaded through from another resolution).
    /// The caller already knows whether it was explicit or default.
    Provided,
    /// A per-tool `minimum_release_age` option or the explicit
    /// `minimum_release_age` setting.
    Explicit,
    /// The built-in default for backends that report release timestamps.
    /// This only gates which versions remote resolution may pick — it must
    /// not disable installed-version fast paths, otherwise every resolution
    /// becomes a remote fetch (https://github.com/jdx/mise/discussions/10308).
    Default,
}

/// Resolve the effective `minimum_release_age` cutoff.
///
/// Precedence (highest to lowest):
/// 1. `before_date` - a pre-resolved `ResolveOptions` cutoff.
/// 2. A per-tool, backend, or config `minimum_release_age` option.
/// 3. The global `minimum_release_age` setting, or the built-in default for
///    backends that provide release timestamps.
///
/// All string-based durations (e.g. `"3d"`) are resolved against
/// [`crate::duration::process_now`] so that every call within a single mise
/// invocation produces the same absolute timestamp. Downstream code can then
/// use the resolved timestamp both to resolve which version to install *and*
/// to build the corresponding package-manager CLI flag (e.g.
/// `--min-release-age`) without the two drifting apart.
pub(crate) fn resolve_before_date(
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<Timestamp>> {
    Ok(
        resolve_before_date_with_excludes(None, before_date, minimum_release_age, false)?
            .map(|(ts, _, _)| ts),
    )
}

/// Resolve the CLI `--minimum-release-age` flag without falling back to global
/// settings or the built-in default when the flag is omitted.
pub fn resolve_cli_minimum_release_age(
    minimum_release_age: Option<&str>,
) -> Result<Option<Timestamp>> {
    if minimum_release_age
        .is_some_and(|age| parse_duration(age).is_ok_and(|duration| duration.is_zero()))
    {
        return Ok(Some(parse_into_timestamp(
            DISABLED_MINIMUM_RELEASE_AGE_CUTOFF,
        )?));
    }
    Ok(
        resolve_before_date_with_excludes(None, None, minimum_release_age, true)?
            .map(|(ts, _, _)| ts),
    )
}

pub(crate) fn resolve_before_date_for_tool(
    backend_arg: &BackendArg,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<Timestamp>> {
    Ok(
        resolve_before_date_for_tool_with_source(backend_arg, before_date, minimum_release_age)?
            .map(|(ts, _)| ts),
    )
}

/// Like `resolve_before_date_for_tool` but also reports where the cutoff came
/// from, so callers can treat the built-in default differently from explicit
/// configuration.
pub(crate) fn resolve_before_date_for_tool_with_source(
    backend_arg: &BackendArg,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<(Timestamp, BeforeDateSource)>> {
    Ok(resolve_before_date_with_excludes(
        Some(backend_arg),
        before_date,
        minimum_release_age,
        is_minimum_release_age_excluded(backend_arg),
    )?
    .map(|(ts, source, _)| (ts, source)))
}

/// The raw `minimum_release_age` value (e.g. `"3d"`, `"24h"`, `"2024-01-01"`)
/// that produces the effective cutoff for a tool, for display in user-facing
/// messages. Follows the same precedence as
/// `resolve_before_date_for_tool_with_source`, except cutoffs pre-resolved by
/// the caller (e.g. the CLI flag) are not visible here — the caller already
/// knows those. Returns `None` when no cutoff applies to the tool.
pub fn effective_minimum_release_age_for_tool(
    backend_arg: &BackendArg,
    minimum_release_age: Option<&str>,
) -> Option<String> {
    resolve_before_date_with_excludes(
        Some(backend_arg),
        None,
        minimum_release_age,
        is_minimum_release_age_excluded(backend_arg),
    )
    .ok()
    .flatten()
    .and_then(|(_, _, age)| age)
}

/// The configured `minimum_release_age` value, but only when it is what
/// produced `before`.
///
/// A cutoff threaded down from `--minimum-release-age` or another resolution
/// does not correspond to any configured value, and every configured value
/// resolves against [`crate::duration::process_now`], so comparing the
/// timestamps is exact within one invocation. Labelling an error with a value
/// that did not produce its cutoff would name the wrong date.
pub(crate) fn minimum_release_age_label(
    backend_arg: &BackendArg,
    minimum_release_age: Option<&str>,
    before: Timestamp,
) -> Option<String> {
    effective_minimum_release_age_for_tool(backend_arg, minimum_release_age)
        .filter(|age| parse_into_timestamp(age).is_ok_and(|resolved| resolved == before))
}

fn resolve_before_date_with_excludes(
    backend_arg: Option<&BackendArg>,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
    excluded: bool,
) -> Result<Option<(Timestamp, BeforeDateSource, Option<String>)>> {
    if let Some(before_date) = before_date {
        return Ok(Some((before_date, BeforeDateSource::Provided, None)));
    }
    if let Some(before) = minimum_release_age {
        if parse_duration(before).is_ok_and(|duration| duration.is_zero()) {
            return Ok(None);
        }
        return Ok(Some((
            parse_into_timestamp(before)?,
            BeforeDateSource::Explicit,
            Some(before.to_string()),
        )));
    }
    if !excluded && let Some(before) = &Settings::get().minimum_release_age {
        if parse_duration(before).is_ok_and(|duration| duration.is_zero()) {
            return Ok(None);
        }
        return Ok(Some((
            parse_into_timestamp(before)?,
            BeforeDateSource::Explicit,
            Some(before.to_string()),
        )));
    }
    if !excluded && backend_arg.is_some() {
        return Ok(Some((
            parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE)?,
            BeforeDateSource::Default,
            Some(DEFAULT_MINIMUM_RELEASE_AGE.to_string()),
        )));
    }
    Ok(None)
}

fn is_minimum_release_age_excluded(backend_arg: &BackendArg) -> bool {
    let excludes = &Settings::get().minimum_release_age_excludes;
    if excludes.is_empty() {
        return false;
    }

    let mut full = None;
    let mut backend_wildcard = None;
    excludes.iter().any(|exclude| {
        let exclude = crate::backend::canonical_backend_full(exclude.trim());
        let exclude = exclude.as_ref();
        if exclude.is_empty() {
            return false;
        }
        if exclude == crate::backend::canonical_backend_full(&backend_arg.short) {
            return true;
        }
        let full = full.get_or_insert_with(|| {
            if backend_arg.short.contains(':') {
                let name = split_bracketed_opts(&backend_arg.short)
                    .map(|(name, _)| name)
                    .unwrap_or(&backend_arg.short);
                crate::backend::canonical_backend_full(name).into_owned()
            } else {
                crate::backend::canonical_backend_full(&backend_arg.full_without_opts())
                    .into_owned()
            }
        });
        if exclude == full {
            return true;
        }
        let backend_wildcard =
            backend_wildcard.get_or_insert_with(|| format!("{}:*", backend_arg.backend_type()));
        exclude == backend_wildcard
    })
}

pub async fn resolve_before_date_for_backend<B: Backend + ?Sized>(
    config: &Arc<Config>,
    backend: &B,
    before_date: Option<Timestamp>,
) -> Result<Option<Timestamp>> {
    if before_date.is_some() {
        return resolve_before_date(before_date, None);
    }

    let opts = config.get_tool_opts_with_overrides(backend.ba()).await?;
    resolve_before_date_for_tool(backend.ba(), None, opts.minimum_release_age())
}

/// Human-readable fragments describing a release hidden by `minimum_release_age`:
/// when it came out and when it becomes eligible, plus the configured age value.
/// Shared by `mise upgrade`'s warning and the resolution error raised when the
/// cutoff hides every candidate.
pub fn format_hidden_release_details(
    created_at: Option<Timestamp>,
    age: Option<&str>,
    tz: jiff::tz::TimeZone,
) -> (String, String) {
    let age_fragment = age.map(|age| format!(" ({age})")).unwrap_or_default();
    let released_fragment = match created_at {
        Some(created) => {
            // An age given as an absolute date is a fixed cutoff, so the
            // release never becomes eligible — only show when it will for
            // relative ages.
            let eligible_at = age.and_then(|age| release_eligible_at(created, age));
            let released = created.to_zoned(tz.clone()).strftime("%Y-%m-%d");
            match eligible_at {
                Some(at) => format!(
                    " (released {released}, eligible {})",
                    at.to_zoned(tz).strftime("%Y-%m-%d %H:%M %Z")
                ),
                None => format!(" (released {released})"),
            }
        }
        None => String::new(),
    };
    (released_fragment, age_fragment)
}

fn release_eligible_at(created_at: Timestamp, age: &str) -> Option<Timestamp> {
    const DAY_NANOS: i128 = 86_400 * 1_000_000_000;

    let span = age.parse::<Span>().ok()?;
    let duration = span.to_duration(date(2025, 1, 1)).ok()?;
    if duration.is_negative() {
        return None;
    }
    let mut high = created_at
        .to_zoned(jiff::tz::TimeZone::UTC)
        .checked_add(span)
        .ok()
        .map(|eligible| eligible.timestamp())?;

    for _ in 0..370 {
        if release_is_eligible_at(created_at, high, &span) {
            let mut low_nanos = created_at.as_nanosecond();
            let mut high_nanos = high.as_nanosecond();
            while low_nanos < high_nanos {
                let mid_nanos = low_nanos + (high_nanos - low_nanos) / 2;
                let mid = Timestamp::from_nanosecond(mid_nanos).ok()?;
                if release_is_eligible_at(created_at, mid, &span) {
                    high_nanos = mid_nanos;
                } else {
                    low_nanos = mid_nanos + 1;
                }
            }
            return Timestamp::from_nanosecond(high_nanos).ok();
        }
        high = Timestamp::from_nanosecond(high.as_nanosecond().checked_add(DAY_NANOS)?).ok()?;
    }
    None
}

fn release_is_eligible_at(created_at: Timestamp, now: Timestamp, age: &Span) -> bool {
    now.to_zoned(jiff::tz::TimeZone::UTC)
        .checked_sub(age)
        .is_ok_and(|cutoff| cutoff.timestamp() > created_at)
}

#[cfg(test)]
mod tests {
    use super::{
        BeforeDateSource, DEFAULT_MINIMUM_RELEASE_AGE, effective_minimum_release_age_for_tool,
        format_hidden_release_details, list_window_for_cutoff, minimum_release_age_label,
        release_age_list_window, release_is_eligible_at, resolve_before_date,
        resolve_before_date_for_tool, resolve_before_date_for_tool_with_source,
    };
    use crate::args::BackendArg;
    use crate::config::SettingsExt;
    use crate::config::settings::{Settings, SettingsPartial};
    use confique::Layer;
    use jiff::Timestamp;
    use jiff::tz::TimeZone;
    use std::time::Duration;
    use test_log::test;

    fn resolved_timestamp(
        before_date: Option<Timestamp>,
        minimum_release_age: Option<&str>,
    ) -> Option<Timestamp> {
        resolve_before_date(before_date, minimum_release_age).unwrap()
    }

    fn resolved_tool_timestamp(
        tool: &str,
        before_date: Option<Timestamp>,
        minimum_release_age: Option<&str>,
    ) -> Option<Timestamp> {
        let backend_arg: BackendArg = tool.into();
        resolve_before_date_for_tool(&backend_arg, before_date, minimum_release_age).unwrap()
    }

    #[test]
    fn test_effective_before_date_prefers_override() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let cli_before = "2024-01-02T03:04:05Z".parse().unwrap();
        assert_eq!(
            resolved_timestamp(Some(cli_before), Some("7d")),
            Some(cli_before)
        );
    }

    #[test]
    fn test_effective_before_date_prefers_tool_option() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        assert_eq!(
            resolved_timestamp(None, Some("2024-01-02")),
            Some(crate::duration::parse_into_timestamp("2024-01-02").unwrap())
        );
    }

    #[test]
    fn test_zero_minimum_release_age_disables_cutoff() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        assert_eq!(resolved_timestamp(None, Some("0s")), None);
        assert_eq!(
            super::resolve_cli_minimum_release_age(Some("0s")).unwrap(),
            Some(crate::duration::parse_into_timestamp("2099-01-01").unwrap())
        );
        assert_eq!(super::resolve_cli_minimum_release_age(None).unwrap(), None);

        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("0s".to_string());
        Settings::reset(Some(partial));
        assert_eq!(resolved_tool_timestamp("github:cli/cli", None, None), None);
    }

    #[test]
    fn test_list_window_for_cutoff() {
        let now = Timestamp::now();
        let ago = |secs: i64| now - jiff::SignedDuration::from_secs(secs);
        let hours = |hours: u64| Duration::from_secs(hours * 60 * 60);
        // Less the hour a versions-host snapshot can lag the cache file.
        assert_eq!(list_window_for_cutoff(ago(6 * 3600), now), hours(5));
        assert_eq!(list_window_for_cutoff(ago(24 * 3600), now), hours(23));
        assert_eq!(list_window_for_cutoff(ago(2 * 3600), now), hours(1));
        assert_eq!(list_window_for_cutoff(ago(7 * 24 * 3600), now), hours(24));
        assert_eq!(list_window_for_cutoff(ago(30 * 60), now), hours(1));
        assert_eq!(list_window_for_cutoff(now, now), hours(1));
        // `--minimum-release-age 0s` resolves to a cutoff in the future.
        assert_eq!(
            list_window_for_cutoff(
                crate::duration::parse_into_timestamp("2099-01-01").unwrap(),
                now
            ),
            hours(1)
        );
    }

    #[test]
    fn test_release_age_list_window() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let hours = |hours: u64| Some(Duration::from_secs(hours * 60 * 60));
        let tool: BackendArg = "github:cli/cli".into();

        // The 24h default cutoff, less the snapshot lag.
        assert_eq!(release_age_list_window(&tool, None), hours(23));
        // A per-tool cutoff.
        assert_eq!(release_age_list_window(&tool, Some("6h")), hours(5));
        assert_eq!(release_age_list_window(&tool, Some("7d")), hours(24));
        assert_eq!(release_age_list_window(&tool, Some("30m")), hours(1));
        assert_eq!(release_age_list_window(&tool, Some("0s")), None);

        // The global setting, which a per-tool cutoff overrides.
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("6h".to_string());
        Settings::reset(Some(partial.clone()));
        assert_eq!(release_age_list_window(&tool, None), hours(5));
        assert_eq!(release_age_list_window(&tool, Some("12h")), hours(11));
        let mut disabled = partial.clone();
        disabled.minimum_release_age = Some("0s".to_string());
        Settings::reset(Some(disabled));
        assert_eq!(release_age_list_window(&tool, None), None);

        // A `fetch_remote_versions_cache` the user set applies as set.
        partial.fetch_remote_versions_cache = Some("1h".to_string());
        Settings::reset(Some(partial));
        assert_eq!(release_age_list_window(&tool, None), None);
        assert_eq!(release_age_list_window(&tool, Some("6h")), None);
    }

    #[test]
    fn test_effective_before_date_falls_back_to_global_setting() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_timestamp(None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_excludes_global_by_backend_id() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm:prettier".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(resolved_tool_timestamp("npm:prettier", None, None), None);
    }

    #[test]
    fn test_pypi_release_age_exclusions_accept_both_backend_names() {
        let _settings = crate::test::SettingsGuard::lock();
        for exclude in ["pipx:*", "pypi:*", "pipx:black", "pypi:black"] {
            let mut partial = SettingsPartial::empty();
            partial.minimum_release_age = Some("2024-01-03".to_string());
            partial.minimum_release_age_excludes = Some(vec![exclude.to_owned()]);
            Settings::reset(Some(partial));
            assert_eq!(resolved_tool_timestamp("pypi:black", None, None), None);
            assert_eq!(resolved_tool_timestamp("pipx:black", None, None), None);
        }
    }

    #[test]
    fn test_effective_before_date_does_not_exclude_backend_by_bare_name() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_excludes_global_by_backend_wildcard() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm:*".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(resolved_tool_timestamp("npm:prettier", None, None), None);
    }

    #[test]
    fn test_effective_before_date_does_not_exclude_by_bare_backend_tool_name() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["prettier".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_exclude_does_not_override_tool_option() {
        let _settings = crate::test::SettingsGuard::lock();
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, Some("2024-01-02")),
            Some(crate::duration::parse_into_timestamp("2024-01-02").unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_without_backend_has_no_default() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        assert_eq!(resolved_timestamp(None, None), None);
    }

    #[test]
    fn test_effective_before_date_falls_back_to_default_for_supported_backend() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE).unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_falls_back_to_default_for_forgejo_backend() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        assert_eq!(
            resolved_tool_timestamp("forgejo:codeberg.org/forgejo/forgejo", None, None),
            Some(crate::duration::parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE).unwrap())
        );
    }

    #[test]
    fn test_effective_before_date_falls_back_to_default_for_every_backend() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let default =
            Some(crate::duration::parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE).unwrap());
        for backend in [
            "asdf:tiny",
            "vfox:jdx/vfox-yarn",
            "conda:make",
            "http:dart",
            "dotnet:Foo",
        ] {
            assert_eq!(
                resolved_tool_timestamp(backend, None, None),
                default,
                "{backend}"
            );
        }
    }

    #[test]
    fn test_before_date_source_distinguishes_default_from_explicit() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let ba: BackendArg = "npm:prettier".into();

        // Built-in default → Default source
        let (_, source) = resolve_before_date_for_tool_with_source(&ba, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(source, BeforeDateSource::Default);

        // Per-tool option → Explicit source
        let (_, source) = resolve_before_date_for_tool_with_source(&ba, None, Some("7d"))
            .unwrap()
            .unwrap();
        assert_eq!(source, BeforeDateSource::Explicit);

        // Explicit global setting → Explicit source
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("7d".to_string());
        Settings::reset(Some(partial));
        let (_, source) = resolve_before_date_for_tool_with_source(&ba, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(source, BeforeDateSource::Explicit);
        Settings::reset(None);

        // Pre-resolved cutoff → Provided source
        let cli_before = "2024-01-02T03:04:05Z".parse().unwrap();
        let (ts, source) = resolve_before_date_for_tool_with_source(&ba, Some(cli_before), None)
            .unwrap()
            .unwrap();
        assert_eq!(ts, cli_before);
        assert_eq!(source, BeforeDateSource::Provided);
    }

    #[test]
    fn test_minimum_release_age_label_only_names_the_value_that_produced_the_cutoff() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let ba: BackendArg = "npm:prettier".into();

        // The built-in default resolves to the cutoff it produced, so it is
        // safe to name in an error message.
        let default_cutoff = resolve_before_date_for_tool(&ba, None, None)
            .unwrap()
            .unwrap();
        assert_eq!(
            minimum_release_age_label(&ba, None, default_cutoff).as_deref(),
            Some(DEFAULT_MINIMUM_RELEASE_AGE)
        );

        // A per-tool value is named the same way.
        let tool_cutoff = resolve_before_date_for_tool(&ba, None, Some("7d"))
            .unwrap()
            .unwrap();
        assert_eq!(
            minimum_release_age_label(&ba, Some("7d"), tool_cutoff).as_deref(),
            Some("7d")
        );

        // A cutoff passed down from --minimum-release-age matches no configured
        // value, so nothing is named rather than the wrong date.
        let flag_cutoff: Timestamp = "1990-01-01T00:00:00Z".parse().unwrap();
        assert_eq!(minimum_release_age_label(&ba, None, flag_cutoff), None);
        assert_eq!(
            minimum_release_age_label(&ba, Some("7d"), flag_cutoff),
            None
        );
    }

    #[test]
    fn test_effective_minimum_release_age_for_tool_reports_raw_value() {
        let _settings = crate::test::SettingsGuard::lock();
        Settings::reset(None);
        let ba: BackendArg = "npm:prettier".into();

        // Built-in default
        assert_eq!(
            effective_minimum_release_age_for_tool(&ba, None).as_deref(),
            Some(DEFAULT_MINIMUM_RELEASE_AGE)
        );

        // Per-tool option
        assert_eq!(
            effective_minimum_release_age_for_tool(&ba, Some("7d")).as_deref(),
            Some("7d")
        );

        // Explicit global setting
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("3d".to_string());
        Settings::reset(Some(partial));
        assert_eq!(
            effective_minimum_release_age_for_tool(&ba, None).as_deref(),
            Some("3d")
        );
        Settings::reset(None);

        // The default reaches backends without release timestamps too
        let asdf_ba: BackendArg = "asdf:tiny".into();
        assert_eq!(
            effective_minimum_release_age_for_tool(&asdf_ba, None).as_deref(),
            Some(DEFAULT_MINIMUM_RELEASE_AGE)
        );
    }

    #[test]
    fn test_effective_before_date_stable_within_process() {
        let _settings = crate::test::SettingsGuard::lock();
        // Covers the invariant behind #9156: relative durations resolve
        // identically across calls within one invocation.
        Settings::reset(None);
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("3d".to_string());
        Settings::reset(Some(partial));
        let a = resolved_timestamp(None, None);
        let b = resolved_timestamp(None, None);
        assert_eq!(a, b);
    }

    #[test]
    fn test_format_hidden_release_details_with_duration_age() {
        let created = "2026-06-26T14:03:00Z".parse().unwrap();
        let (released, age) =
            format_hidden_release_details(Some(created), Some("3d"), TimeZone::UTC);
        assert_eq!(
            released,
            " (released 2026-06-26, eligible 2026-06-29 14:03 UTC)"
        );
        assert_eq!(age, " (3d)");
    }

    #[test]
    fn test_format_hidden_release_details_with_calendar_age() {
        let created = "2023-03-01T14:03:00Z".parse().unwrap();
        let (released, age) =
            format_hidden_release_details(Some(created), Some("1y"), TimeZone::UTC);
        assert_eq!(
            released,
            " (released 2023-03-01, eligible 2024-03-01 14:03 UTC)"
        );
        assert_eq!(age, " (1y)");
    }

    #[test]
    fn test_format_hidden_release_details_with_non_reversible_calendar_age() {
        let created = "2019-01-31T15:30:00Z".parse().unwrap();
        let (released, age) =
            format_hidden_release_details(Some(created), Some("1mo"), TimeZone::UTC);
        assert_eq!(
            released,
            " (released 2019-01-31, eligible 2019-03-01 00:00 UTC)"
        );
        assert_eq!(age, " (1mo)");
    }

    #[test]
    fn test_release_is_eligible_at_uses_strict_cutoff() {
        let created = "2024-01-01T00:00:00Z".parse().unwrap();
        let age = "24h".parse().unwrap();
        let exact_cutoff = "2024-01-02T00:00:00Z".parse().unwrap();
        let after_cutoff = "2024-01-02T00:00:00.000000001Z".parse().unwrap();

        assert!(!release_is_eligible_at(created, exact_cutoff, &age));
        assert!(release_is_eligible_at(created, after_cutoff, &age));
    }

    #[test]
    fn test_format_hidden_release_details_with_absolute_age() {
        // An absolute-date cutoff never becomes eligible, so no eligible time
        let created = "2026-06-26T14:03:00Z".parse().unwrap();
        let (released, age) =
            format_hidden_release_details(Some(created), Some("2026-01-01"), TimeZone::UTC);
        assert_eq!(released, " (released 2026-06-26)");
        assert_eq!(age, " (2026-01-01)");
    }

    #[test]
    fn test_format_hidden_release_details_without_release_date() {
        let (released, age) = format_hidden_release_details(None, Some("24h"), TimeZone::UTC);
        assert_eq!(released, "");
        assert_eq!(age, " (24h)");
    }

    #[test]
    fn test_format_hidden_release_details_without_age() {
        let created = "2026-06-26T14:03:00Z".parse().unwrap();
        let (released, age) = format_hidden_release_details(Some(created), None, TimeZone::UTC);
        assert_eq!(released, " (released 2026-06-26)");
        assert_eq!(age, "");
    }
}
