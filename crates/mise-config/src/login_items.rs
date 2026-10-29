use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct LoginItemsTomlConfig {
    /// application bundles to open at login: absolute or `~/` paths ending in `.app`
    #[serde(default)]
    pub apps: Vec<String>,
    /// also remove login items that are not declared (default: false, so apps that register themselves are left alone)
    #[serde(default)]
    pub exclusive: Option<bool>,
}
