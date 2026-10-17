use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum SecretTomlConfig {
    Env(String),
    Options(SecretOptionsTomlConfig),
}

#[derive(Clone, Debug, Deserialize)]
pub struct SecretOptionsTomlConfig {
    pub env: String,
    pub description: Option<String>,
    #[serde(default)]
    pub allow_empty: bool,
}
