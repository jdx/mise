//! Unified asset matching for backend tool installation
//!
//! This module provides a high-level `AssetMatcher` that combines multiple
//! strategies for finding the best asset for installation:
//!
//! - **Auto-detection**: Uses platform heuristics to score and rank assets
//! - **Pattern matching**: Supports explicit patterns with OS/arch placeholders
//! - **Filtering**: Custom predicates for fine-grained asset selection
//!
//! # Example
//!
//! ```ignore
//! use crate::backend::asset_matcher::AssetMatcher;
//!
//! // Auto-detect best asset for current platform
//! let asset = AssetMatcher::new()
//!     .for_current_platform()
//!     .prefer_archive(true)
//!     .pick_from(&assets)?;
//!
//! // Match using explicit pattern
//! let asset = AssetMatcher::new()
//!     .with_pattern("tool-{version}-{os}-{arch}.tar.gz")
//!     .with_version("1.0.0")
//!     .pick_from(&assets)?;
//!
//! // Find checksum file for an asset
//! let checksum = AssetMatcher::new()
//!     .for_checksum_of("tool-1.0.0-linux-x64.tar.gz")
//!     .pick_from(&assets);
//! ```

use eyre::{Result, bail};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use super::asset_detector::AssetPicker;
use super::platform_target::PlatformTarget;
use crate::config::Settings;

/// Common checksum file extensions
static CHECKSUM_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        ".sha256",
        ".sha256sum",
        ".sha256sums",
        ".SHA256",
        ".SHA256SUM",
        ".SHA256SUMS",
        ".sha512",
        ".sha512sum",
        ".sha512sums",
        ".SHA512",
        ".SHA512SUM",
        ".SHA512SUMS",
        ".md5",
        ".md5sum",
        ".checksums",
        ".CHECKSUMS",
    ]
});

/// Common checksum filename patterns
static CHECKSUM_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)^sha256sums?\.txt$").unwrap(),
        Regex::new(r"(?i)^sha512sums?\.txt$").unwrap(),
        Regex::new(r"(?i)^checksums?\.txt$").unwrap(),
        Regex::new(r"(?i)^SHASUMS").unwrap(),
        Regex::new(r"(?i)checksums?\.sha256$").unwrap(),
    ]
});

/// Common signature file extensions
static SIGNATURE_EXTENSIONS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| vec![".sig", ".asc", ".gpg", ".minisig"]);

/// Represents a matched asset with metadata
#[derive(Debug, Clone)]
pub struct MatchedAsset {
    /// The asset name/filename
    pub name: String,
    /// Optional URL if available
    pub url: Option<String>,
    /// Match score (higher is better)
    pub score: i32,
    /// How the asset was matched
    pub match_type: MatchType,
}

/// How an asset was matched
#[derive(Debug, Clone, PartialEq)]
pub enum MatchType {
    /// Matched via auto-detection scoring
    AutoDetected,
    /// Matched via explicit pattern
    Pattern,
    /// Matched via exact name
    Exact,
    /// Matched as related asset (checksum, signature)
    Related,
}

/// Builder for matching assets
pub struct AssetMatcher {
    /// Target OS (e.g., "linux", "macos", "windows")
    target_os: Option<String>,
    /// Target architecture (e.g., "x86_64", "aarch64")
    target_arch: Option<String>,
    /// Target libc variant (e.g., "gnu", "musl")
    target_libc: Option<String>,
    /// Explicit patterns to match (with placeholders)
    patterns: Vec<String>,
    /// Version string for pattern substitution
    version: Option<String>,
    /// Whether to prefer archive formats
    prefer_archive: bool,
    /// Whether to allow non-archive binaries
    allow_binary: bool,
    /// Custom filter predicate
    filter: Option<AssetFilter>,
    /// Assets to exclude
    exclude: HashSet<String>,
    /// Minimum required score for auto-detection
    min_score: i32,
}

type AssetFilter = Box<dyn Fn(&str) -> bool + Send + Sync>;

