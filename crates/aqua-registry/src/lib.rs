//! Aqua Registry
//!
//! This crate provides functionality for working with Aqua package registry files.
//! It can load registry data from baked-in files, local repositories, or remote HTTP sources.

mod registry;
mod template;
mod types;

// Re-export only what's needed by the main mise crate
pub use registry::{
    AQUA_STANDARD_REGISTRY_FILES, AquaRegistry, DefaultRegistryFetcher, FileCacheStore,
    NoOpCacheStore,
};
pub use types::{AquaChecksumType, AquaMinisignType, AquaPackage, AquaPackageType, RegistryYaml};

use thiserror::Error;

/// Errors that can occur when working with the Aqua registry
#[derive(Error, Debug)]
pub enum AquaRegistryError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("registry not available: {0}")]
    RegistryNotAvailable(String),
    #[error("template error: {0}")]
    TemplateError(#[from] eyre::Error),
    #[error("yaml parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("expression error: {0}")]
    ExpressionError(String),
}

pub type Result<T> = std::result::Result<T, AquaRegistryError>;

/// Configuration for the Aqua registry
#[derive(Debug, Clone)]
pub struct AquaRegistryConfig {
    /// Path to cache directory for cloned repositories
    pub cache_dir: std::path::PathBuf,
    /// URL of the registry repository (if None, only baked registry will be used)
    pub registry_url: Option<String>,
    /// Whether to use the baked-in registry
    pub use_baked_registry: bool,
    /// Whether to skip network operations (prefer offline mode)
    pub prefer_offline: bool,
}

impl Default for AquaRegistryConfig {
    fn default() -> Self {
        Self {
            cache_dir: std::env::temp_dir().join("aqua-registry"),
            registry_url: Some("https://github.com/aquaproj/aqua-registry".to_string()),
            use_baked_registry: true,
            prefer_offline: false,
        }
    }
}

/// Trait for fetching registry files from various sources
#[allow(async_fn_in_trait)]
pub trait RegistryFetcher {
    /// Fetch and parse a registry YAML file for the given package ID
    async fn fetch_registry(&self, package_id: &str) -> Result<crate::types::RegistryYaml>;
}

/// Trait for caching registry data
pub trait CacheStore {
    /// Check if cached data exists and is fresh
    fn is_fresh(&self, key: &str) -> bool;
    /// Store data in cache
    fn store(&self, key: &str, data: &[u8]) -> std::io::Result<()>;
    /// Retrieve data from cache
    fn retrieve(&self, key: &str) -> std::io::Result<Option<Vec<u8>>>;
}
