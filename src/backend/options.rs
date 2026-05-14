use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{lookup_platform_key_for_target, lookup_with_fallback};
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
            .is_some_and(|v| is_truthy(&v))
    }
}

pub(crate) fn is_truthy(value: &str) -> bool {
    matches!(value.trim(), "true" | "1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;

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
