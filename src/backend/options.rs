use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    list_available_platforms_with_key, lookup_platform_key_for_target, lookup_platform_value,
    lookup_platform_value_for_target, lookup_with_fallback,
};
use crate::toolset::ToolVersionOptions;
use eyre::{Result, bail};

/// The ordering policy a backend applies to eligible version candidates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionOrder {
    #[default]
    Source,
    Semver,
}

impl VersionOrder {
    pub(crate) fn from_options(options: &ToolVersionOptions) -> Result<Self> {
        match options.opts.get("version_order") {
            None => Ok(Self::Source),
            Some(toml::Value::String(value)) => match value.as_str() {
                "source" => Ok(Self::Source),
                "semver" => Ok(Self::Semver),
                _ => bail!("version_order must be \"source\" or \"semver\""),
            },
            Some(_) => bail!("version_order must be \"source\" or \"semver\""),
        }
    }

    pub(crate) fn order(self, versions: Vec<String>) -> Vec<String> {
        if self == Self::Source {
            return versions;
        }

        let mut opaque = Vec::new();
        let mut semantic = Vec::new();
        for (source_index, version) in versions.into_iter().enumerate() {
            let normalized = version
                .strip_prefix('v')
                .or_else(|| version.strip_prefix('V'))
                .unwrap_or(&version);
            match semver::Version::parse(normalized) {
                Ok(parsed) => semantic.push((source_index, version, parsed)),
                Err(_) => opaque.push(version),
            }
        }
        semantic.sort_by(|(left_index, _, left), (right_index, _, right)| {
            left.cmp_precedence(right)
                .then_with(|| left_index.cmp(right_index))
        });
        opaque.extend(semantic.into_iter().map(|(_, version, _)| version));
        opaque
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BackendOptions<'a> {
    raw: &'a ToolVersionOptions,
}

impl<'a> BackendOptions<'a> {
    pub(crate) fn new(raw: &'a ToolVersionOptions) -> Self {
        Self { raw }
    }

    pub(crate) fn raw(&self) -> &'a ToolVersionOptions {
        self.raw
    }

    /// Returns the option only when the underlying TOML value is a string.
    /// Prefer platform helpers for options that may be written as native TOML
    /// scalars.
    pub(crate) fn str(&self, key: &str) -> Option<&'a str> {
        self.raw.get(key)
    }

    pub(crate) fn platform_string(&self, key: &str) -> Option<String> {
        lookup_with_fallback(self.raw, key)
    }

    pub(crate) fn platform_value_without_base(&self, key: &str) -> Option<&'a toml::Value> {
        lookup_platform_value(self.raw, key)
    }

    /// Returns a comma-separated option value from either a string or an array of
    /// strings, warning about non-string array entries.
    pub(crate) fn comma_joined(&self, key: &str) -> Option<String> {
        match self.raw.opts.get(key) {
            Some(toml::Value::Array(values)) => {
                let values = values
                    .iter()
                    .filter_map(|value| {
                        value.as_str().map(str::to_string).or_else(|| {
                            warn!("invalid value in `{key}` array: {value}; expected string");
                            None
                        })
                    })
                    .collect::<Vec<_>>();
                if values.is_empty() {
                    None
                } else {
                    Some(values.join(","))
                }
            }
            _ => self.raw.get_string(key),
        }
    }

    pub(crate) fn platform_string_for_target(
        &self,
        key: &str,
        target: &PlatformTarget,
    ) -> Option<String> {
        lookup_platform_key_for_target(self.raw, key, target).or_else(|| self.raw.get_string(key))
    }

    pub(crate) fn platform_string_for_target_without_base(
        &self,
        key: &str,
        target: &PlatformTarget,
    ) -> Option<String> {
        lookup_platform_key_for_target(self.raw, key, target)
    }

    pub(crate) fn platform_value_for_target(
        &self,
        key: &str,
        target: &PlatformTarget,
    ) -> Option<&'a toml::Value> {
        lookup_platform_value_for_target(self.raw, key, target).or_else(|| self.raw.opts.get(key))
    }

    pub(crate) fn platform_bool_for_target(&self, key: &str, target: &PlatformTarget) -> bool {
        self.platform_string_for_target(key, target)
            .is_some_and(|v| bool_str_or_default(key, &v, false))
    }

    pub(crate) fn bool(&self, key: &str) -> bool {
        self.bool_with_default(key, false)
    }

    pub(crate) fn bool_with_default(&self, key: &str, default: bool) -> bool {
        self.raw
            .opts
            .get(key)
            .map_or(default, |value| bool_value_or_default(key, value, default))
    }

    pub(crate) fn available_platforms_with_key(&self, key: &str) -> Vec<String> {
        list_available_platforms_with_key(self.raw, key)
    }
}

