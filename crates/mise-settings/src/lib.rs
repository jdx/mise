//! mise's settings: the types generated from `settings.toml` and the
//! process-wide cache that [`Settings::get`] reads.
//!
//! Loading settings means discovering config files, checking trust, and
//! rendering templates, all of which live in mise itself. mise registers that
//! loader once at startup with [`set_loader`]; [`Settings::try_get`] returns the
//! cached value or runs the loader. Keeping only the types and the cache here
//! lets lower-level crates read settings without depending on mise's config
//! system.

use confique::Config;
use confique::env::parse::{list_by_colon, list_by_comma};
use eyre::{Result, bail};
use indexmap::{IndexMap, indexmap};
use itertools::Itertools;
use serde::ser::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeSet, HashSet};
use std::env::consts::{ARCH, OS};
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock as Lazy;
use std::sync::{Arc, OnceLock, RwLock};

// settings are generated from settings.toml in the project root
// make sure you run `mise run render` after updating settings.toml
include!(concat!(env!("OUT_DIR"), "/settings.rs"));

pub enum SettingsType {
    Bool,
    String,
    Integer,
    Duration,
    Path,
    Url,
    ListString,
    ListPath,
    SetString,
    IndexMap,
    BoolOrString,
}

pub struct SettingsMeta {
    // pub key: String,
    pub type_: SettingsType,
    pub description: &'static str,
    pub env: Option<&'static str>,
    pub deprecated: Option<&'static str>,
    pub deprecated_warn_at: Option<&'static str>,
    pub deprecated_remove_at: Option<&'static str>,
    pub global_only: bool,
    /// Consumed before config files are read, so a value in one can never apply.
    pub env_only: bool,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SettingsStatusMissingTools {
    /// never show the warning
    Never,
    /// hide this warning if the user hasn't installed at least 1 version of the tool before
    #[default]
    IfOtherVersionsInstalled,
    /// always show the warning if tools are missing
    Always,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum NpmPackageManager {
    #[default]
    Auto,
    Npm,
    Aube,
    AubeCli,
    Bun,
    Pnpm,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SystemDepsMode {
    /// prompt to install missing plugin system dependencies (falls back to `warn` non-interactively)
    #[default]
    Prompt,
    /// install missing plugin system dependencies without prompting
    Auto,
    /// print missing plugin system dependencies and continue
    Warn,
    /// skip the plugin system dependency check
    Ignore,
}

/// How task output is presented. mise's task runner resolves it further; see
/// `TaskOutputExt` there.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::EnumIs,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum TaskOutput {
    Interleave,
    KeepOrder,
    #[default]
    Prefix,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PythonUvVenvAuto {
    #[default]
    Off,
    Source,
    CreateSource,
    LegacyTrue,
}

impl PythonUvVenvAuto {
    pub fn should_source(self) -> bool {
        matches!(self, Self::Source | Self::CreateSource | Self::LegacyTrue)
    }

    pub fn should_create(self) -> bool {
        matches!(self, Self::CreateSource | Self::LegacyTrue)
    }

    /// `true`, which mise deprecated in favor of `"create|source"`. mise warns when a
    /// loaded setting still uses it.
    pub fn is_legacy_true(self) -> bool {
        matches!(self, Self::LegacyTrue)
    }
}

impl<'de> Deserialize<'de> for PythonUvVenvAuto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{self, Visitor};
        use std::fmt;

        struct PythonUvVenvAutoVisitor;

        impl<'de> Visitor<'de> for PythonUvVenvAutoVisitor {
            type Value = PythonUvVenvAuto;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a boolean, \"source\", or \"create|source\"")
            }

            fn visit_bool<E>(self, value: bool) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                Ok(if value {
                    PythonUvVenvAuto::LegacyTrue
                } else {
                    PythonUvVenvAuto::Off
                })
            }

            fn visit_str<E>(self, value: &str) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                let normalized = value.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "source" => Ok(PythonUvVenvAuto::Source),
                    "create|source" => Ok(PythonUvVenvAuto::CreateSource),
                    "true" | "yes" | "1" => self.visit_bool(true),
                    "false" | "no" | "0" => self.visit_bool(false),
                    _ => Err(E::invalid_value(de::Unexpected::Str(value), &self)),
                }
            }

            fn visit_string<E>(self, value: String) -> Result<PythonUvVenvAuto, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_any(PythonUvVenvAutoVisitor)
    }
}

