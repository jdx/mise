//! Provider traits for injecting external data into the editor
//!
//! These traits allow the main mise crate to inject data (like the tool registry)
//! without creating circular dependencies.

use async_trait::async_trait;

/// Information about a tool from the registry
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool name (e.g., "node", "python")
    pub name: String,
    /// Short description of the tool
    pub description: Option<String>,
    /// Aliases for this tool
    pub aliases: Vec<String>,
}

/// Information about a setting
#[derive(Debug, Clone)]
pub struct SettingInfo {
    /// Setting name (e.g., "experimental", "jobs")
    pub name: String,
    /// Description of what this setting does
    pub description: Option<String>,
    /// Type of the setting (for display and validation)
    pub setting_type: SettingType,
    /// Default value as string
    pub default: Option<String>,
}

/// Type of a setting value
#[derive(Debug, Clone, PartialEq)]
pub enum SettingType {
    /// Boolean (true/false)
    Bool,
    /// Integer number
    Integer,
    /// String value
    String,
    /// Array of strings
    StringArray,
    /// Duration (e.g., "1h", "30m")
    Duration,
    /// Path on filesystem
    Path,
    /// Enum with specific allowed values
    Enum(Vec<String>),
}

/// Information about a backend
#[derive(Debug, Clone)]
pub struct BackendInfo {
    /// Backend name (e.g., "cargo", "npm")
    pub name: String,
    /// Description of the backend
    pub description: Option<String>,
}

/// Provider for tool information from the registry
pub trait ToolProvider: Send + Sync {
    /// List all available tools
    fn list_tools(&self) -> Vec<ToolInfo>;
}

/// Provider for backend information
pub trait BackendProvider: Send + Sync {
    /// List all available backends
    fn list_backends(&self) -> Vec<BackendInfo>;
}

/// Default empty backend provider
pub struct EmptyBackendProvider;

impl BackendProvider for EmptyBackendProvider {
    fn list_backends(&self) -> Vec<BackendInfo> {
        Vec::new()
    }
}

/// Provider for setting information
pub trait SettingProvider: Send + Sync {
    /// List all available settings
    fn list_settings(&self) -> Vec<SettingInfo>;
}

/// Default empty tool provider (no tools available)
pub struct EmptyToolProvider;

impl ToolProvider for EmptyToolProvider {
    fn list_tools(&self) -> Vec<ToolInfo> {
        Vec::new()
    }
}

/// Default empty setting provider (no settings available)
pub struct EmptySettingProvider;

impl SettingProvider for EmptySettingProvider {
    fn list_settings(&self) -> Vec<SettingInfo> {
        Vec::new()
    }
}

/// Provider for tool version information
#[async_trait]
pub trait VersionProvider: Send + Sync {
    /// Get the latest version of a tool
    ///
    /// Returns the full version string (e.g., "3.12.4" for python)
    async fn latest_version(&self, tool: &str) -> Option<String>;
}

/// Default empty version provider
pub struct EmptyVersionProvider;

#[async_trait]
impl VersionProvider for EmptyVersionProvider {
    async fn latest_version(&self, _tool: &str) -> Option<String> {
        None
    }
}

/// Marker for the custom version entry option
pub const VERSION_CUSTOM_MARKER: &str = "other...";

/// Generate version variants from a full version string
///
/// Given "3.12.4", returns ["latest", "3", "3.12", "3.12.4", "other..."]
pub fn version_variants(full_version: &str) -> Vec<String> {
    let mut variants = vec!["latest".to_string()];

    // Parse version segments
    let parts: Vec<&str> = full_version.split('.').collect();

    // Build progressive versions
    let mut current = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            current.push('.');
        }
        current.push_str(part);
        variants.push(current.clone());
    }

    // Add custom entry option at the end
    variants.push(VERSION_CUSTOM_MARKER.to_string());

    variants
}
