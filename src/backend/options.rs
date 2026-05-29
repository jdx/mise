use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    list_available_platforms_with_key, lookup_platform_key_for_target, lookup_with_fallback,
};
use crate::toolset::ToolVersionOptions;

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
}
