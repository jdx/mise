use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum SecretTomlConfig {
    /// environment variable that provides this secret's value
    Env(#[schemars(regex(pattern = r"^[A-Za-z_][A-Za-z0-9_]*$"))] String),
    Options(SecretOptionsTomlConfig),
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SecretOptionsTomlConfig {
    /// environment variable that provides this secret's value
    #[schemars(regex(pattern = r"^[A-Za-z_][A-Za-z0-9_]*$"))]
    pub env: String,
    /// human-readable description shown in status output and prompts
    pub description: Option<String>,
    /// accept an empty value; empty values are rejected by default
    #[serde(default)]
    pub allow_empty: bool,
}
