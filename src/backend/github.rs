use crate::backend::asset_detector;
use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::lookup_platform_key;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, template_string, try_with_v_prefix, verify_artifact,
};
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::toolset::ToolVersionOptions;
use crate::{backend::Backend, github, gitlab};
use async_trait::async_trait;
use eyre::Result;
use regex::Regex;
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug)]
pub struct UnifiedGitBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for UnifiedGitBackend {
    fn get_type(&self) -> BackendType {
        if self.is_gitlab() {
            BackendType::Gitlab
        } else {
            BackendType::Github
        }
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let repo = self.ba.tool_name();
        let opts = self.ba.opts();
        let api_url = self.get_api_url(&opts);
        if self.is_gitlab() {
            let releases = gitlab::list_releases_from_url(api_url.as_str(), &repo).await?;
            Ok(releases
                .into_iter()
                .filter(|r| {
                    opts.get("version_prefix")
                        .is_none_or(|p| r.tag_name.starts_with(p))
                })
                .map(|r| self.strip_version_prefix(&r.tag_name))
                .rev()
                .collect())
        } else {
            let releases = github::list_releases_from_url(api_url.as_str(), &repo).await?;
            Ok(releases
                .into_iter()
                .filter(|r| {
                    opts.get("version_prefix")
                        .is_none_or(|p| r.tag_name.starts_with(p))
                })
                .map(|r| self.strip_version_prefix(&r.tag_name))
                .rev()
                .collect())
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = self.get_api_url(&opts);

        // Check if URL already exists in lockfile platforms first
        let platform_key = self.get_platform_key();
        let asset_url = if let Some(existing_platform) = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|asset| asset.url.clone())
        {
            debug!(
                "Using existing URL from lockfile for platform {}: {}",
                platform_key, existing_platform
            );
            existing_platform
        } else {
            // Find the asset URL for this specific version
            self.resolve_asset_url(&tv, &opts, &repo, &api_url).await?
        };

        // Download and install
        self.download_and_install(ctx, &mut tv, &asset_url, &opts)
            .await?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path_template) = opts.get("bin_path") {
            let bin_path = template_string(bin_path_template, tv);
            Ok(vec![tv.install_path().join(bin_path)])
        } else {
            self.discover_bin_paths(tv)
        }
    }
}

