//! Binary path provider trait for backends
//!
//! This trait handles binary paths and execution environment.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;

use crate::config::Config;
use crate::toolset::ResolveOptions;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};

use super::identity::BackendIdentity;

/// Trait for binary path discovery and execution environment.
///
/// This trait provides methods for:
/// - Listing binary paths for a tool version
/// - Computing execution environment
/// - Finding specific binaries (which)
/// - Computing PATH for commands
#[async_trait]
pub trait BinPathProvider: BackendIdentity {
    // ========== Binary Paths ==========

    /// List all binary paths for a tool version.
    ///
    /// Default returns `<install_path>/bin`. Backends can override
    /// for tools with different layouts.
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        match tv.request {
            ToolRequest::System { .. } => Ok(vec![]),
            _ => Ok(vec![tv.install_path().join("bin")]),
        }
    }

    // ========== Execution Environment ==========

    /// Get additional environment variables for this tool version.
    ///
    /// These are merged with the toolset environment when running commands.
    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    /// Build the PATH environment variable for running commands.
    ///
    /// Combines this tool's bin paths with dependency paths and system PATH.
    async fn path_env_for_cmd(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<OsString>;

    // ========== Binary Discovery ==========

    /// Find a specific binary in this tool's paths.
    ///
    /// Returns the full path to the binary if found.
    async fn which(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> Result<Option<PathBuf>>;

    // ========== Outdated Info ==========

    /// Get outdated information for a tool version.
    async fn outdated_info(
        &self,
        _config: &Arc<Config>,
        _tv: &ToolVersion,
        _bump: bool,
        _opts: &ResolveOptions,
    ) -> Result<Option<OutdatedInfo>> {
        Ok(None)
    }

    // ========== Metadata ==========

    /// Get a description of this tool.
    async fn description(&self) -> Option<String> {
        None
    }
}
