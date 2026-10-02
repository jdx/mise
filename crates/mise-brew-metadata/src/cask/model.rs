use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

use crate::RubySourceChecksum;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaskUrlSpecs {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub only_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cask {
    pub token: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub aliases: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub old_tokens: Vec<String>,
    pub version: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub auto_updates: bool,
    pub url: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub url_specs: CaskUrlSpecs,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub artifacts: Vec<Value>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub depends_on: CaskDependencies,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub conflicts_with: CaskConflicts,
    #[serde(default)]
    pub ruby_source_path: Option<String>,
    #[serde(default)]
    pub ruby_source_checksum: Option<RubySourceChecksum>,
    #[serde(default)]
    pub tap_git_head: Option<String>,
    #[serde(skip)]
    pub raw_base: Option<String>,
    /// Which manager is installing this cask. Never deserialized: API and tap
    /// metadata is always `brew-cask`, and inline declarations set it directly.
    #[serde(skip)]
    pub manager: CaskManager,
    /// The resolved application directory selected for this install. Cask
    /// metadata never supplies this; mise sets it from package configuration.
    #[serde(skip)]
    pub appdir: Option<PathBuf>,
}

impl Cask {
    /// User-facing manager label for diagnostics.
    pub fn label(&self) -> &'static str {
        self.manager.label()
    }
}

/// Which package manager is driving the shared cask install pipeline.
///
/// `brew-cask` and `macos-app` run the same installer — download, verify,
/// extract, then swap an app bundle into the app directory — and differ only in
/// where the metadata came from and where mise records ownership.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CaskManager {
    /// Metadata resolved from the Homebrew cask API or a tap.
    #[default]
    BrewCask,
    /// Metadata declared inline in `[bootstrap.packages]`.
    MacosApp,
}

impl CaskManager {
    pub fn label(self) -> &'static str {
        match self {
            Self::BrewCask => "brew-cask",
            Self::MacosApp => "macos-app",
        }
    }

    /// True when this manager shares Homebrew's Caskroom and must therefore
    /// arbitrate token ownership with an installed Homebrew.
    pub fn uses_homebrew_caskroom(self) -> bool {
        matches!(self, Self::BrewCask)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaskDependencies {
    #[serde(default)]
    pub formula: Vec<String>,
    #[serde(default)]
    pub cask: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CaskConflicts {
    #[serde(default)]
    pub cask: Vec<String>,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
