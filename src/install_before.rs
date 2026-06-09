use std::sync::Arc;

use eyre::Result;
use jiff::Timestamp;

use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::{BackendArg, split_bracketed_opts};
use crate::config::{Config, Settings};
use crate::duration::parse_into_timestamp;

const DEFAULT_MINIMUM_RELEASE_AGE: &str = "24h";

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
    resolve_before_date_with_excludes(None, before_date, minimum_release_age, false)
}

pub fn resolve_before_date_for_tool(
    backend_arg: &BackendArg,
    before_date: Option<Timestamp>,
    minimum_release_age: Option<&str>,
) -> Result<Option<Timestamp>> {
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
) -> Result<Option<Timestamp>> {
    if let Some(before_date) = before_date {
        return Ok(Some(before_date));
    }
    if let Some(before) = minimum_release_age {
        return Ok(Some(parse_into_timestamp(before)?));
    }
    if !excluded && let Some(before) = &Settings::get().minimum_release_age {
        return Ok(Some(parse_into_timestamp(before)?));
    }
    if !excluded && backend_arg.is_some_and(default_minimum_release_age_applies) {
        return Ok(Some(parse_into_timestamp(DEFAULT_MINIMUM_RELEASE_AGE)?));
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
    use super::{DEFAULT_MINIMUM_RELEASE_AGE, resolve_before_date, resolve_before_date_for_tool};
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
