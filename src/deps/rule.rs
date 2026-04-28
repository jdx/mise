use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// List of built-in provider names that have specialized implementations
pub const BUILTIN_PROVIDERS: &[&str] = &[
    "npm",
    "yarn",
    "pnpm",
    "bun",           // Node.js
    "aube",          // Node.js
    "go",            // Go
    "pip",           // Python (requirements.txt)
    "poetry",        // Python (poetry)
    "uv",            // Python (uv)
    "bundler",       // Ruby
    "composer",      // PHP
    "git-submodule", // Git
];

/// Configuration for a deps provider (both built-in and custom)
///
/// Built-in providers have auto-detected sources/outputs and default run commands.
/// Custom providers require explicit sources, outputs, and run.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DepsProviderConfig {
    /// Whether to auto-run this provider before mise x/run (default: false)
    #[serde(default)]
    pub auto: bool,
    /// Command to run when stale (required for custom, optional override for built-in)
    pub run: Option<String>,
    /// Files/patterns to check for changes (required for custom, auto-detected for built-in)
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files/directories that should be newer than sources (required for custom, auto-detected for built-in)
    #[serde(default)]
    pub outputs: Vec<String>,
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

impl DepsProviderConfig {
    /// Check if this is a custom rule (has explicit run command and is not a built-in name)
    pub fn is_custom(&self, name: &str) -> bool {
        !BUILTIN_PROVIDERS.contains(&name) && self.run.is_some()
    }
}

/// Top-level [deps] configuration section
///
/// All providers are configured at the same level:
/// - `[deps.npm]` - built-in npm provider
/// - `[deps.codegen]` - custom provider
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DepsConfig {
    /// List of provider IDs to disable at runtime
    #[serde(default)]
    pub disable: Vec<String>,
    /// All provider configurations (both built-in and custom)
    #[serde(flatten)]
    pub providers: BTreeMap<String, DepsProviderConfig>,
}

impl DepsConfig {
    /// Merge two DepsConfigs, with `other` taking precedence
    pub fn merge(&self, other: &DepsConfig) -> DepsConfig {
        let mut providers = self.providers.clone();
        for (k, v) in &other.providers {
            providers.insert(k.clone(), v.clone());
        }

        let mut disable = self.disable.clone();
        disable.extend(other.disable.clone());

        DepsConfig { disable, providers }
    }

    /// Get a provider config by name
    pub fn get(&self, name: &str) -> Option<&DepsProviderConfig> {
        self.providers.get(name)
    }
}
