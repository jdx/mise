use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Configuration for a user-defined prepare rule
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareRule {
    /// Files/patterns to check for changes (sources)
    #[serde(default)]
    pub sources: Vec<String>,
    /// Files/directories that should be newer than sources
    #[serde(default)]
    pub outputs: Vec<String>,
    /// Command to run when stale
    pub run: String,
    /// Optional description
    pub description: Option<String>,
    /// Whether this rule is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Working directory
    pub dir: Option<String>,
    /// Environment variables
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Priority (higher runs first, default: 100)
    #[serde(default = "default_priority")]
    pub priority: u32,
}

/// Configuration for overriding a built-in provider
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareProviderConfig {
    /// Whether this provider is enabled (default: true)
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Custom command to run (overrides default)
    pub run: Option<String>,
    /// Additional sources to watch beyond the defaults
    #[serde(default)]
    pub extra_sources: Vec<String>,
    /// Additional outputs to check beyond the defaults
    #[serde(default)]
    pub extra_outputs: Vec<String>,
    /// Environment variables to set
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory
    pub dir: Option<String>,
}

/// Top-level [prepare] configuration section
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareConfig {
    /// Master switch to enable/disable auto-prepare (default: true)
    #[serde(default = "default_true")]
    pub auto: bool,
    /// List of provider IDs to disable
    #[serde(default)]
    pub disable: Vec<String>,
    /// User-defined prepare rules
    #[serde(default)]
    pub rules: BTreeMap<String, PrepareRule>,
    /// NPM provider configuration override
    pub npm: Option<PrepareProviderConfig>,
    /// Cargo provider configuration override (future)
    pub cargo: Option<PrepareProviderConfig>,
    /// Go provider configuration override (future)
    pub go: Option<PrepareProviderConfig>,
    /// Python/pip provider configuration override (future)
    pub python: Option<PrepareProviderConfig>,
}

impl PrepareConfig {
    /// Merge two PrepareConfigs, with `other` taking precedence
    pub fn merge(&self, other: &PrepareConfig) -> PrepareConfig {
        let mut rules = self.rules.clone();
        rules.extend(other.rules.clone());

        let mut disable = self.disable.clone();
        disable.extend(other.disable.clone());

        PrepareConfig {
            auto: other.auto,
            disable,
            rules,
            npm: other.npm.clone().or_else(|| self.npm.clone()),
            cargo: other.cargo.clone().or_else(|| self.cargo.clone()),
            go: other.go.clone().or_else(|| self.go.clone()),
            python: other.python.clone().or_else(|| self.python.clone()),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_priority() -> u32 {
    100
}
