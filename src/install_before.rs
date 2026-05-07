use std::sync::Arc;

use eyre::Result;
use jiff::Timestamp;

use crate::backend::Backend;
use crate::config::{Config, Settings};
use crate::duration::parse_into_timestamp;

/// Resolve the effective `minimum_release_age` cutoff.
///
/// Precedence (highest to lowest):
/// 1. `before_date` - a pre-resolved `ResolveOptions` cutoff.
/// 2. A per-tool, backend, or config `minimum_release_age` option.
/// 3. The global `minimum_release_age` setting.
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
    if let Some(before_date) = before_date {
        return Ok(Some(before_date));
    }
    if let Some(before) = minimum_release_age {
        return Ok(Some(parse_into_timestamp(before)?));
    }
    if let Some(before) = &Settings::get().minimum_release_age {
        return Ok(Some(parse_into_timestamp(before)?));
    }
    Ok(None)
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
    resolve_before_date(None, opts.minimum_release_age())
}

#[cfg(test)]
mod tests {
    use super::resolve_before_date;
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
    fn test_effective_before_date_none_when_unset() {
        Settings::reset(None);
        assert_eq!(resolved_timestamp(None, None), None);
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
