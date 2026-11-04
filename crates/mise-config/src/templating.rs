use std::ops::Deref;

use serde::Deserialize;
use serde::de::DeserializeOwned;

/// A `[bootstrap]` entry that retains its raw TOML so string values can be
/// rendered as templates before the entry is used.
///
/// Derefs to the unrendered parsed value: callers that only inspect the
/// declaration (tests, counters) need no changes, while callers that build a
/// request from it render it first. Rendering needs the config's template context, so mise
/// provides it outside this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct Templated<T> {
    pub raw: toml::Value,
    pub parsed: T,
    pub ignored: Vec<String>,
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Templated<T> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Capturing the raw table consumes every key, so the outer
        // `serde_ignored` pass over the config can no longer see unknown fields
        // inside this entry. Record them here instead and let the caller report
        // them with the section and entry name.
        let raw = toml::Value::deserialize(deserializer)?;
        let mut ignored = vec![];
        let parsed = serde_ignored::deserialize(raw.clone(), |path| ignored.push(path.to_string()))
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            raw,
            parsed,
            ignored,
        })
    }
}

// The schema describes the entry as written, which is `T` before its strings are rendered.
impl<T: schemars::JsonSchema> schemars::JsonSchema for Templated<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        T::json_schema(generator)
    }
}

impl<T> Deref for Templated<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.parsed
    }
}

impl<T> Templated<T> {
    /// Field paths inside this entry that no field of `T` claimed, relative to
    /// the entry itself. The caller prefixes them with the section and entry
    /// name when warning.
    pub fn ignored_fields(&self) -> &[String] {
        &self.ignored
    }
}