pub(crate) fn is_truthy(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1")
}

pub(crate) fn is_falsey(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "false" | "0")
}

pub(crate) fn bool_value_or_default(key: &str, value: &toml::Value, default: bool) -> bool {
    bool_value(key, value).unwrap_or(default)
}

pub(crate) fn bool_value(key: &str, value: &toml::Value) -> Option<bool> {
    let parsed = match value {
        toml::Value::Boolean(value) => Some(*value),
        toml::Value::String(value) => parse_bool_str(value),
        toml::Value::Integer(0) => Some(false),
        toml::Value::Integer(1) => Some(true),
        _ => None,
    };
    if parsed.is_none() {
        warn_invalid_bool_value(key, value);
    }
    parsed
}

fn bool_str_or_default(key: &str, value: &str, default: bool) -> bool {
    parse_bool_str(value).unwrap_or_else(|| {
        warn_invalid_bool_value(key, value);
        default
    })
}

fn parse_bool_str(value: &str) -> Option<bool> {
    if is_truthy(value) {
        Some(true)
    } else if is_falsey(value) {
        Some(false)
    } else {
        None
    }
}

fn warn_invalid_bool_value(key: &str, value: impl std::fmt::Display) {
    warn!(
        "invalid boolean value for tool option `{key}`: {value}; expected true, false, 1, or 0; using default"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;

    fn opts_with_value(key: &str, value: toml::Value) -> ToolVersionOptions {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(key.to_string(), value);
        opts
    }

    #[test]
    fn test_platform_value_prefers_platform_value() {
        use crate::backend::static_helpers::platform_aliases;

        let mut opts = opts_with_value("filter_bins", toml::Value::String("base".into()));
        let (os, arch) = platform_aliases().into_iter().next().unwrap();
        let mut linux = toml::Table::new();
        linux.insert(
            "filter_bins".into(),
            toml::Value::Array(vec![toml::Value::String("platform".into())]),
        );
        let mut platforms = toml::Table::new();
        platforms.insert(format!("{os}-{arch}"), toml::Value::Table(linux));
        opts.opts
            .insert("platforms".into(), toml::Value::Table(platforms));

        assert_eq!(
            BackendOptions::new(&opts).platform_value_without_base("filter_bins"),
            Some(&toml::Value::Array(vec![toml::Value::String(
                "platform".into()
            )]))
        );
    }

    #[test]
    fn test_comma_joined_accepts_string_or_array() {
        let string_opts = opts_with_value("tags", toml::Value::String("sqlite,fts5".into()));
        assert_eq!(
            BackendOptions::new(&string_opts)
                .comma_joined("tags")
                .as_deref(),
            Some("sqlite,fts5")
        );

        let array_opts = opts_with_value(
            "tags",
            toml::Value::Array(vec![
                toml::Value::String("sqlite".into()),
                toml::Value::Integer(1),
                toml::Value::String("fts5".into()),
            ]),
        );
        assert_eq!(
            BackendOptions::new(&array_opts)
                .comma_joined("tags")
                .as_deref(),
            Some("sqlite,fts5")
        );
    }

    #[test]
    fn test_bool_parses_consistent_formats() {
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::Boolean(true))).bool("flag")
        );
        assert!(
            !BackendOptions::new(&opts_with_value("flag", toml::Value::Boolean(false)))
                .bool("flag")
        );
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::String("TRUE".into())))
                .bool("flag")
        );
        assert!(
            !BackendOptions::new(&opts_with_value(
                "flag",
                toml::Value::String("FALSE".into())
            ))
            .bool("flag")
        );
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::String("1".into())))
                .bool("flag")
        );
        assert!(
            !BackendOptions::new(&opts_with_value("flag", toml::Value::String("0".into())))
                .bool("flag")
        );
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::Integer(1))).bool("flag")
        );
        assert!(
            !BackendOptions::new(&opts_with_value("flag", toml::Value::Integer(0))).bool("flag")
        );
    }

    #[test]
    fn test_bool_invalid_values_fall_back_to_default() {
        assert!(!BackendOptions::new(&ToolVersionOptions::default()).bool("missing"));
        assert!(
            !BackendOptions::new(&opts_with_value("flag", toml::Value::String("00".into())))
                .bool("flag")
        );
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::String("00".into())))
                .bool_with_default("flag", true)
        );
        assert!(
            BackendOptions::new(&opts_with_value("flag", toml::Value::Integer(2)))
                .bool_with_default("flag", true)
        );
        assert_eq!(bool_value("flag", &toml::Value::String("00".into())), None);
    }

    #[test]
    fn test_platform_bool_for_target_uses_requested_target() {
        let mut opts = ToolVersionOptions::default();
        let mut platforms = toml::Table::new();
        let mut linux = toml::Table::new();
        let mut windows = toml::Table::new();
        linux.insert("no_app".into(), toml::Value::Boolean(false));
        windows.insert("no_app".into(), toml::Value::Boolean(true));
        platforms.insert("linux-x64".into(), toml::Value::Table(linux));
        platforms.insert("windows-x64".into(), toml::Value::Table(windows));
        opts.opts
            .insert("platforms".into(), toml::Value::Table(platforms));

        let values = BackendOptions::new(&opts);
        let linux = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let windows = PlatformTarget::new(Platform::parse("windows-x64").unwrap());

        assert!(!values.platform_bool_for_target("no_app", &linux));
        assert!(values.platform_bool_for_target("no_app", &windows));
    }

    #[test]
    fn test_version_order_parses_options() {
        assert_eq!(
            VersionOrder::from_options(&ToolVersionOptions::default()).unwrap(),
            VersionOrder::Source
        );
        assert_eq!(
            VersionOrder::from_options(&opts_with_value(
                "version_order",
                toml::Value::String("semver".into())
            ))
            .unwrap(),
            VersionOrder::Semver
        );
        assert_eq!(
            VersionOrder::from_options(&opts_with_value(
                "version_order",
                toml::Value::String("chronological".into())
            ))
            .unwrap_err()
            .to_string(),
            "version_order must be \"source\" or \"semver\""
        );
    }

    #[test]
    fn test_semver_order_ranks_semver_after_opaque_versions() {
        let versions = ["2.0.0", "nightly", "1.0.0", "edge", "3.0.0"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            VersionOrder::Semver.order(versions),
            ["nightly", "edge", "1.0.0", "2.0.0", "3.0.0"]
        );
    }

    #[test]
    fn test_semver_order_preserves_equal_precedence_and_opaque_order() {
        let versions = ["nightly", "edge", "1.0.0+002", "1.0.0+001", "1.0.0"]
            .map(String::from)
            .to_vec();
        assert_eq!(VersionOrder::Semver.order(versions.clone()), versions);
    }

    #[test]
    fn test_semver_order_accepts_v_prefixed_versions() {
        let versions = ["v11.11.0", "nightly", "v10.99.0", "v10.34.5"]
            .map(String::from)
            .to_vec();
        assert_eq!(
            VersionOrder::Semver.order(versions),
            ["nightly", "v10.34.5", "v10.99.0", "v11.11.0"]
        );
    }
}
