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
