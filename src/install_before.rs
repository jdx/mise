use std::sync::Arc;

use eyre::Result;
use jiff::Timestamp;

use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::{BackendArg, split_bracketed_opts};
use crate::config::{Config, Settings};
use crate::duration::{parse_duration, parse_into_timestamp};

const DEFAULT_MINIMUM_RELEASE_AGE: &str = "24h";
const DISABLED_MINIMUM_RELEASE_AGE_CUTOFF: &str = "2099-01-01";

/// Where an effective release-age cutoff came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeforeDateSource {
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
pub fn resolve_before_date(
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<Timestamp>> {
    Ok(
        resolve_before_date_with_excludes(None, before_date, minimum_release_age, false)?
            .map(|(ts, _)| ts),
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
    Ok(resolve_before_date_with_excludes(None, None, minimum_release_age, true)?.map(|(ts, _)| ts))
}

pub fn resolve_before_date_for_tool(
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
pub fn resolve_before_date_for_tool_with_source(
    backend_arg: &BackendArg,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<(Timestamp, BeforeDateSource)>> {
    resolve_before_date_with_excludes(
        Some(backend_arg),
        before_date,
        minimum_release_age,
        is_minimum_release_age_excluded(backend_arg),
    )
}

fn resolve_before_date_with_excludes(
    backend_arg: Option<&BackendArg>,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
    excluded: bool,
) -> Result<Option<(Timestamp, BeforeDateSource)>> {
    if let Some(before_date) = before_date {
        return Ok(Some((before_date, BeforeDateSource::Provided)));
    }
    if let Some(before) = minimum_release_age {
        if parse_duration(before).is_ok_and(|duration| duration.is_zero()) {
            return Ok(None);
        }
        return Ok(Some((
            parse_into_timestamp(before)?,
            BeforeDateSource::Explicit,
        )));
    }
    if !excluded && let Some(before) = &Settings::get().minimum_release_age {
        if parse_duration(before).is_ok_and(|duration| duration.is_zero()) {
            return Ok(None);
        }
        return Ok(Some((
            parse_into_timestamp(before)?,
            BeforeDateSource::Explicit,
        )));
    }
    if !excluded && backend_arg.is_some_and(default_minimum_release_age_applies) {
        return Ok(Some((
            parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE)?,
            BeforeDateSource::Default,
        )));
    }
    Ok(None)
}

fn default_minimum_release_age_applies(backend_arg: &BackendArg) -> bool {
    matches!(
        backend_arg.backend_type(),
        BackendType::Aqua
            | BackendType::Cargo
            | BackendType::Core
            | BackendType::Forgejo
            | BackendType::Gem
            | BackendType::Github
            | BackendType::Gitlab
            | BackendType::Go
            | BackendType::Npm
            | BackendType::Pipx
            | BackendType::Spm
            | BackendType::Ubi
    )
}

fn is_minimum_release_age_excluded(backend_arg: &BackendArg) -> bool {
    let excludes = &Settings::get().minimum_release_age_excludes;
    if excludes.is_empty() {
        return false;
    }

    let mut full = None;
    let mut backend_wildcard = None;
    excludes.iter().any(|exclude| {
        let exclude = exclude.trim();
        if exclude.is_empty() {
            return false;
        }
        if exclude == backend_arg.short {
            return true;
        }
        let full = full.get_or_insert_with(|| {
            if backend_arg.short.contains(':') {
                split_bracketed_opts(&backend_arg.short)
                    .map(|(name, _)| name.to_string())
                    .unwrap_or_else(|| backend_arg.short.clone())
            } else {
                backend_arg.full_without_opts()
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

pub(crate) async fn resolve_before_date_for_backend<B: Backend + ?Sized>(
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

#[cfg(test)]
mod tests {
    use super::{
        BeforeDateSource, DEFAULT_MINIMUM_RELEASE_AGE, resolve_before_date,
        resolve_before_date_for_tool, resolve_before_date_for_tool_with_source,
    };
    use crate::cli::args::BackendArg;
    use crate::config::settings::{Settings, SettingsPartial};
    use confique::Layer;
    use jiff::Timestamp;
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
        Settings::reset(None);
        let cli_before = "2024-01-02T03:04:05Z".parse().unwrap();
        assert_eq!(
            resolved_timestamp(Some(cli_before), Some("7d")),
            Some(cli_before)
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_prefers_tool_option() {
        Settings::reset(None);
        assert_eq!(
            resolved_timestamp(None, Some("2024-01-02")),
            Some(crate::duration::parse_into_timestamp("2024-01-02").unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_zero_minimum_release_age_disables_cutoff() {
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
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_falls_back_to_global_setting() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_timestamp(None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_excludes_global_by_backend_id() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm:prettier".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(resolved_tool_timestamp("npm:prettier", None, None), None);
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_does_not_exclude_backend_by_bare_name() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_excludes_global_by_backend_wildcard() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm:*".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(resolved_tool_timestamp("npm:prettier", None, None), None);
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_does_not_exclude_by_bare_backend_tool_name() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["prettier".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp("2024-01-03").unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_exclude_does_not_override_tool_option() {
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("2024-01-03".to_string());
        partial.minimum_release_age_excludes = Some(vec!["npm".to_string()]);
        Settings::reset(Some(partial));
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, Some("2024-01-02")),
            Some(crate::duration::parse_into_timestamp("2024-01-02").unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_without_backend_has_no_default() {
        Settings::reset(None);
        assert_eq!(resolved_timestamp(None, None), None);
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_falls_back_to_default_for_supported_backend() {
        Settings::reset(None);
        assert_eq!(
            resolved_tool_timestamp("npm:prettier", None, None),
            Some(crate::duration::parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE).unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_falls_back_to_default_for_forgejo_backend() {
        Settings::reset(None);
        assert_eq!(
            resolved_tool_timestamp("forgejo:codeberg.org/forgejo/forgejo", None, None),
            Some(crate::duration::parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE).unwrap())
        );
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_skips_default_for_unsupported_backend() {
        Settings::reset(None);
        assert_eq!(resolved_tool_timestamp("asdf:tiny", None, None), None);
        Settings::reset(None);
    }

    #[test]
    fn test_before_date_source_distinguishes_default_from_explicit() {
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
        Settings::reset(None);
    }

    #[test]
    fn test_effective_before_date_stable_within_process() {
        // Covers the invariant behind #9156: relative durations resolve
        // identically across calls within one invocation.
        Settings::reset(None);
        let mut partial = SettingsPartial::empty();
        partial.minimum_release_age = Some("3d".to_string());
        Settings::reset(Some(partial));
        let a = resolved_timestamp(None, None);
        let b = resolved_timestamp(None, None);
        assert_eq!(a, b);
        Settings::reset(None);
    }
}