impl std::fmt::Debug for AssetMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetMatcher")
            .field("target_os", &self.target_os)
            .field("target_arch", &self.target_arch)
            .field("target_libc", &self.target_libc)
            .field("patterns", &self.patterns)
            .field("version", &self.version)
            .field("prefer_archive", &self.prefer_archive)
            .field("allow_binary", &self.allow_binary)
            .field("filter", &self.filter.is_some())
            .field("exclude", &self.exclude)
            .field("min_score", &self.min_score)
            .finish()
    }
}

impl Default for AssetMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetMatcher {
    /// Create a new AssetMatcher with default settings
    pub fn new() -> Self {
        Self {
            target_os: None,
            target_arch: None,
            target_libc: None,
            patterns: Vec::new(),
            version: None,
            prefer_archive: true,
            allow_binary: true,
            filter: None,
            exclude: HashSet::new(),
            min_score: 0,
        }
    }

    /// Configure for the current platform using mise settings
    pub fn for_current_platform(mut self) -> Self {
        let settings = Settings::get();
        self.target_os = Some(settings.os().to_string());
        self.target_arch = Some(settings.arch().to_string());
        // Determine libc variant
        if settings.os() == "windows" {
            self.target_libc = Some("msvc".to_string());
        } else if cfg!(target_env = "musl") {
            self.target_libc = Some("musl".to_string());
        } else {
            self.target_libc = Some("gnu".to_string());
        }
        self
    }

    /// Configure for a specific target platform
    pub fn for_target(mut self, target: &PlatformTarget) -> Self {
        self.target_os = Some(target.os_name().to_string());
        self.target_arch = Some(target.arch_name().to_string());
        self
    }

    /// Set explicit OS target
    pub fn with_os(mut self, os: impl Into<String>) -> Self {
        self.target_os = Some(os.into());
        self
    }

    /// Set explicit architecture target
    pub fn with_arch(mut self, arch: impl Into<String>) -> Self {
        self.target_arch = Some(arch.into());
        self
    }

    /// Set explicit libc variant
    pub fn with_libc(mut self, libc: impl Into<String>) -> Self {
        self.target_libc = Some(libc.into());
        self
    }

