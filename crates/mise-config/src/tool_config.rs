//! `[tool_config]`.

use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// policy for tools declared by configs sharing the containing config root
// Unlike `[settings]`, this is intentionally config-root-owned rather than an invocation-wide
// merged value.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ToolConfig {
    /// require tools declared by this config root to resolve and install from their lockfiles
    pub locked: bool,
}
