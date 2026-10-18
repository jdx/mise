use serde::Deserialize;

/// A `[bootstrap.packages]` value. The string form remains the concise default;
/// the table form adds platform selection without changing package keys.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum PackageTomlConfig {
    Version(String),
    Options(PackageOptionsTomlConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageOptionsTomlConfig {
    #[serde(default = "latest_package_version")]
    pub version: String,
    #[serde(default, deserialize_with = "deserialize_package_os")]
    pub os: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_package_env")]
    pub env: Vec<String>,
    /// Adopt an identical existing cask artifact instead of replacing it.
    #[serde(default)]
    pub adopt: Option<bool>,
    /// `brew-cask` only: application directory for this cask and its cask
    /// dependencies. Takes precedence over MISE_BREW_CASK_OPT_APPDIR.
    #[serde(default)]
    pub appdir: Option<String>,
    /// `macos-app` only: direct download URL for the app archive.
    #[serde(default)]
    pub url: Option<String>,
    /// `macos-app` only: expected sha256 of the download.
    #[serde(default)]
    pub sha256: Option<String>,
    /// `macos-app` only: bundle to install out of the archive, e.g. "Nuvio.app".
    #[serde(default)]
    pub artifact: Option<String>,
    /// `snap` only: channel to install from and track, e.g. "latest/stable".
    #[serde(default)]
    pub channel: Option<String>,
    /// `snap` only: install with classic confinement.
    #[serde(default)]
    pub classic: Option<bool>,
    #[serde(default)]
    pub state: PackageDesiredStateTomlConfig,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
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