impl UnifiedGitBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn is_gitlab(&self) -> bool {
        self.ba.backend_type() == BackendType::Gitlab
    }

    fn repo(&self) -> String {
        // Use tool_name() method to properly resolve aliases
        // This ensures that when an alias like "test-edit = github:microsoft/edit" is used,
        // the repository name is correctly extracted as "microsoft/edit"
        self.ba.tool_name()
    }

    // Helper to format asset names for error messages
    fn format_asset_list<'a, I>(assets: I) -> String
    where
        I: Iterator<Item = &'a String>,
    {
        assets.cloned().collect::<Vec<_>>().join(", ")
    }

    fn get_api_url(&self, opts: &ToolVersionOptions) -> String {
        opts.get("api_url")
            .map(|s| s.as_str())
            .unwrap_or(if self.is_gitlab() {
                "https://gitlab.com/api/v4"
            } else {
                "https://api.github.com"
            })
            .to_string()
    }

    /// Downloads and installs the asset
    async fn download_and_install(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        asset_url: &str,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        let filename = get_filename_from_url(asset_url);
        let file_path = tv.download_path().join(&filename);
        let headers = if self.is_gitlab() {
            gitlab::get_headers(asset_url)
        } else {
            github::get_headers(asset_url)
        };

        // Store the asset URL in the tool version
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(asset_url.to_string());

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file_with_headers(asset_url, &file_path, &headers, Some(ctx.pr.as_ref()))
            .await?;

        // Verify and install
        verify_artifact(tv, &file_path, opts, Some(ctx.pr.as_ref()))?;
        install_artifact(tv, &file_path, opts, Some(ctx.pr.as_ref()))?;
        self.verify_checksum(ctx, tv, &file_path)?;

        Ok(())
    }

    /// Discovers bin paths in the installation directory
    fn discover_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<std::path::PathBuf>> {
        let bin_path = tv.install_path().join("bin");
        if bin_path.exists() {
            return Ok(vec![bin_path]);
        }

        // Look for bin directory in subdirectories (for extracted archives)
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
            for entry in entries.flatten() {
                let sub_bin_path = entry.path().join("bin");
                if sub_bin_path.exists() {
                    paths.push(sub_bin_path);
                }
            }
        }

        if paths.is_empty() {
            Ok(vec![tv.install_path()])
        } else {
            Ok(paths)
        }
    }

    /// Resolves the asset URL using either explicit patterns or auto-detection
    async fn resolve_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
    ) -> Result<String> {
        // Check for direct platform-specific URLs first
        if let Some(direct_url) = lookup_platform_key(opts, "url") {
            return Ok(direct_url);
        }

        let version = &tv.version;
        let version_prefix = opts.get("version_prefix").map(|s| s.as_str());
        if self.is_gitlab() {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_gitlab_asset_url(tv, opts, repo, api_url, &candidate)
                    .await
            })
            .await
        } else {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_github_asset_url(tv, opts, repo, api_url, &candidate)
                    .await
            })
            .await
        }
    }

    async fn resolve_github_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
    ) -> Result<String> {
        let release = github::get_release_for_url(api_url, repo, version).await?;

        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();

        // Try explicit pattern first, then fall back to auto-detection
        if let Some(pattern) = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern with actual values
            let templated_pattern = template_string(&pattern, tv);

            // Find matching asset using pattern
            let asset = release
                .assets
                .into_iter()
                .find(|a| self.matches_pattern(&a.name, &templated_pattern))
                .ok_or_else(|| {
                    eyre::eyre!(
                        "No matching asset found for pattern: {}\nAvailable assets: {}",
                        templated_pattern,
                        Self::format_asset_list(available_assets.iter())
                    )
                })?;

            return Ok(asset.browser_download_url);
        }

        // Fall back to auto-detection
        let asset_name = self.auto_detect_asset(&available_assets)?;
        let asset = self
            .find_asset_case_insensitive(&release.assets, &asset_name, |a| &a.name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
                    Self::format_asset_list(available_assets.iter())
                )
            })?;

        Ok(asset.browser_download_url.clone())
    }

    async fn resolve_gitlab_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
    ) -> Result<String> {
        let release = gitlab::get_release_for_url(api_url, repo, version).await?;

        let available_assets: Vec<String> = release
            .assets
            .links
            .iter()
            .map(|a| a.name.clone())
            .collect();

        // Try explicit pattern first, then fall back to auto-detection
        if let Some(pattern) = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern with actual values
            let templated_pattern = template_string(&pattern, tv);

            // Find matching asset using pattern
            let asset = release
                .assets
                .links
                .into_iter()
                .find(|a| self.matches_pattern(&a.name, &templated_pattern))
                .ok_or_else(|| {
                    eyre::eyre!(
                        "No matching asset found for pattern: {}\nAvailable assets: {}",
                        templated_pattern,
                        Self::format_asset_list(available_assets.iter())
                    )
                })?;

            return Ok(asset.direct_asset_url);
        }

        // Fall back to auto-detection
        let asset_name = self.auto_detect_asset(&available_assets)?;
        let asset = self
            .find_asset_case_insensitive(&release.assets.links, &asset_name, |a| &a.name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
                    Self::format_asset_list(available_assets.iter())
                )
            })?;

        Ok(asset.direct_asset_url.clone())
    }

    fn auto_detect_asset(&self, available_assets: &[String]) -> Result<String> {
        let settings = Settings::get();
        let picker = asset_detector::AssetPicker::new(
            settings.os().to_string(),
            settings.arch().to_string(),
        );

        picker.pick_best_asset(available_assets).ok_or_else(|| {
            eyre::eyre!(
                "No suitable asset found for current platform ({}-{})\nAvailable assets: {}",
                settings.os(),
                settings.arch(),
                available_assets.join(", ")
            )
        })
    }

    fn find_asset_case_insensitive<'a, T>(
        &self,
        assets: &'a [T],
        target_name: &str,
        get_name: impl Fn(&T) -> &str,
    ) -> Option<&'a T> {
        // First try exact match, then case-insensitive
        assets
            .iter()
            .find(|a| get_name(a) == target_name)
            .or_else(|| {
                let target_lower = target_name.to_lowercase();
                assets
                    .iter()
                    .find(|a| get_name(a).to_lowercase() == target_lower)
            })
    }

    fn matches_pattern(&self, asset_name: &str, pattern: &str) -> bool {
        // Simple pattern matching - convert glob-like pattern to regex
        let regex_pattern = pattern
            .replace(".", "\\.")
            .replace("*", ".*")
            .replace("?", ".");

        if let Ok(re) = Regex::new(&format!("^{regex_pattern}$")) {
            re.is_match(asset_name)
        } else {
            // Fallback to simple contains check
            asset_name.contains(pattern)
        }
    }

    fn strip_version_prefix(&self, tag_name: &str) -> String {
        let opts = self.ba.opts();

        // If a custom version_prefix is configured, strip it first
        if let Some(prefix) = opts.get("version_prefix") {
            if let Some(stripped) = tag_name.strip_prefix(prefix) {
                return stripped.to_string();
            }
        }

        // Fall back to stripping 'v' prefix
        if tag_name.starts_with('v') {
            tag_name.trim_start_matches('v').to_string()
        } else {
            tag_name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;

    fn create_test_backend() -> UnifiedGitBackend {
        UnifiedGitBackend::from_arg(BackendArg::new(
            "github".to_string(),
            Some("github:test/repo".to_string()),
        ))
    }

    #[test]
    fn test_pattern_matching() {
        let backend = create_test_backend();
        assert!(backend.matches_pattern("test-v1.0.0.zip", "test-*"));
        assert!(!backend.matches_pattern("other-v1.0.0.zip", "test-*"));
    }

    #[test]
    fn test_version_prefix_functionality() {
        let mut backend = create_test_backend();

        // Test with no version prefix configured
        assert_eq!(backend.strip_version_prefix("v1.0.0"), "1.0.0");
        assert_eq!(backend.strip_version_prefix("1.0.0"), "1.0.0");

        // Test with custom version prefix
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("version_prefix".to_string(), "release-".to_string());
        backend.ba = Arc::new(BackendArg::new_raw(
            "test".to_string(),
            Some("github:test/repo".to_string()),
            "test".to_string(),
            Some(opts),
        ));

        assert_eq!(backend.strip_version_prefix("release-1.0.0"), "1.0.0");
        assert_eq!(backend.strip_version_prefix("1.0.0"), "1.0.0");
    }

    #[test]
    fn test_find_asset_case_insensitive() {
        let backend = create_test_backend();

        // Mock asset structs for testing
        struct TestAsset {
            name: String,
        }

        let assets = vec![
            TestAsset {
                name: "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            },
            TestAsset {
                name: "tool-1.0.0-Darwin-x86_64.tar.gz".to_string(),
            },
            TestAsset {
                name: "tool-1.0.0-Windows-x86_64.zip".to_string(),
            },
        ];

        // Test exact match (should find immediately)
        let result =
            backend.find_asset_case_insensitive(&assets, "tool-1.0.0-linux-x86_64.tar.gz", |a| {
                &a.name
            });
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-linux-x86_64.tar.gz");

        // Test case-insensitive match for Darwin (capital D)
        let result = backend.find_asset_case_insensitive(
            &assets,
            "tool-1.0.0-darwin-x86_64.tar.gz", // lowercase 'd'
            |a| &a.name,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-Darwin-x86_64.tar.gz");

        // Test case-insensitive match for Windows (capital W)
        let result = backend.find_asset_case_insensitive(
            &assets,
            "tool-1.0.0-windows-x86_64.zip", // lowercase 'w'
            |a| &a.name,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-Windows-x86_64.zip");

        // Test no match
        let result =
            backend.find_asset_case_insensitive(&assets, "nonexistent-asset.tar.gz", |a| &a.name);
        assert!(result.is_none());
    }
}
