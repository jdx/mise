use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// List of built-in provider names that have specialized implementations
pub const BUILTIN_PROVIDERS: &[&str] = &[
    "npm", "yarn", "pnpm", "bun",      // Node.js
    "go",       // Go
    "pip",      // Python (requirements.txt)
    "poetry",   // Python (poetry)
    "uv",       // Python (uv)
    "bundler",  // Ruby
    "composer", // PHP
];

/// Configuration for a prepare provider (both built-in and custom)
///
/// Built-in providers have auto-detected sources/outputs and default run commands.
/// Custom providers require explicit sources, outputs, and run.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareProviderConfig {
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
    /// Whether to update mtime of output files/dirs after a successful run (default: true)
    /// This is useful when the prepare command is a no-op (e.g., `uv sync` when all is well)
    /// so that the outputs appear fresh for subsequent freshness checks.
    pub touch_outputs: Option<bool>,
}

impl PrepareProviderConfig {
    /// Check if this is a custom rule (has explicit run command and is not a built-in name)
    pub fn is_custom(&self, name: &str) -> bool {
        !BUILTIN_PROVIDERS.contains(&name) && self.run.is_some()
    }
}

/// Top-level [prepare] configuration section
///
/// All providers are configured at the same level:
/// - `[prepare.npm]` - built-in npm provider
/// - `[prepare.codegen]` - custom provider
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PrepareConfig {
    /// List of provider IDs to disable at runtime
    #[serde(default)]
    pub disable: Vec<String>,
    /// All provider configurations (both built-in and custom)
    #[serde(flatten)]
    pub providers: BTreeMap<String, PrepareProviderConfig>,
}

impl PrepareConfig {
    /// Merge two PrepareConfigs, with `other` taking precedence
    pub fn merge(&self, other: &PrepareConfig) -> PrepareConfig {
        let mut providers = self.providers.clone();
        for (k, v) in &other.providers {
            providers.insert(k.clone(), v.clone());
        }

        let mut disable = self.disable.clone();
        disable.extend(other.disable.clone());

        PrepareConfig { disable, providers }
    }

    /// Get a provider config by name
    pub fn get(&self, name: &str) -> Option<&PrepareProviderConfig> {
        self.providers.get(name)
    }
}
