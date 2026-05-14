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

    /// Returns the option as an owned `String`, coercing scalar TOML values to
    /// their string representation.
    pub(crate) fn string(&self, key: &str) -> Option<String> {
        self.raw.get_string(key)
    }

    /// Returns the option only when the underlying TOML value is a string.
    /// Prefer `string()` for options that may be written as native TOML scalars.
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

    pub(crate) fn bool(&self, key: &str) -> bool {
        self.string(key).is_some_and(|v| is_truthy(&v))
    }

    pub(crate) fn platform_bool(&self, key: &str) -> bool {
        self.platform_string(key).is_some_and(|v| is_truthy(&v))
    }

    pub(crate) fn available_platforms_with_key(&self, key: &str) -> Vec<String> {
        list_available_platforms_with_key(self.raw, key)
    }
}

pub(crate) fn is_truthy(value: &str) -> bool {
    matches!(value.trim(), "true" | "1")
}
