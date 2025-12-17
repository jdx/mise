//! Lockfile support trait for backends
//!
//! This trait handles platform-specific metadata for lockfile generation.

use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;
use eyre::Result;

use crate::config::Settings;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::platform::Platform;
use crate::toolset::ToolVersion;

use super::super::platform_target::PlatformTarget;
use super::super::{GitHubReleaseInfo, SecurityFeature};
use super::identity::BackendIdentity;

/// Trait for lockfile and platform-specific metadata support.
///
/// This trait provides methods for:
/// - Platform key generation for lockfile storage
/// - Resolving lockfile options
/// - Platform variant enumeration
/// - Checksum verification
/// - Security feature information
#[async_trait]
pub trait LockfileSupport: BackendIdentity {
    // ========== Platform Keys ==========

    /// Generate a platform key for lockfile storage.
    ///
    /// Default implementation uses os-arch format.
    /// Backends can override for more specific keys.
    fn get_platform_key(&self) -> String {
        let settings = Settings::get();
        let os = settings.os();
        let arch = settings.arch();
        format!("{os}-{arch}")
    }

    /// Resolve lockfile options for a tool request on a target platform.
    ///
    /// These options affect artifact identity and must match exactly
    /// for lockfile lookup.
    fn resolve_lockfile_options(
        &self,
        _request: &crate::toolset::ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        BTreeMap::new() // Default: no options affect artifact identity
    }

    /// Return all platform variants that should be locked for a given platform.
    ///
    /// Some tools have compile-time variants (e.g., bun has baseline/musl)
    /// that result in different download URLs and checksums.
    fn platform_variants(&self, platform: &Platform) -> Vec<Platform> {
        vec![platform.clone()] // Default: just the base platform
    }

    // ========== Lock Info Resolution ==========

    /// Resolve platform-specific lock information without installation.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo>;

    /// Provide tarball URL for platform-specific tool installation.
    ///
    /// Backends can implement this for simple tarball-based tools.
    async fn get_tarball_url(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<Option<String>> {
        Ok(None) // Default: no tarball URL available
    }

    /// Provide GitHub/GitLab release info for platform-specific tool installation.
    async fn get_github_release_info(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<Option<GitHubReleaseInfo>> {
        Ok(None) // Default: no GitHub release info available
    }

    /// Shared logic for processing tarball-based tools.
    async fn resolve_lock_info_from_tarball(
        &self,
        tarball_url: &str,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        Ok(PlatformInfo {
            checksum: None,
            size: None,
            url: Some(tarball_url.to_string()),
            url_api: None,
        })
    }

    /// Shared logic for processing GitHub/GitLab release-based tools.
    async fn resolve_lock_info_from_github_release(
        &self,
        release_info: &GitHubReleaseInfo,
        _tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let asset_name = release_info.asset_pattern.as_ref().map(|pattern| {
            pattern
                .replace("{os}", target.os_name())
                .replace("{arch}", target.arch_name())
        });

        let asset_url = match (&release_info.api_url, &asset_name) {
            (Some(base_url), Some(name)) => Some(format!("{}/{}", base_url, name)),
            _ => asset_name.clone(),
        };

        Ok(PlatformInfo {
            checksum: None,
            size: None,
            url: asset_url,
            url_api: None,
        })
    }

    /// Fallback method when no specific metadata resolution is available.
    async fn resolve_lock_info_fallback(
        &self,
        _tv: &ToolVersion,
        _target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        Ok(PlatformInfo {
            checksum: None,
            size: None,
            url: None,
            url_api: None,
        })
    }

    // ========== Checksum Verification ==========

    /// Verify checksum of a downloaded file.
    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> Result<()>;

    // ========== Security ==========

    /// Get security features supported by this backend.
    async fn security_info(&self) -> Vec<SecurityFeature> {
        vec![]
    }
}
