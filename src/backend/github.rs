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

/// Asset auto-detection module for GitHub/GitLab releases
mod asset_detector {
    use regex::Regex;
    use std::sync::LazyLock;

    /// Platform detection patterns
    pub struct PlatformPatterns {
        pub os_patterns: &'static [(AssetOs, Regex)],
        pub arch_patterns: &'static [(AssetArch, Regex)],
        pub archive_extensions: &'static [&'static str],
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum AssetOs {
        Linux,
        Macos,
        Windows,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum AssetArch {
        X64,
        Arm64,
        X86,
        Arm,
    }

    impl AssetOs {
        pub fn matches_target(&self, target: &str) -> bool {
            match self {
                AssetOs::Linux => target == "linux",
                AssetOs::Macos => target == "macos" || target == "darwin",
                AssetOs::Windows => target == "windows",
            }
        }
    }

    impl AssetArch {
        pub fn matches_target(&self, target: &str) -> bool {
            match self {
                AssetArch::X64 => target == "x86_64" || target == "amd64" || target == "x64",
                AssetArch::Arm64 => target == "aarch64" || target == "arm64",
                AssetArch::X86 => target == "x86" || target == "i386" || target == "i686",
                AssetArch::Arm => target == "arm",
            }
        }
    }

    static OS_PATTERNS: LazyLock<Vec<(AssetOs, Regex)>> = LazyLock::new(|| {
        vec![
            (
                AssetOs::Linux,
                Regex::new(r"(?i)(?:\b|_)linux(?:\b|_|32|64)").unwrap(),
            ),
            (
                AssetOs::Macos,
                Regex::new(r"(?i)(?:\b|_)(?:darwin|mac(?:osx?)?|osx)(?:\b|_)").unwrap(),
            ),
            (
                AssetOs::Windows,
                Regex::new(r"(?i)(?:\b|_)win(?:32|64|dows)?(?:\b|_)").unwrap(),
            ),
        ]
    });

    static ARCH_PATTERNS: LazyLock<Vec<(AssetArch, Regex)>> = LazyLock::new(|| {
        vec![
            (
                AssetArch::X64,
                Regex::new(r"(?i)(?:\b|_)(?:x86[_-]64|x64|amd64)(?:\b|_)").unwrap(),
            ),
            (
                AssetArch::Arm64,
                Regex::new(r"(?i)(?:\b|_)(?:aarch_?64|arm_?64)(?:\b|_)").unwrap(),
            ),
            (
                AssetArch::X86,
                Regex::new(r"(?i)(?:\b|_)(?:x86|i386|i686)(?:\b|_)").unwrap(),
            ),
            (
                AssetArch::Arm,
                Regex::new(r"(?i)(?:\b|_)arm(?:v[0-7])?(?:\b|_)").unwrap(),
            ),
        ]
    });

    static ARCHIVE_EXTENSIONS: &[&str] = &[
        ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".tzst", ".zip",
        ".7z", ".tar",
    ];

    pub static PLATFORM_PATTERNS: LazyLock<PlatformPatterns> = LazyLock::new(|| PlatformPatterns {
        os_patterns: &OS_PATTERNS,
        arch_patterns: &ARCH_PATTERNS,
        archive_extensions: ARCHIVE_EXTENSIONS,
    });

    /// Automatically detects the best asset for the current platform
    pub struct AssetPicker {
        target_os: String,
        target_arch: String,
    }

    impl AssetPicker {
        pub fn new(target_os: String, target_arch: String) -> Self {
            Self {
                target_os,
                target_arch,
            }
        }

        /// Picks the best asset from available options
        pub fn pick_best_asset(&self, assets: &[String]) -> Option<String> {
            let candidates = self.filter_archive_assets(assets);
            let mut scored_assets = self.score_all_assets(&candidates);

            // Sort by score (higher is better)
            scored_assets.sort_by(|a, b| b.0.cmp(&a.0));

            // Return the best match if it has a positive score
            scored_assets
                .first()
                .filter(|(score, _)| *score > 0)
                .map(|(_, asset)| asset.clone())
        }

        /// Filters assets to prefer archive formats
        fn filter_archive_assets(&self, assets: &[String]) -> Vec<String> {
            let archive_assets: Vec<String> = assets
                .iter()
                .filter(|name| {
                    PLATFORM_PATTERNS
                        .archive_extensions
                        .iter()
                        .any(|ext| name.ends_with(ext))
                })
                .cloned()
                .collect();

            if archive_assets.is_empty() {
                assets.to_vec()
            } else {
                archive_assets
            }
        }

        /// Scores all assets based on platform compatibility
        fn score_all_assets(&self, assets: &[String]) -> Vec<(i32, String)> {
            assets
                .iter()
                .map(|asset| (self.score_asset(asset), asset.clone()))
                .collect()
        }

        /// Scores a single asset based on platform compatibility
        pub fn score_asset(&self, asset: &str) -> i32 {
            let mut score = 0;

            // OS scoring
            score += self.score_os_match(asset);

            // Architecture scoring
            score += self.score_arch_match(asset);

            // Format preferences
            score += self.score_format_preferences(asset);

            // Penalties for unwanted builds
            score += self.score_build_penalties(asset);

            score
        }

        fn score_os_match(&self, asset: &str) -> i32 {
            for (os, pattern) in PLATFORM_PATTERNS.os_patterns.iter() {
                if pattern.is_match(asset) {
                    return if os.matches_target(&self.target_os) {
                        100 // Exact OS match
                    } else {
                        -50 // Wrong OS
                    };
                }
            }
            0 // No OS detected
        }

        fn score_arch_match(&self, asset: &str) -> i32 {
            for (arch, pattern) in PLATFORM_PATTERNS.arch_patterns.iter() {
                if pattern.is_match(asset) {
                    return if arch.matches_target(&self.target_arch) {
                        50 // Exact arch match
                    } else {
                        -25 // Wrong arch
                    };
                }
            }
            0 // No arch detected
        }

        fn score_format_preferences(&self, asset: &str) -> i32 {
            if PLATFORM_PATTERNS
                .archive_extensions
                .iter()
                .any(|ext| asset.ends_with(ext))
            {
                10 // Prefer archive formats
            } else {
                0
            }
        }

        fn score_build_penalties(&self, asset: &str) -> i32 {
            let mut penalty = 0;
            if asset.contains("debug") || asset.contains("test") {
                penalty -= 20;
            }
            penalty
        }
    }
}

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
        let api_url = self.get_api_url(&opts);