impl serde::Serialize for PythonUvVenvAuto {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            PythonUvVenvAuto::Off => serializer.serialize_bool(false),
            PythonUvVenvAuto::LegacyTrue => serializer.serialize_bool(true),
            PythonUvVenvAuto::Source => serializer.serialize_str("source"),
            PythonUvVenvAuto::CreateSource => serializer.serialize_str("create|source"),
        }
    }
}

fn remove_empty_nested_settings(table: &mut toml::Table, prefix: &str) {
    table.retain(|key, value| {
        let path = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        let Some(child) = value.as_table_mut() else {
            return true;
        };
        remove_empty_nested_settings(child, &path);
        !child.is_empty() || SETTINGS_META.contains_key(path.as_str())
    });
}

/// Replace secret values in a serialized settings table.
pub fn redact_settings_table(table: &mut toml::Table) {
    let Some(cache) = table
        .get_mut("task")
        .and_then(toml::Value::as_table_mut)
        .and_then(|task| task.get_mut("cache"))
        .and_then(toml::Value::as_table_mut)
    else {
        return;
    };
    if cache.contains_key("remote_token") {
        cache.insert(
            "remote_token".to_string(),
            toml::Value::String("[redacted]".to_string()),
        );
    }
}

pub type SettingsPartial = <Settings as Config>::Layer;

impl Display for Settings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{s}"),
            Err(e) => Err(std::fmt::Error::custom(e)),
        }
    }
}

impl SettingsStatus {
    pub fn missing_tools(&self) -> SettingsStatusMissingTools {
        SettingsStatusMissingTools::from_str(&self.missing_tools).unwrap()
    }
}

/// Deserialize a string to a boolean, accepting "false", "no", "0"
/// and their case-insensitive variants as `false`. Any other value (incl. "") is considered `true`.
fn bool_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match s.to_lowercase().as_str() {
        "false" | "no" | "0" => Ok(false),
        _ => Ok(true),
    }
}

fn set_by_comma<T, C>(input: &str) -> Result<C, <T as FromStr>::Err>
where
    T: FromStr + Eq + Ord,
    C: FromIterator<T>,
{
    input
        .split(',')
        // Filter out empty strings
        .filter_map(|s| {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                Some(T::from_str(trimmed))
            } else {
                None
            }
        })
        // collect into BTreeSet to remove duplicates
        .collect::<Result<BTreeSet<_>, _>>()
        .map(|set| set.into_iter().collect())
}

fn validate_setting_enum_values<'a>(
    name: &str,
    values: impl IntoIterator<Item = &'a str>,
    allowed: &[&str],
) -> Result<()> {
    if let Some(invalid) = values.into_iter().find(|value| !allowed.contains(value)) {
        bail!(
            "invalid {name} value {invalid:?}; expected one of: {}",
            allowed.join(", ")
        );
    }
    Ok(())
}

fn normalize_tool_names(tools: &BTreeSet<String>) -> BTreeSet<String> {
    tools
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Parse URL replacements from JSON string format
/// Expected format: {"source_domain": "replacement_domain", ...}
pub fn parse_url_replacements(input: &str) -> Result<IndexMap<String, String>, serde_json::Error> {
    serde_json::from_str(input)
}

/// Parse a path list from an environment variable using the OS-native path
/// separator (`:` on Unix, `;` on Windows). This correctly handles Windows
/// absolute paths whose drive letters contain `:` (e.g. `C:\foo`).
fn list_by_os_path_separator<C>(input: &str) -> Result<C, std::convert::Infallible>
where
    C: FromIterator<PathBuf>,
{
    Ok(std::env::split_paths(input)
        .filter(|p| !p.as_os_str().is_empty())
        .collect())
}

mod accessors;
mod cache;
pub use cache::{Loader, clear, is_loaded, last_cached, load_defaults, set_loader, store};

#[cfg(test)]
mod tests;
