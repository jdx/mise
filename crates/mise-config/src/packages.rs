use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// A `[bootstrap.packages]` value. The string form remains the concise default; the table form
// adds platform selection without changing package keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum PackageTomlConfig {
    /// version pin in the manager's native format, or "latest"
    Version(String),
    Options(PackageOptionsTomlConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("dependentRequired" = {"url": ["sha256", "artifact", "version"]}))]
pub struct PackageOptionsTomlConfig {
    /// version pin in the manager's native format, or "latest"
    #[serde(default = "latest_package_version")]
    pub version: String,
    /// operating system or OS/architecture selectors
    #[serde(default, deserialize_with = "deserialize_package_os")]
    #[schemars(schema_with = "package_os")]
    pub os: Vec<String>,
    /// mise environments in which this package is enabled
    #[serde(default, deserialize_with = "deserialize_package_env")]
    #[schemars(schema_with = "package_env")]
    pub env: Vec<String>,
    /// adopt an existing identical app artifact instead of replacing it (brew-cask and macos-app)
    #[serde(default)]
    pub adopt: Option<bool>,
    /// brew-cask only: application directory for this cask and its cask dependencies; takes precedence over MISE_BREW_CASK_OPT_APPDIR
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub appdir: Option<String>,
    /// macos-app only: download URL for the app archive; {{version}} is replaced with the declared version
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub url: Option<String>,
    /// macos-app only: expected sha256 of the download as 64 hex characters; required with url. Homebrew's no_check sentinel is not accepted
    #[serde(default)]
    #[schemars(regex(pattern = r"^[0-9a-fA-F]{64}$"))]
    pub sha256: Option<String>,
    /// macos-app only: app bundle to install from the archive, e.g. Nuvio.app; required with url
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r".*\S.*"))]
    pub artifact: Option<String>,
    /// snap only: channel to install from and track, e.g. latest/stable or 22/edge
    #[serde(default)]
    #[schemars(regex(
        pattern = r"^[A-Za-z0-9._][A-Za-z0-9._-]*(/[A-Za-z0-9._][A-Za-z0-9._-]*){0,2}$"
    ))]
    pub channel: Option<String>,
    /// snap only: install with classic confinement
    #[serde(default)]
    pub classic: Option<bool>,
    /// desired package state; pacman, scoop, snap, and zypper support absent
    #[serde(default)]
    pub state: PackageDesiredStateTomlConfig,
}

fn package_os(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/os_filter_item"},
            {"type": "array", "items": {"$ref": "#/$defs/os_filter_item"}, "minItems": 1}
        ]
    })
}

fn package_env(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "oneOf": [
            {"type": "string", "minLength": 1, "pattern": "\\S"},
            {
                "type": "array",
                "items": {"type": "string", "minLength": 1, "pattern": "\\S"},
                "minItems": 1
            }
        ]
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PackageDesiredStateTomlConfig {
    #[default]
    Present,
    Absent,
}

impl PackageTomlConfig {
    pub fn version(&self) -> &str {
        match self {
            Self::Version(version) => version,
            Self::Options(options) => &options.version,
        }
    }

    pub fn adopt(&self) -> Option<bool> {
        match self {
            Self::Options(options) => options.adopt,
            Self::Version(_) => None,
        }
    }

    pub fn appdir(&self) -> Option<&str> {
        match self {
            Self::Options(options) => options.appdir.as_deref(),
            Self::Version(_) => None,
        }
    }

    /// The inline app declaration on this entry, if it carries one.
    ///
    /// `url` is what marks an entry as inline-declared; the other fields are
    /// validated against it by `AppSpec::parse`.
    pub fn app_fields(&self) -> Option<(&str, Option<&str>, Option<&str>, &str)> {
        match self {
            Self::Options(options) => options
                .url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(|url| {
                    (
                        url,
                        options.sha256.as_deref(),
                        options.artifact.as_deref(),
                        options.version.as_str(),
                    )
                }),
            Self::Version(_) => None,
        }
    }

    /// Whether this package is enabled by at least one active mise environment.
    pub fn is_env_supported(&self, environments: &[String]) -> bool {
        let Self::Options(options) = self else {
            return true;
        };
        options.env.is_empty() || options.env.iter().any(|entry| environments.contains(entry))
    }
}

fn latest_package_version() -> String {
    "latest".to_string()
}

pub fn deserialize_package_os<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let values = match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
    };
    if values.is_empty() || values.iter().any(|value| value.is_empty()) {
        return Err(serde::de::Error::custom(
            "os must contain at least one non-empty selector",
        ));
    }
    Ok(values)
}

/// Deserialize one or more non-blank mise environment selectors.
fn deserialize_package_env<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    let values = match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(value) => vec![value],
        OneOrMany::Many(values) => values,
    };
    if values.is_empty() || values.iter().any(|value| value.trim().is_empty()) {
        return Err(serde::de::Error::custom(
            "package env must contain at least one non-empty selector",
        ));
    }
    Ok(values)
}