    /// Add a pattern to match against
    ///
    /// Patterns support placeholders:
    /// - `{os}` - Operating system (linux, darwin, windows)
    /// - `{arch}` - Architecture (x86_64, aarch64, arm64, amd64)
    /// - `{version}` - Version string (requires `with_version`)
    /// - `{ext}` - Archive extension
    pub fn with_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.patterns.push(pattern.into());
        self
    }

    /// Add multiple patterns
    pub fn with_patterns(mut self, patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.patterns.extend(patterns.into_iter().map(|p| p.into()));
        self
    }

    /// Set version for pattern substitution
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set whether to prefer archive formats over binaries
    pub fn prefer_archive(mut self, prefer: bool) -> Self {
        self.prefer_archive = prefer;
        self
    }

    /// Set whether to allow matching standalone binaries
    pub fn allow_binary(mut self, allow: bool) -> Self {
        self.allow_binary = allow;
        self
    }

    /// Add a custom filter function
    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.filter = Some(Box::new(filter));
        self
    }

    /// Exclude specific asset names
    pub fn exclude(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude.extend(names.into_iter().map(|n| n.into()));
        self
    }

    /// Set minimum score for auto-detection matching
    pub fn min_score(mut self, score: i32) -> Self {
        self.min_score = score;
        self
    }

    /// Pick the best matching asset from a list of names
    pub fn pick_from(&self, assets: &[String]) -> Result<MatchedAsset> {
        let filtered = self.filter_assets(assets);

        if filtered.is_empty() {
            bail!("No assets available after filtering");
        }

        // Try pattern matching first if patterns are specified
        if !self.patterns.is_empty()
            && let Some(matched) = self.match_by_pattern(&filtered)
        {
            return Ok(matched);
        }

        // Fall back to auto-detection
        self.match_by_auto_detection(&filtered)
    }

    /// Pick the best matching asset, returning None if no match
    pub fn try_pick_from(&self, assets: &[String]) -> Option<MatchedAsset> {
        self.pick_from(assets).ok()
    }

    /// Find all matching assets, sorted by score (best first)
    pub fn find_all(&self, assets: &[String]) -> Vec<MatchedAsset> {
        let filtered = self.filter_assets(assets);
        let mut matches = Vec::new();

        // Add pattern matches
        for asset in &filtered {
            if self.matches_any_pattern(asset) {
                matches.push(MatchedAsset {
                    name: asset.clone(),
                    url: None,
                    score: 1000, // Pattern matches get high priority
                    match_type: MatchType::Pattern,
                });
            }
        }

        // Add auto-detected matches
        if let Some(picker) = self.create_picker() {
            for asset in &filtered {
                let score = picker.score_asset(asset);
                if score >= self.min_score {
                    // Don't add if already matched by pattern
                    if !matches.iter().any(|m| m.name == *asset) {
                        matches.push(MatchedAsset {
                            name: asset.clone(),
                            url: None,
                            score,
                            match_type: MatchType::AutoDetected,
                        });
                    }
                }
            }
        }

        // Sort by score descending
        matches.sort_by(|a, b| b.score.cmp(&a.score));
        matches
    }

    /// Find checksum file for a given asset
    pub fn find_checksum_for(&self, asset_name: &str, assets: &[String]) -> Option<String> {
        let base_name = asset_name
            .trim_end_matches(".tar.gz")
            .trim_end_matches(".tar.xz")
            .trim_end_matches(".tar.bz2")
            .trim_end_matches(".zip")
            .trim_end_matches(".tgz");

        // Try exact match with checksum extension
        for ext in CHECKSUM_EXTENSIONS.iter() {
            let checksum_name = format!("{asset_name}{ext}");
            if assets.iter().any(|a| a == &checksum_name) {
                return Some(checksum_name);
            }
            let checksum_name = format!("{base_name}{ext}");
            if assets.iter().any(|a| a == &checksum_name) {
                return Some(checksum_name);
            }
        }

        // Try common checksum file patterns
        for pattern in CHECKSUM_PATTERNS.iter() {
            for asset in assets {
                if pattern.is_match(asset) {
                    return Some(asset.clone());
                }
            }
        }

        None
    }

    /// Find signature file for a given asset
    pub fn find_signature_for(&self, asset_name: &str, assets: &[String]) -> Option<String> {
        for ext in SIGNATURE_EXTENSIONS.iter() {
            let sig_name = format!("{asset_name}{ext}");
            if assets.iter().any(|a| a == &sig_name) {
                return Some(sig_name);
            }
        }
        None
    }

    // ========== Internal Methods ==========

    fn filter_assets(&self, assets: &[String]) -> Vec<String> {
        assets
            .iter()
            .filter(|a| !self.exclude.contains(*a))
            .filter(|a| self.filter.as_ref().is_none_or(|f| f(a)))
            .cloned()
            .collect()
    }

    fn create_picker(&self) -> Option<AssetPicker> {
        let os = self.target_os.as_ref()?;
        let arch = self.target_arch.as_ref()?;
        Some(AssetPicker::new(os.clone(), arch.clone()))
    }

    fn match_by_pattern(&self, assets: &[String]) -> Option<MatchedAsset> {
        let expanded_patterns = self.expand_patterns();

        for pattern in &expanded_patterns {
            // Try exact match first
            if let Some(asset) = assets.iter().find(|a| *a == pattern) {
                return Some(MatchedAsset {
                    name: asset.clone(),
                    url: None,
                    score: 1000,
                    match_type: MatchType::Pattern,
                });
            }

            // Try regex match
            if let Ok(re) = Regex::new(&format!("^{}$", regex::escape(pattern)))
                && let Some(asset) = assets.iter().find(|a| re.is_match(a))
            {
                return Some(MatchedAsset {
                    name: asset.clone(),
                    url: None,
                    score: 900,
                    match_type: MatchType::Pattern,
                });
            }
        }

        None
    }

    fn matches_any_pattern(&self, asset: &str) -> bool {
        let expanded = self.expand_patterns();
        expanded.iter().any(|p| asset == p || asset.contains(p))
    }

    fn expand_patterns(&self) -> Vec<String> {
        let os_variants = self.os_variants();
        let arch_variants = self.arch_variants();
        let ext_variants = vec!["tar.gz", "tar.xz", "tar.bz2", "zip", "tgz"];

        let mut expanded = Vec::new();

        for pattern in &self.patterns {
            // Generate all combinations
            for os in &os_variants {
                for arch in &arch_variants {
                    for ext in &ext_variants {
                        let mut p = pattern.clone();
                        p = p.replace("{os}", os);
                        p = p.replace("{arch}", arch);
                        p = p.replace("{ext}", ext);
                        if let Some(ref version) = self.version {
                            p = p.replace("{version}", version);
                        }
                        expanded.push(p);
                    }
                }
            }

            // Also add pattern with just version substituted
            if let Some(ref version) = self.version {
                let p = pattern.replace("{version}", version);
                if !expanded.contains(&p) {
                    expanded.push(p);
                }
            }
        }

        expanded
    }

    fn os_variants(&self) -> Vec<&str> {
        match self.target_os.as_deref() {
            Some("linux") => vec!["linux", "Linux", "unknown-linux"],
            Some("macos") | Some("darwin") => {
                vec!["darwin", "Darwin", "macos", "macOS", "apple-darwin"]
            }
            Some("windows") => vec!["windows", "Windows", "win", "win64", "win32", "pc-windows"],
            _ => vec![],
        }
    }

    fn arch_variants(&self) -> Vec<&str> {
        match self.target_arch.as_deref() {
            Some("x86_64") | Some("x64") | Some("amd64") => {
                vec!["x86_64", "x64", "amd64", "64bit"]
            }
            Some("aarch64") | Some("arm64") => vec!["aarch64", "arm64"],
            Some("x86") | Some("i686") | Some("i386") => vec!["x86", "i686", "i386", "32bit"],
            Some("arm") => vec!["arm", "armv7"],
            _ => vec![],
        }
    }

    fn match_by_auto_detection(&self, assets: &[String]) -> Result<MatchedAsset> {
        let picker = self
            .create_picker()
            .ok_or_else(|| eyre::eyre!("Target OS and arch must be set for auto-detection"))?;

        let best = picker.pick_best_asset(assets).ok_or_else(|| {
            let os = self.target_os.as_deref().unwrap_or("unknown");
            let arch = self.target_arch.as_deref().unwrap_or("unknown");
            eyre::eyre!(
                "No matching asset found for platform {}-{}\nAvailable assets:\n{}",
                os,
                arch,
                assets.join("\n")
            )
        })?;

        let score = picker.score_asset(&best);

        Ok(MatchedAsset {
            name: best,
            url: None,
            score,
            match_type: MatchType::AutoDetected,
        })
    }
}

