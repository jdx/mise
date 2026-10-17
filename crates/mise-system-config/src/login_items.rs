use serde::Deserialize;

#[derive(Debug, Default, Clone, Deserialize)]
pub struct LoginItemsTomlConfig {
    /// application bundles to open at login: absolute or `~/` `.app` paths
    #[serde(default)]
    pub apps: Vec<String>,
    /// also remove login items that are not declared
    #[serde(default)]
    pub exclusive: Option<bool>,
}
