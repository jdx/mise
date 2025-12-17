//! Installer trait for backends
//!
//! This trait handles tool installation and uninstallation.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;

use crate::config::Config;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::ui::progress_report::SingleReport;

use super::identity::BackendIdentity;

/// Trait for tool installation and uninstallation.
///
/// This trait provides methods for:
/// - Installing tool versions
/// - Uninstalling tool versions
/// - Managing installation directories
/// - Checking installation status
#[async_trait]
pub trait Installer: BackendIdentity {
    // ========== Installation ==========

    /// Install a tool version.
    ///
    /// This is the main entry point for installation. It handles:
    /// - Dry-run mode
    /// - Force reinstallation
    /// - Locked mode verification
    /// - Directory setup and cleanup
    /// - Post-install hooks
    ///
    /// Backends should implement `install_version_` for the actual installation logic.
    async fn install_version(&self, ctx: InstallContext, tv: ToolVersion) -> Result<ToolVersion>;

    /// Backend implementation for installing a tool version.
    ///
    /// This is the method backends must implement with their
    /// actual installation logic (download, extract, compile, etc.)
    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion>;

    // ========== Uninstallation ==========

    /// Uninstall a tool version.
    ///
    /// Handles removing the installation directory, cache, and downloads.
    async fn uninstall_version(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        dryrun: bool,
    ) -> Result<()>;

    /// Backend-specific uninstallation logic.
    ///
    /// Override this for backends that need custom cleanup.
    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &dyn SingleReport,
        _tv: &ToolVersion,
    ) -> Result<()> {
        Ok(())
    }

    /// Purge all versions of this tool.
    fn purge(&self, pr: &dyn SingleReport) -> Result<()>;

    // ========== Directory Management ==========

    /// Create installation directories for a tool version.
    fn create_install_dirs(&self, tv: &ToolVersion) -> Result<()>;

    /// Clean up directories on installation error.
    fn cleanup_install_dirs_on_error(&self, tv: &ToolVersion);

    /// Clean up temporary directories after successful installation.
    fn cleanup_install_dirs(&self, tv: &ToolVersion);

    /// Get the path to the incomplete marker file.
    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf;

    // ========== Status Checking ==========

    /// Check if a version is installed.
    fn is_version_installed(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> bool;

    /// Check if a version is outdated.
    async fn is_version_outdated(&self, config: &Arc<Config>, tv: &ToolVersion) -> bool;

    // ========== Symlink Management ==========

    /// Get the symlink path if this version is a symlink.
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf>;

    /// Create a symlink for a version.
    fn create_symlink(
        &self,
        version: &str,
        target: &std::path::Path,
    ) -> Result<Option<(PathBuf, PathBuf)>>;
}
