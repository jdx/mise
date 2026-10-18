use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FirewallTomlConfig {
    #[serde(flatten)]
    pub values: std::collections::HashMap<String, toml::Value>,
}
