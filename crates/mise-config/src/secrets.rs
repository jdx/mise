use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// `[secrets]`, which mise reads as raw TOML so a malformed one cannot fail the whole file; its
// own parser warns about a source it does not know. These types describe what it accepts, and
// a test in mise checks that it reads the same keys.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
// Open: a source other than fnox warns and is ignored.
#[schemars(extend("additionalProperties" = true))]
pub struct SecretsDecl {
    /// Use fnox (fnox.toml discovered from this config's directory). https://mise.jdx.dev/environments/secrets/fnox.html
    pub fnox: Option<FnoxDecl>,
}

#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FnoxDecl {
    /// fnox profile (fnox -P); defaults to FNOX_PROFILE, then "default"
    pub profile: Option<String>,
}

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