        // Find the asset URL for this specific version
        let asset_url = self.resolve_asset_url(&tv, &opts, &repo, &api_url).await?;

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

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(asset_url, &file_path, Some(&ctx.pr))
            .await?;

        // Verify and install
        verify_artifact(tv, &file_path, opts)?;
        install_artifact(tv, &file_path, opts)?;
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
        let version = self.normalize_version(&tv.version);

        // Check for direct platform-specific URLs first
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

    fn normalize_version(&self, version: &str) -> String {
        if version.starts_with('v') {
            version.to_string()
        } else {
            format!("v{version}")
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
        let asset = release
            .assets
            .into_iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
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
        let asset = release
            .assets
            .links
            .into_iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
                    Self::format_asset_list(available_assets.iter())
                )
            })?;

        Ok(asset.direct_asset_url)
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
    fn test_asset_picker_functionality() {
        let picker = asset_detector::AssetPicker::new("linux".to_string(), "x86_64".to_string());
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-x86_64.tar.gz".to_string(),
            "tool-1.0.0-windows-x86_64.zip".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_asset_scoring() {
        let picker = asset_detector::AssetPicker::new("linux".to_string(), "x86_64".to_string());

        let score_linux = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz");
        let score_windows = picker.score_asset("tool-1.0.0-windows-x86_64.zip");
        let score_linux_arm = picker.score_asset("tool-1.0.0-linux-arm64.tar.gz");

        assert!(
            score_linux > score_windows,
            "Linux should score higher than Windows"
        );
        assert!(
            score_linux > score_linux_arm,
            "x86_64 should score higher than arm64"
        );
    }

    #[test]
    fn test_archive_preference() {
        let picker = asset_detector::AssetPicker::new("linux".to_string(), "x86_64".to_string());
        let assets = vec![
            "tool-1.0.0-linux-x86_64".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_pattern_matching() {
        let backend = create_test_backend();

        assert!(backend.matches_pattern("test-1.0.0-linux.tar.gz", "test-*-linux.tar.gz"));
        assert!(backend.matches_pattern("test-1.0.0-linux.tar.gz", "test-?.?.?-linux.tar.gz"));
        assert!(!backend.matches_pattern("test-1.0.0-windows.zip", "test-*-linux.tar.gz"));
    }

    #[test]
    fn test_version_normalization() {
        let backend = create_test_backend();

        assert_eq!(backend.normalize_version("1.0.0"), "v1.0.0");
        assert_eq!(backend.normalize_version("v1.0.0"), "v1.0.0");
    }
}
