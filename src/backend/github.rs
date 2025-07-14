use crate::backend::backend_type::BackendType;
use crate::backend::static_helpers::lookup_platform_key;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, template_string, verify_artifact,
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
        if self.is_gitlab() {
            let releases = gitlab::list_releases(&repo).await?;
            Ok(releases
                .into_iter()
                .map(|r| r.tag_name.trim_start_matches('v').to_string())
                .collect())
        } else {
            let releases = github::list_releases(&repo).await?;
            Ok(releases
                .into_iter()
                .map(|r| r.tag_name.trim_start_matches('v').to_string())
                .collect())
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let feature_name = if self.is_gitlab() {
            "gitlab backend"
        } else {
            "github backend"
        };
        Settings::get().ensure_experimental(feature_name)?;
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = opts
            .get("api_url")
            .map(|s| s.as_str())
            .unwrap_or(if self.is_gitlab() {
                "https://gitlab.com/api/v4"
            } else {
                "https://api.github.com"
            });

        // Find the asset URL for this specific version
        let asset_url = self.resolve_asset_url(&tv, &opts, &repo, api_url).await?;

        // Download
        let filename = get_filename_from_url(&asset_url);
        let file_path = tv.download_path().join(&filename);

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(&asset_url, &file_path, Some(&ctx.pr))
            .await?;

        // Verify (shared)
        verify_artifact(&tv, &file_path, &opts)?;

        // Install (shared)
        install_artifact(&tv, &file_path, &opts)?;

        // Verify checksum if specified
        self.verify_checksum(ctx, &mut tv, &file_path)?;

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
            // Look for bin directory in the install path
            let bin_path = tv.install_path().join("bin");
            if bin_path.exists() {
                Ok(vec![bin_path])
            } else {
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
                if !paths.is_empty() {
                    Ok(paths)
                } else {
                    Ok(vec![tv.install_path()])
                }
            }
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

    async fn resolve_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
    ) -> Result<String> {
        let version = if tv.version.starts_with('v') {
            tv.version.clone()
        } else {
            format!("v{}", tv.version)
        };

        // Check for direct platform-specific URLs first using the helper
        if let Some(direct_url) = lookup_platform_key(opts, "url") {
            return Ok(direct_url);
        }

        if self.is_gitlab() {
            self.resolve_gitlab_asset_url(tv, opts, repo, api_url, &version)
                .await
        } else {
            self.resolve_github_asset_url(tv, opts, repo, api_url, &version)
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

        // Get platform-specific pattern first, then fall back to general pattern
        let pattern = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
            .unwrap_or("{name}-{version}-{os}-{arch}.{ext}".to_string());

        // Template the pattern with actual values
        let templated_pattern = template_string(&pattern, tv);

        // Find matching asset - pattern is already templated by mise.toml parsing
        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
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

        Ok(asset.browser_download_url)
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

        // Get platform-specific pattern first, then fall back to general pattern
        let pattern = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
            .unwrap_or("{name}-{version}-{os}-{arch}.{ext}".to_string());

        // Template the pattern with actual values
        let templated_pattern = template_string(&pattern, tv);

        let available_assets: Vec<String> = release.assets.links.iter().map(|a| a.name.clone()).collect();
        // Find matching asset - pattern is already templated by mise.toml parsing
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

        Ok(asset.direct_asset_url)
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
}
