//! Version provider trait for backends
//!
//! This trait handles version discovery, listing, and querying.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::Result;
use jiff::Timestamp;

use crate::config::Config;
use crate::toolset::install_state;

use super::super::VersionInfo;
use super::identity::BackendIdentity;

/// Trait for version discovery and listing.
///
/// This trait provides methods for:
/// - Listing remote versions available for installation
/// - Listing locally installed versions
/// - Querying for specific versions (latest, matching patterns)
/// - Parsing idiomatic version files
#[async_trait]
pub trait VersionProvider: BackendIdentity {
    // ========== Remote Version Listing ==========

    /// List all remote versions available for installation.
    ///
    /// This is the simple version that returns just version strings.
    async fn list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<String>> {
        Ok(self
            .list_remote_versions_with_info(config)
            .await?
            .into_iter()
            .map(|v| v.version)
            .collect())
    }

    /// List remote versions with additional metadata.
    ///
    /// Returns version info including optional created_at timestamps
    /// for date-based filtering. Backends should implement
    /// `_list_remote_versions_with_info` or `_list_remote_versions`.
    async fn list_remote_versions_with_info(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<VersionInfo>>;

    /// Backend implementation for fetching remote versions with metadata.
    ///
    /// Default wraps `_list_remote_versions` with no timestamps.
    /// Override this to provide timestamp information.
    async fn _list_remote_versions_with_info(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<VersionInfo>> {
        Ok(self
            ._list_remote_versions(config)
            .await?
            .into_iter()
            .map(|v| VersionInfo {
                version: v,
                ..Default::default()
            })
            .collect())
    }

    /// Backend implementation for fetching remote versions (without metadata).
    ///
    /// Override this OR `_list_remote_versions_with_info` (not both needed).
    /// WARNING: Implementing neither will cause infinite recursion.
    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<String>> {
        Ok(self
            ._list_remote_versions_with_info(config)
            .await?
            .into_iter()
            .map(|v| v.version)
            .collect())
    }

    // ========== Installed Version Listing ==========

    /// List all locally installed versions.
    fn list_installed_versions(&self) -> Vec<String> {
        install_state::list_versions(&self.ba().short)
    }

    /// List installed versions matching a query pattern.
    fn list_installed_versions_matching(&self, query: &str) -> Vec<String> {
        let versions = self.list_installed_versions();
        self.fuzzy_match_filter(versions, query)
    }

    // ========== Version Querying ==========

    /// Get the latest stable version.
    async fn latest_stable_version(&self, config: &Arc<Config>) -> Result<Option<String>> {
        self.latest_version(config, Some("latest".into())).await
    }

    /// Get the latest version matching an optional query.
    async fn latest_version(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
    ) -> Result<Option<String>>;

    /// Get the latest version with date filtering support.
    async fn latest_version_with_opts(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
        before_date: Option<Timestamp>,
    ) -> Result<Option<String>>;

    /// List remote versions matching a query pattern.
    async fn list_versions_matching(
        &self,
        config: &Arc<Config>,
        query: &str,
    ) -> Result<Vec<String>> {
        let versions = self.list_remote_versions(config).await?;
        Ok(self.fuzzy_match_filter(versions, query))
    }

    /// List versions matching a query, optionally filtered by release date.
    async fn list_versions_matching_with_opts(
        &self,
        config: &Arc<Config>,
        query: &str,
        before_date: Option<Timestamp>,
    ) -> Result<Vec<String>>;

    /// Get the latest installed version matching an optional query.
    fn latest_installed_version(&self, query: Option<String>) -> Result<Option<String>>;

    // ========== Idiomatic Version Files ==========

    /// List idiomatic version filenames for this backend.
    ///
    /// These are files like `.node-version`, `.python-version`, etc.
    async fn idiomatic_filenames(&self) -> Result<Vec<String>>;

    /// Parse an idiomatic version file and return the version string.
    async fn parse_idiomatic_file(&self, path: &Path) -> Result<String>;

    // ========== Aliases ==========

    /// Get version aliases (e.g., "lts" -> "20.10.0").
    fn get_aliases(&self) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    // ========== Utility ==========

    /// Filter versions using fuzzy matching.
    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String>;
}
