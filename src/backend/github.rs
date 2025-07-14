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
use std::sync::LazyLock;

// Auto-detection patterns based on common conventions
static OS_PATTERNS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "linux",
            Regex::new(r"(?i)(?:\b|_)linux(?:\b|_|32|64)").unwrap(),
        ),
        (
            "macos",
            Regex::new(r"(?i)(?:\b|_)(?:darwin|mac(?:osx?)?|osx)(?:\b|_)").unwrap(),
        ),
        (
            "windows",
            Regex::new(r"(?i)(?:\b|_)win(?:32|64|dows)?(?:\b|_)").unwrap(),
        ),
    ]
});

static ARCH_PATTERNS: LazyLock<Vec<(&str, Regex)>> = LazyLock::new(|| {
    vec![
        (
            "x64",
            Regex::new(r"(?i)(?:\b|_)(?:x86[_-]64|x64|amd64)(?:\b|_)").unwrap(),
        ),
        (
            "arm64",
            Regex::new(r"(?i)(?:\b|_)(?:aarch_?64|arm_?64)(?:\b|_)").unwrap(),
        ),
        (
            "x86",
            Regex::new(r"(?i)(?:\b|_)(?:x86|i386|i686)(?:\b|_)").unwrap(),
        ),
        (
            "arm",
            Regex::new(r"(?i)(?:\b|_)arm(?:v[0-7])?(?:\b|_)").unwrap(),
        ),
    ]
});

// Common archive extensions
static ARCHIVE_EXTENSIONS: LazyLock<Vec<&str>> = LazyLock::new(|| {
    vec![
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".tzst", ".zip",
        ".7z", ".tar",
    ]
});

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

    /// Automatically picks the best asset based on platform conventions
    fn auto_pick_asset(&self, assets: &[String]) -> Option<String> {
        let settings = Settings::get();
        let target_os = settings.os();
        let target_arch = settings.arch();

        // Filter assets by archive type first
        let archive_assets: Vec<String> = assets
            .iter()
            .filter(|name| ARCHIVE_EXTENSIONS.iter().any(|ext| name.ends_with(ext)))
            .cloned()
            .collect();

        let candidates = if archive_assets.is_empty() {
            assets
        } else {
            &archive_assets
        };

        // Score each asset based on how well it matches the target platform
        let mut scored_assets: Vec<(i32, String)> = candidates
            .iter()
            .map(|asset| {
                (
                    self.score_asset(asset, target_os, target_arch),
                    asset.clone(),
                )
            })
            .collect();

        // Sort by score (higher is better)
        scored_assets.sort_by(|a, b| b.0.cmp(&a.0));

        // Return the best match if it has a positive score
        if let Some((score, asset)) = scored_assets.first() {
            if *score > 0 {
                Some(asset.clone())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Scores an asset based on how well it matches the target platform
    fn score_asset(&self, asset: &str, target_os: &str, target_arch: &str) -> i32 {
        let mut score = 0;

        // Check OS match
        for (os, pattern) in OS_PATTERNS.iter() {
            if pattern.is_match(asset) {
                if *os == target_os {
                    score += 100; // Exact OS match
                } else if (target_os == "darwin" || target_os == "macos") && *os == "macos" {
                    score += 100; // Handle macos/darwin aliases
                } else {
                    score -= 50; // Wrong OS
                }
            }
        }

        // Check architecture match
        for (arch, pattern) in ARCH_PATTERNS.iter() {
            if pattern.is_match(asset) {
                if *arch == target_arch {
                    score += 50; // Exact arch match
                } else if (target_arch == "amd64" || target_arch == "x86_64") && *arch == "x64" {
                    score += 50; // Handle x86_64/amd64/x64 aliases
                } else if (target_arch == "arm64" || target_arch == "aarch64") && *arch == "arm64" {
                    score += 50; // Handle aarch64/arm64 aliases
                } else {
                    score -= 25; // Wrong arch
                }
            }
        }

        // Prefer archive formats
        if ARCHIVE_EXTENSIONS.iter().any(|ext| asset.ends_with(ext)) {
            score += 10;
        }

        // Penalize debug/test builds
        if asset.contains("debug") || asset.contains("test") {
            score -= 20;
        }

        score
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
        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();

        // Try platform-specific pattern first, then fall back to general pattern
        if let Some(pattern) = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern with actual values
            let templated_pattern = template_string(&pattern, tv);

            // Find matching asset using explicit pattern
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
        } else {
            // Use auto-detection when no explicit pattern is provided
            if let Some(asset_name) = self.auto_pick_asset(&available_assets) {
                let asset = release
                    .assets
                    .into_iter()
                    .find(|a| a.name == asset_name)
                    .ok_or_else(|| eyre::eyre!("Auto-detected asset not found: {}", asset_name))?;

                Ok(asset.browser_download_url)
            } else {
                eyre::bail!(
                    "No suitable asset found for current platform ({}-{})\nAvailable assets: {}",
                    Settings::get().os(),
                    Settings::get().arch(),
                    Self::format_asset_list(available_assets.iter())
                )
            }
        }
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

        // Try platform-specific pattern first, then fall back to general pattern
        if let Some(pattern) = lookup_platform_key(opts, "asset_pattern")
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern with actual values
            let templated_pattern = template_string(&pattern, tv);

            // Find matching asset using explicit pattern
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
        } else {
            // Use auto-detection when no explicit pattern is provided
            if let Some(asset_name) = self.auto_pick_asset(&available_assets) {
                let asset = release
                    .assets
                    .links
                    .into_iter()
                    .find(|a| a.name == asset_name)
                    .ok_or_else(|| eyre::eyre!("Auto-detected asset not found: {}", asset_name))?;

                Ok(asset.direct_asset_url)
            } else {
                eyre::bail!(
                    "No suitable asset found for current platform ({}-{})\nAvailable assets: {}",
                    Settings::get().os(),
                    Settings::get().arch(),
                    Self::format_asset_list(available_assets.iter())
                )
            }
        }
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
    fn test_auto_pick_asset_macos() {
        let backend = create_test_backend();
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-x86_64.tar.gz".to_string(),
            "tool-1.0.0-windows-x86_64.zip".to_string(),
        ];

        // This test will pick the asset based on the actual platform
        if let Some(picked) = backend.auto_pick_asset(&assets) {
            assert!(picked.ends_with(".tar.gz") || picked.ends_with(".zip"));
        }
    }

    #[test]
    fn test_score_asset_logic() {
        let backend = create_test_backend();

        // Test scoring for linux x86_64
        let score1 = backend.score_asset("tool-1.0.0-linux-x86_64.tar.gz", "linux", "x86_64");
        let score2 = backend.score_asset("tool-1.0.0-windows-x86_64.zip", "linux", "x86_64");
        let score3 = backend.score_asset("tool-1.0.0-linux-arm64.tar.gz", "linux", "x86_64");

        // Should prefer exact OS and arch match
        assert!(
            score1 > score2,
            "Linux asset should score higher than Windows asset for Linux target"
        );
        assert!(
            score1 > score3,
            "x86_64 asset should score higher than arm64 asset for x86_64 target"
        );
    }

    #[test]
    fn test_auto_pick_prefers_archives() {
        let backend = create_test_backend();
        let assets = vec![
            "tool-1.0.0-linux-x86_64".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
        ];

        if let Some(picked) = backend.auto_pick_asset(&assets) {
            assert!(picked.ends_with(".tar.gz"), "Should prefer archive format");
        }
    }
}
