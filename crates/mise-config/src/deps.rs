//! `[deps]`: the providers that bring a project's dependencies up to date.
//!
//! The doc comments on these types are their descriptions in `schema/mise.json`, which editors
//! show to users. Notes for Rust readers go in plain comments.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The built-in providers, which have specialized implementations in mise. The schema lists
/// them by name so an editor can offer them; any other name is a custom provider.
pub const BUILTIN_PROVIDERS: &[&str] = &[
    "npm",
    "yarn",
    "pnpm",
    "bun",           // Node.js
    "aube",          // Node.js
    "deno",          // Deno
    "go",            // Go
    "pip",           // Python (requirements.txt)
    "poetry",        // Python (poetry)
    "uv",            // Python (uv)
    "bundler",       // Ruby
    "composer",      // PHP
    "dart",          // Dart
    "flutter",       // Flutter
    "git-submodule", // Git
];

/// Prepare provider configuration
// Built-in providers have auto-detected sources/outputs and default run commands. Custom
// providers require explicit sources, outputs, and run.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct DepsProviderDecl {
    /// Auto-run before `mise x` and `mise run`
    #[serde(default)]
    pub auto: bool,
    /// Command to run when stale
    pub run: Option<String>,
    /// Files/patterns to check for changes
    pub sources: Option<Vec<String>>,
    /// Files/directories that should be newer than sources
    pub outputs: Option<Vec<String>>,
    /// Environment variables to set
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, crate::schema::EnvVarValue>")]
    pub env: BTreeMap<String, String>,
    /// Working directory for the command
    pub dir: Option<String>,
    /// Description shown in output
    pub description: Option<String>,
    /// Other deps providers that must complete before this one runs
    #[serde(default)]
    pub depends: Vec<String>,
    /// Timeout for the run command (e.g., "30s", "5m", "1h")
    pub timeout: Option<String>,
}

// Top-level [deps] configuration section. All providers are configured at the same level:
// `[deps.npm]` is a built-in provider and `[deps.codegen]` a custom one.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[schemars(transform = named_providers)]
pub struct DepsDecl {
    /// Disable specific deps providers
    #[serde(default)]
    pub disable: Vec<String>,
    /// All provider configurations (both built-in and custom)
    #[serde(flatten)]
    #[schemars(skip)]
    pub providers: BTreeMap<String, DepsProviderDecl>,
}

// The flattened map is every provider, so the derive skips it and this adds them: the built-in
// ones by name, as properties, so an editor offers them, and any other name as a custom one.
fn named_providers(schema: &mut schemars::Schema) {
    let provider = serde_json::json!({"$ref": "#/$defs/deps_provider"});
    if let Some(properties) = schema
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    {
        for name in BUILTIN_PROVIDERS {
            properties.insert((*name).into(), provider.clone());
        }
    }
    schema.insert("additionalProperties".into(), provider);
}