/// Convenience function to detect the best asset for the current platform
pub fn detect_best_asset(assets: &[String]) -> Result<String> {
    AssetMatcher::new()
        .for_current_platform()
        .pick_from(assets)
        .map(|m| m.name)
}

/// Convenience function to detect the best asset for a target platform
pub fn detect_asset_for_target(assets: &[String], target: &PlatformTarget) -> Result<String> {
    AssetMatcher::new()
        .for_target(target)
        .pick_from(assets)
        .map(|m| m.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_assets() -> Vec<String> {
        vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-linux-aarch64.tar.gz".to_string(),
            "tool-1.0.0-darwin-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-arm64.tar.gz".to_string(),
            "tool-1.0.0-windows-x86_64.zip".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.sha256".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.sig".to_string(),
            "checksums.txt".to_string(),
        ]
    }

    #[test]
    fn test_auto_detection_linux_x64() {
        let matcher = AssetMatcher::new().with_os("linux").with_arch("x86_64");

        let result = matcher.pick_from(&test_assets()).unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64.tar.gz");
        assert_eq!(result.match_type, MatchType::AutoDetected);
    }

    #[test]
    fn test_auto_detection_macos_arm64() {
        let matcher = AssetMatcher::new().with_os("macos").with_arch("aarch64");

        let result = matcher.pick_from(&test_assets()).unwrap();
        assert_eq!(result.name, "tool-1.0.0-darwin-arm64.tar.gz");
    }

    #[test]
    fn test_pattern_matching() {
        let matcher = AssetMatcher::new()
            .with_pattern("tool-{version}-linux-x86_64.tar.gz")
            .with_version("1.0.0");

        let result = matcher.pick_from(&test_assets()).unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64.tar.gz");
        assert_eq!(result.match_type, MatchType::Pattern);
    }

    #[test]
    fn test_find_checksum() {
        let matcher = AssetMatcher::new();
        let checksum = matcher.find_checksum_for("tool-1.0.0-linux-x86_64.tar.gz", &test_assets());
        assert_eq!(
            checksum,
            Some("tool-1.0.0-linux-x86_64.tar.gz.sha256".to_string())
        );
    }

    #[test]
    fn test_find_checksum_global() {
        let matcher = AssetMatcher::new();
        let assets = vec!["tool-1.0.0.tar.gz".to_string(), "checksums.txt".to_string()];
        let checksum = matcher.find_checksum_for("tool-1.0.0.tar.gz", &assets);
        assert_eq!(checksum, Some("checksums.txt".to_string()));
    }

    #[test]
    fn test_find_signature() {
        let matcher = AssetMatcher::new();
        let sig = matcher.find_signature_for("tool-1.0.0-linux-x86_64.tar.gz", &test_assets());
        assert_eq!(sig, Some("tool-1.0.0-linux-x86_64.tar.gz.sig".to_string()));
    }

    #[test]
    fn test_exclude_filter() {
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .exclude(["tool-1.0.0-linux-x86_64.tar.gz"]);

        let result = matcher.pick_from(&test_assets()).unwrap();
        // Should pick aarch64 since x86_64 is excluded
        assert_eq!(result.name, "tool-1.0.0-linux-aarch64.tar.gz");
    }

    #[test]
    fn test_custom_filter() {
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .with_filter(|name| !name.contains("aarch64"));

        let result = matcher.pick_from(&test_assets()).unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_find_all() {
        let matcher = AssetMatcher::new().with_os("linux").with_arch("x86_64");

        let results = matcher.find_all(&test_assets());
        assert!(!results.is_empty());
        // First result should be the best match
        assert_eq!(results[0].name, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_ripgrep_assets() {
        let ripgrep_assets = vec![
            "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-unknown-linux-musl.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-pc-windows-msvc.zip".to_string(),
            "ripgrep-14.1.1-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz".to_string(),
        ];

        // Linux x86_64 should prefer gnu or musl based on build
        let matcher = AssetMatcher::new().with_os("linux").with_arch("x86_64");
        let result = matcher.pick_from(&ripgrep_assets).unwrap();
        assert!(result.name.contains("linux") && result.name.contains("x86_64"));

        // macOS arm64
        let matcher = AssetMatcher::new().with_os("macos").with_arch("aarch64");
        let result = matcher.pick_from(&ripgrep_assets).unwrap();
        assert_eq!(result.name, "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz");

        // Windows x86_64
        let matcher = AssetMatcher::new().with_os("windows").with_arch("x86_64");
        let result = matcher.pick_from(&ripgrep_assets).unwrap();
        assert_eq!(result.name, "ripgrep-14.1.1-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn test_convenience_functions() {
        let assets = vec![
            "tool-linux-x64.tar.gz".to_string(),
            "tool-darwin-arm64.tar.gz".to_string(),
        ];

        // This will use the current platform, so we just check it doesn't panic
        let _ = detect_best_asset(&assets);
    }
}
