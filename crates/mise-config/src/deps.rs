//! `[deps]`: the providers that bring a project's dependencies up to date.

pub(crate) mod schema;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Configuration for a deps provider (both built-in and custom)
///
/// Built-in providers have auto-detected sources/outputs and default run commands.
/// Custom providers require explicit sources, outputs, and run.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepsProviderDecl {
    /// Whether to auto-run this provider before mise x/run (default: false)
    #[serde(default)]
    pub auto: bool,
    /// Command to run when stale (required for custom, optional override for built-in)
    pub run: Option<String>,
    /// Files/patterns to check for changes (required for custom, auto-detected for built-in)
    pub sources: Option<Vec<String>>,
    /// Files/directories that should be newer than sources (required for custom, auto-detected for built-in)
    pub outputs: Option<Vec<String>>,
    /// Environment variables to set
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory
    pub dir: Option<String>,
    /// Optional description
    pub description: Option<String>,
    /// Other deps providers that must complete before this one runs
    #[serde(default)]
    pub depends: Vec<String>,
    /// Timeout for the run command (e.g., "30s", "5m", "1h")
    pub timeout: Option<String>,
}

/// Top-level [deps] configuration section
///
/// All providers are configured at the same level:
/// - `[deps.npm]` - built-in npm provider
/// - `[deps.codegen]` - custom provider
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DepsDecl {
    /// List of provider IDs to disable at runtime
    #[serde(default)]
    pub disable: Vec<String>,
    /// All provider configurations (both built-in and custom)
    #[serde(flatten)]
    pub providers: BTreeMap<String, DepsProviderDecl>,
}
