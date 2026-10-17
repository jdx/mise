use indexmap::IndexMap;
use serde::Deserialize;

/// `[dotfile_groups]` as written in one config file: group name -> table.
/// Values stay TOML until expansion so a malformed group warns and is
/// skipped instead of failing the whole config file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DotfileGroupsTomlConfig(pub IndexMap<String, toml::Value>);

/// `[dotfiles]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(transparent)]
pub struct DotfilesTomlConfig(pub IndexMap<String, toml::Value>);
