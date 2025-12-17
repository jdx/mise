//! Dependency management trait for backends
//!
//! This trait handles tool dependencies and environment resolution.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;
use indexmap::IndexSet;

use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::toolset::Toolset;

use super::identity::BackendIdentity;

/// Trait for dependency management.
///
/// This trait provides methods for:
/// - Declaring tool dependencies
/// - Resolving dependency toolsets
/// - Finding dependency binaries
/// - Checking for missing dependencies
#[async_trait]
pub trait DependencyManager: BackendIdentity {
    // ========== Dependency Declaration ==========

    /// Get required dependencies for this tool.
    ///
    /// If any of these tools are installing in parallel, we should
    /// wait for them to finish before installing this tool.
    fn get_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }

    /// Get optional dependencies for this tool.
    ///
    /// These wait for install but do not warn (e.g., cargo-binstall).
    fn get_optional_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }

    /// Get all dependencies (required + optional) transitively.
    fn get_all_dependencies(&self, optional: bool) -> Result<IndexSet<BackendArg>>;

    // ========== Dependency Resolution ==========

    /// Get the toolset containing all dependencies.
    async fn dependency_toolset(&self, config: &Arc<Config>) -> Result<Toolset>;

    /// Find a binary in the dependency toolset.
    async fn dependency_which(&self, config: &Arc<Config>, bin: &str) -> Option<PathBuf>;

    /// Get environment variables from dependencies.
    async fn dependency_env(&self, config: &Arc<Config>) -> Result<BTreeMap<String, String>>;

    // ========== Dependency Warnings ==========

    /// Warn if any required dependencies are missing.
    async fn warn_if_dependencies_missing(&self, config: &Arc<Config>) -> Result<()>;

    /// Check if a required dependency is available and warn if not.
    ///
    /// Provides a consistent warning message format across all backends.
    async fn warn_if_dependency_missing(
        &self,
        config: &Arc<Config>,
        program: &str,
        install_instructions: &str,
    );
}
