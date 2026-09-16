use serde::Deserialize;
use serde_json::Value;

use super::RubySourceChecksum;

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct CaskUrlSpecs {
    #[serde(default)]
    pub(super) branch: Option<String>,
    #[serde(default)]
    pub(super) only_path: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(in crate::system::packages::brew) struct Cask {
    pub(super) token: String,
    #[serde(default)]
    pub(super) aliases: Vec<String>,
    #[serde(default)]
    pub(super) old_tokens: Vec<String>,
    pub(super) version: String,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) auto_updates: bool,
    pub(super) url: String,
    #[serde(default)]
    pub(super) url_specs: CaskUrlSpecs,
    #[serde(default)]
    pub(super) sha256: Option<String>,
    #[serde(default)]
    pub(super) artifacts: Vec<Value>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) depends_on: CaskDependencies,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub(super) conflicts_with: CaskConflicts,
    #[serde(default)]
    pub(super) ruby_source_path: Option<String>,
    #[serde(default)]
    pub(super) ruby_source_checksum: Option<RubySourceChecksum>,
    #[serde(default)]
    pub(super) tap_git_head: Option<String>,
    #[serde(skip)]
    pub(super) raw_base: Option<String>,
    /// Which manager is installing this cask. Never deserialized: API and tap
    /// metadata is always `brew-cask`, and inline declarations set it directly.
    #[serde(skip)]
    pub(super) manager: CaskManager,
}

impl Cask {
    /// User-facing manager label for diagnostics.
    pub(in crate::system::packages::brew) fn label(&self) -> &'static str {
        self.manager.label()
    }
}

/// Which package manager is driving the shared cask install pipeline.
///
/// `brew-cask` and `macos-app` run the same installer — download, verify,
/// extract, then swap an app bundle into the app directory — and differ only in
/// where the metadata came from and where mise records ownership.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::system::packages::brew) enum CaskManager {
    /// Metadata resolved from the Homebrew cask API or a tap.
    #[default]
    BrewCask,
    /// Metadata declared inline in `[bootstrap.packages]`.
    MacosApp,
}

impl CaskManager {
    pub(in crate::system::packages::brew) fn label(self) -> &'static str {
        match self {
            Self::BrewCask => "brew-cask",
            Self::MacosApp => "macos-app",
        }
    }

    /// True when this manager shares Homebrew's Caskroom and must therefore
    /// arbitrate token ownership with an installed Homebrew.
    pub(in crate::system::packages::brew) fn uses_homebrew_caskroom(self) -> bool {
        matches!(self, Self::BrewCask)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct CaskDependencies {
    #[serde(default)]
    pub(super) formula: Vec<String>,
    #[serde(default)]
    pub(super) cask: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(super) struct CaskConflicts {
    #[serde(default)]
    pub(super) cask: Vec<String>,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}
