//! Unified asset matching for backend tool installation
//!
//! This module provides a high-level `AssetMatcher` that uses platform heuristics
//! to score and rank assets for finding the best download for the target platform.
//!
//! # Example
//!
//! ```ignore
//! use crate::backend::asset_matcher::AssetMatcher;
//!
//! // Auto-detect best asset for a target platform
//! let asset = AssetMatcher::new()
//!     .for_target(&target)
//!     .with_no_app(true) // optional: avoid .app bundles
//!     .pick_from(&assets)?;
//! ```

use eyre::Result;
use regex::Regex;
use std::sync::LazyLock;

use super::platform_target::PlatformTarget;
use super::static_helpers::get_filename_from_url;
use crate::http::HTTP;

// ========== Platform Detection Types (from asset_detector) ==========

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssetLibc {
    Gnu,
    Musl,
    Msvc,
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

impl AssetLibc {
    pub fn matches_target(&self, target: &str) -> bool {
        match self {
            AssetLibc::Gnu => target == "gnu",
            AssetLibc::Musl => target == "musl",
            AssetLibc::Msvc => target == "msvc",
        }
    }
}

/// Detected platform information from a URL
#[derive(Debug, Clone)]
pub struct DetectedPlatform {
    pub os: AssetOs,
    pub arch: AssetArch,
    #[allow(unused)]
    pub libc: Option<AssetLibc>,
}

impl DetectedPlatform {
    /// Convert to mise's platform string format (e.g., "linux-x64", "macos-arm64")
    pub fn to_platform_string(&self) -> String {
        let os_str = match self.os {
            AssetOs::Linux => "linux",
            AssetOs::Macos => "macos",
            AssetOs::Windows => "windows",
        };

        let arch_str = match self.arch {
            AssetArch::X64 => "x64",
            AssetArch::Arm64 => "arm64",
            AssetArch::X86 => "x86",
            AssetArch::Arm => "arm",
        };

        format!("{os_str}-{arch_str}")
    }
}

// Platform detection patterns
static OS_PATTERNS: LazyLock<Vec<(AssetOs, Regex)>> = LazyLock::new(|| {
    vec![
        (
            AssetOs::Linux,
            Regex::new(r"(?i)(?:\b|_)(?:linux|ubuntu|debian|fedora|centos|rhel|alpine|arch)(?:\b|_|32|64|-)")
                .unwrap(),
        ),
        (
            AssetOs::Macos,
            Regex::new(r"(?i)(?:\b|_)(?:darwin|mac(?:osx?)?|osx)(?:\b|_)").unwrap(),
        ),
        (
            AssetOs::Windows,
            Regex::new(r"(?i)(?:\b|_)(?:mingw-w64|win(?:32|64|dows)?)(?:\b|_)").unwrap(),
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

static LIBC_PATTERNS: LazyLock<Vec<(AssetLibc, Regex)>> = LazyLock::new(|| {
    vec![
        (
            AssetLibc::Msvc,
            Regex::new(r"(?i)(?:\b|_)(?:msvc)(?:\b|_)").unwrap(),
        ),
        (
            AssetLibc::Gnu,
            Regex::new(r"(?i)(?:\b|_)(?:gnu|glibc)(?:\b|_)").unwrap(),
        ),
        (
            AssetLibc::Musl,
            Regex::new(r"(?i)(?:\b|_)(?:musl)(?:\b|_)").unwrap(),
        ),
    ]
});

static ARCHIVE_EXTENSIONS: &[&str] = &[
    ".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".txz", ".tzst", ".zip", ".7z",
    ".tar",
];

// ========== AssetPicker (from asset_detector) ==========

/// Automatically detects the best asset for the current platform
pub struct AssetPicker {
    target_os: String,
    target_arch: String,
    target_libc: String,
    no_app: bool,
}

impl AssetPicker {
    /// Create an AssetPicker with an explicit libc setting
    pub fn with_libc(target_os: String, target_arch: String, libc: Option<String>) -> Self {
        let target_libc = libc.unwrap_or_else(|| {
            if target_os == "windows" {
                "msvc".to_string()
            } else if cfg!(target_env = "musl") {
                "musl".to_string()
            } else {
                "gnu".to_string()
            }
        });

        Self {
            target_os,
            target_arch,
            target_libc,
            no_app: false,
        }
    }

    /// Set whether to avoid .app bundles (prefer standalone CLI tools)
    pub fn with_no_app(mut self, no_app: bool) -> Self {
        self.no_app = no_app;
        self
    }

    /// Picks the best asset from available options
    pub fn pick_best_asset(&self, assets: &[String]) -> Option<String> {
        let mut scored_assets = self.score_all_assets(assets);
        scored_assets.sort_by(|a, b| b.0.cmp(&a.0));
        scored_assets
            .first()
            .filter(|(score, _)| *score > 0)
            .map(|(_, asset)| asset.clone())
    }

    /// Picks the best provenance file for the current platform from available assets.
    /// Returns the provenance file that best matches the target OS and architecture.
    pub fn pick_best_provenance(&self, assets: &[String]) -> Option<String> {
        // Filter to only provenance files
        let provenance_assets: Vec<&String> = assets
            .iter()
            .filter(|a| {
                let name = a.to_lowercase();
                name.contains(".intoto.jsonl")
                    || name.contains("provenance")
                    || name.ends_with(".attestation")
            })
            .collect();

        if provenance_assets.is_empty() {
            return None;
        }

        // Score by platform match only (no format/build penalties)
        let mut scored: Vec<(i32, &String)> = provenance_assets
            .into_iter()
            .map(|asset| {
                let score = self.score_os_match(asset) + self.score_arch_match(asset);
                (score, asset)
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.first().map(|(_, asset)| (*asset).clone())
    }

    fn score_all_assets(&self, assets: &[String]) -> Vec<(i32, String)> {
        assets
            .iter()
            .map(|asset| (self.score_asset(asset), asset.clone()))
            .collect()
    }

    /// Scores a single asset based on platform compatibility
    pub fn score_asset(&self, asset: &str) -> i32 {
        let mut score = 0;
        score += self.score_os_match(asset);
        score += self.score_arch_match(asset);
        if self.target_os == "linux" || self.target_os == "windows" {
            score += self.score_libc_match(asset);
        }
        score += self.score_format_preferences(asset);
        score += self.score_build_penalties(asset);
        score
    }

    fn score_os_match(&self, asset: &str) -> i32 {
        for (os, pattern) in OS_PATTERNS.iter() {
            if pattern.is_match(asset) {
                return if os.matches_target(&self.target_os) {
                    100
                } else {
                    -100
                };
            }
        }
        // Check for Windows-specific file extensions (.msi, .exe)
        // These should be penalized on non-Windows platforms
        // See: https://github.com/jdx/mise/discussions/7837
        let lower = asset.to_lowercase();
        if (lower.ends_with(".msi") || lower.ends_with(".exe")) && self.target_os != "windows" {
            return -100;
        }
        // On Windows, these are valid but don't need a boost - let other
        // factors (arch match, format preferences) determine the best asset
        0
    }

    fn score_arch_match(&self, asset: &str) -> i32 {
        for (arch, pattern) in ARCH_PATTERNS.iter() {
            if pattern.is_match(asset) {
                return if arch.matches_target(&self.target_arch) {
                    50
                } else {
                    // Architecture mismatch should be disqualifying - don't silently
                    // fall back to incompatible architectures (e.g., x86_64 when arm64
                    // is requested). See: https://github.com/jdx/mise/discussions/7628
                    -150
                };
            }
        }
        0
    }

    fn score_libc_match(&self, asset: &str) -> i32 {
        for (libc, pattern) in LIBC_PATTERNS.iter() {
            if pattern.is_match(asset) {
                return if libc.matches_target(&self.target_libc) {
                    25
                } else {
                    -10
                };
            }
        }
        0
    }

    fn score_format_preferences(&self, asset: &str) -> i32 {
        let asset = asset.to_lowercase();
        if asset.ends_with(".zip") {
            if self.target_os == "windows" {
                return 15;
            } else {
                return 5;
            }
        }
        if ARCHIVE_EXTENSIONS.iter().any(|ext| asset.ends_with(ext)) {
            10
        } else {
            0
        }
    }

    fn score_build_penalties(&self, asset: &str) -> i32 {
        let mut penalty = 0;
        let asset = asset.to_lowercase();
        if asset.contains("debug") || asset.contains("test") {
            penalty -= 20;
        }
        if asset.ends_with(".artifactbundle") || asset.contains(".artifactbundle.") {
            penalty -= 30;
        }
        // Penalize macOS .app bundles on non-macOS platforms
        if asset.contains(".app.") && self.target_os != "macos" {
            penalty -= 100;
        }
        // Penalize .app bundles if no_app option is set
        // .app bundles often contain Xcode extensions or GUI apps, not CLI tools
        if self.no_app && asset.contains(".app.") {
            penalty -= 50;
        }

        // Penalize .vsix files
        if asset.ends_with(".vsix") {
            penalty -= 100;
        }

        // Penalize metadata/checksum/signature files
        if asset.ends_with(".asc")
            || asset.ends_with(".sig")
            || asset.ends_with(".sign")
            || asset.ends_with(".sha256")
            || asset.ends_with(".sha512")
            || asset.ends_with(".sha1")
            || asset.ends_with(".md5")
            || asset.ends_with(".json")
            || asset.ends_with(".txt")
            || asset.ends_with(".xml")
            || asset.ends_with(".sbom")
            || asset.ends_with(".spdx")
            || asset.ends_with(".intoto")
            || asset.ends_with(".attestation")
            || asset.ends_with(".pem")
            || asset.ends_with(".crt")
            || asset.ends_with(".key")
            || asset.ends_with(".pub")
            || asset.ends_with(".manifest")
        {
            penalty -= 100;
        }

        // Penalize common non-binary filenames
        if asset.contains("release-info") || asset.contains("changelog") {
            penalty -= 50;
        }

        penalty
    }
}

/// Detects platform information from a URL
pub fn detect_platform_from_url(url: &str) -> Option<DetectedPlatform> {
    let mut detected_os = None;
    let mut detected_arch = None;
    let mut detected_libc = None;

    let filename = get_filename_from_url(url);

    for (os, pattern) in OS_PATTERNS.iter() {
        if pattern.is_match(&filename) {
            detected_os = Some(*os);
            break;
        }
    }

    for (arch, pattern) in ARCH_PATTERNS.iter() {
        if pattern.is_match(&filename) {
            detected_arch = Some(*arch);
            break;
        }
    }

    if detected_os == Some(AssetOs::Linux) || detected_os == Some(AssetOs::Windows) {
        for (libc, pattern) in LIBC_PATTERNS.iter() {
            if pattern.is_match(&filename) {
                detected_libc = Some(*libc);
                break;
            }
        }
    }

    if let (Some(os), Some(arch)) = (detected_os, detected_arch) {
        Some(DetectedPlatform {
            os,
            arch,
            libc: detected_libc,
        })
    } else {
        None
    }
}

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

/// Represents a matched asset with metadata
#[derive(Debug, Clone)]
pub struct MatchedAsset {
    /// The asset name/filename
    pub name: String,
}

/// Builder for matching assets
#[derive(Debug, Default)]
pub struct AssetMatcher {
    /// Target OS (e.g., "linux", "macos", "windows")
    target_os: Option<String>,
    /// Target architecture (e.g., "x86_64", "aarch64")
    target_arch: Option<String>,
    /// Target libc variant (e.g., "gnu", "musl")
    target_libc: Option<String>,
    /// Whether to avoid .app bundles
    no_app: bool,
}

impl AssetMatcher {
    /// Create a new AssetMatcher with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure for a specific target platform
    pub fn for_target(mut self, target: &PlatformTarget) -> Self {
        self.target_os = Some(target.os_name().to_string());
        self.target_arch = Some(target.arch_name().to_string());
        self.target_libc = target.qualifier().map(|s| s.to_string());
        self
    }

    /// Set whether to avoid .app bundles (prefer standalone CLI tools)
    pub fn with_no_app(mut self, no_app: bool) -> Self {
        self.no_app = no_app;
        self
    }

    /// Pick the best matching asset from a list of names
    pub fn pick_from(&self, assets: &[String]) -> Result<MatchedAsset> {
        self.match_by_auto_detection(assets)
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

    // ========== Internal Methods ==========

    fn create_picker(&self) -> Option<AssetPicker> {
        let os = self.target_os.as_ref()?;
        let arch = self.target_arch.as_ref()?;
        Some(
            AssetPicker::with_libc(os.clone(), arch.clone(), self.target_libc.clone())
                .with_no_app(self.no_app),
        )
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

        Ok(MatchedAsset { name: best })
    }
}

// ========== Checksum Fetching Helpers ==========

/// Represents an asset with its download URL
#[derive(Debug, Clone)]
pub struct Asset {
    /// The asset filename
    pub name: String,
    /// The download URL for the asset
    pub url: String,
}

impl Asset {
    pub fn new(name: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            url: url.into(),
        }
    }
}

/// Result of a checksum lookup
#[derive(Debug, Clone)]
pub struct ChecksumResult {
    /// Algorithm used (sha256, sha512, md5, blake3)
    pub algorithm: String,
    /// The hash value
    pub hash: String,
    /// Which checksum file this came from
    pub source_file: String,
}

impl ChecksumResult {
    /// Format as "algorithm:hash" string for verification
    pub fn to_string_formatted(&self) -> String {
        format!("{}:{}", self.algorithm, self.hash)
    }
}

/// Checksum file fetcher that finds and parses checksums from release assets
pub struct ChecksumFetcher<'a> {
    assets: &'a [Asset],
}

impl<'a> ChecksumFetcher<'a> {
    /// Create a new checksum fetcher with the given assets
    pub fn new(assets: &'a [Asset]) -> Self {
        Self { assets }
    }

    /// Find and fetch the checksum for a specific asset
    ///
    /// This method:
    /// 1. Finds a checksum file that matches the asset name
    /// 2. Fetches the checksum file
    /// 3. Parses it to extract the checksum for the target file
    ///
    /// Returns None if no checksum file is found or parsing fails.
    pub async fn fetch_checksum_for(&self, asset_name: &str) -> Option<ChecksumResult> {
        let asset_names: Vec<String> = self.assets.iter().map(|a| a.name.clone()).collect();
        let matcher = AssetMatcher::new();

        // First try to find an asset-specific checksum file (e.g., file.tar.gz.sha256)
        if let Some(checksum_filename) = matcher.find_checksum_for(asset_name, &asset_names)
            && let Some(checksum_asset) = self.assets.iter().find(|a| a.name == checksum_filename)
            && let Some(result) = self
                .fetch_and_parse_checksum(&checksum_asset.url, &checksum_filename, asset_name)
                .await
        {
            return Some(result);
        }

        // Try common global checksum files by exact name match first
        let global_patterns = [
            "checksums.txt",
            "SHA256SUMS",
            "SHASUMS256.txt",
            "sha256sums.txt",
        ];
        for pattern in global_patterns {
            if let Some(checksum_asset) = self
                .assets
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(pattern))
                && let Some(result) = self
                    .fetch_and_parse_checksum(&checksum_asset.url, &checksum_asset.name, asset_name)
                    .await
            {
                return Some(result);
            }
        }

        // Last resort: try any file with "checksum" in the name
        if let Some(checksum_asset) = self
            .assets
            .iter()
            .find(|a| a.name.to_lowercase().contains("checksum"))
            && let Some(result) = self
                .fetch_and_parse_checksum(&checksum_asset.url, &checksum_asset.name, asset_name)
                .await
        {
            return Some(result);
        }

        None
    }

    /// Fetch a checksum file and parse it for the target asset
    async fn fetch_and_parse_checksum(
        &self,
        url: &str,
        checksum_filename: &str,
        target_asset: &str,
    ) -> Option<ChecksumResult> {
        let content = match HTTP.get_text(url).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to fetch checksum file {}: {}", url, e);
                return None;
            }
        };

        // Detect algorithm from filename
        let algorithm = detect_checksum_algorithm(checksum_filename);

        // Try to parse the checksum
        parse_checksum_content(&content, target_asset, &algorithm, checksum_filename)
    }
}

/// Detect the checksum algorithm from the filename
fn detect_checksum_algorithm(filename: &str) -> String {
    let lower = filename.to_lowercase();
    if lower.contains("sha512") || lower.ends_with(".sha512") || lower.ends_with(".sha512sum") {
        "sha512".to_string()
    } else if lower.contains("md5") || lower.ends_with(".md5") || lower.ends_with(".md5sum") {
        "md5".to_string()
    } else if lower.contains("blake3") || lower.ends_with(".b3") {
        "blake3".to_string()
    } else {
        // Default to sha256 (most common)
        "sha256".to_string()
    }
}

/// Parse checksum content and extract the hash for a specific file
fn parse_checksum_content(
    content: &str,
    target_file: &str,
    algorithm: &str,
    source_file: &str,
) -> Option<ChecksumResult> {
    let trimmed = content.trim();

    // Check if this looks like a multi-line SHASUMS file (has lines with two parts)
    let is_shasums_format = trimmed.lines().any(|line| {
        let parts: Vec<&str> = line.split_whitespace().collect();
        parts.len() >= 2
    });

    if is_shasums_format {
        // Try standard SHASUMS format: "<hash>  <filename>" or "<hash> *<filename>"
        // Parse manually to avoid panic from hash::parse_shasums
        for line in trimmed.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(hash_str), Some(filename)) = (parts.next(), parts.next()) {
                // Strip leading * or . from filename if present (some formats use this)
                let clean_filename = filename.trim_start_matches(['*', '.']);
                if (clean_filename == target_file || filename == target_file)
                    && is_valid_hash(hash_str, algorithm)
                {
                    return Some(ChecksumResult {
                        algorithm: algorithm.to_string(),
                        hash: hash_str.to_string(),
                        source_file: source_file.to_string(),
                    });
                }
            }
        }
        // Target file not found in SHASUMS file - return None, don't fall through
        return None;
    }

    // Only for single-file checksum (e.g., file.tar.gz.sha256), extract just the hash
    // Format is typically "<hash>" or "<hash>  <filename>"
    if let Some(first_word) = trimmed.split_whitespace().next() {
        // Validate it looks like a hash (hex string of appropriate length)
        if is_valid_hash(first_word, algorithm) {
            return Some(ChecksumResult {
                algorithm: algorithm.to_string(),
                hash: first_word.to_string(),
                source_file: source_file.to_string(),
            });
        }
    }

    None
}

/// Check if a string looks like a valid hash for the given algorithm
fn is_valid_hash(s: &str, algorithm: &str) -> bool {
    let expected_len = match algorithm {
        "sha256" => 64,
        "sha512" => 128,
        "md5" => 32,
        "blake3" => 64,
        _ => return s.len() >= 32, // At least 128 bits
    };
    s.len() == expected_len && s.chars().all(|c| c.is_ascii_hexdigit())
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
            "checksums.txt".to_string(),
        ]
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

    // ========== Checksum Helper Tests ==========

    #[test]
    fn test_detect_checksum_algorithm() {
        assert_eq!(detect_checksum_algorithm("SHA256SUMS"), "sha256");
        assert_eq!(detect_checksum_algorithm("file.sha256"), "sha256");
        assert_eq!(detect_checksum_algorithm("sha256sums.txt"), "sha256");
        assert_eq!(detect_checksum_algorithm("SHA512SUMS"), "sha512");
        assert_eq!(detect_checksum_algorithm("file.sha512"), "sha512");
        assert_eq!(detect_checksum_algorithm("file.md5"), "md5");
        assert_eq!(detect_checksum_algorithm("MD5SUMS"), "md5");
        assert_eq!(detect_checksum_algorithm("checksums.b3"), "blake3");
        assert_eq!(detect_checksum_algorithm("checksums.txt"), "sha256"); // default
    }

    #[test]
    fn test_is_valid_hash() {
        // SHA256 (64 chars)
        assert!(is_valid_hash(
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sha256"
        ));
        assert!(!is_valid_hash("e3b0c44298fc1c149afbf4c8996fb924", "sha256")); // too short

        // SHA512 (128 chars)
        assert!(is_valid_hash(
            "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e",
            "sha512"
        ));

        // MD5 (32 chars)
        assert!(is_valid_hash("d41d8cd98f00b204e9800998ecf8427e", "md5"));
        assert!(!is_valid_hash("d41d8cd98f00b204", "md5")); // too short

        // Invalid characters
        assert!(!is_valid_hash(
            "g3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "sha256"
        ));
    }

    #[test]
    fn test_parse_checksum_content_shasums_format() {
        let content = r#"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  tool-1.0.0-linux-x64.tar.gz
abc123def456abc123def456abc123def456abc123def456abc123def456abcd  tool-1.0.0-darwin-arm64.tar.gz"#;

        let result = parse_checksum_content(
            content,
            "tool-1.0.0-linux-x64.tar.gz",
            "sha256",
            "SHA256SUMS",
        );

        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.algorithm, "sha256");
        assert_eq!(
            r.hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(r.source_file, "SHA256SUMS");
    }

    #[test]
    fn test_parse_checksum_content_single_file() {
        let content = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

        let result = parse_checksum_content(
            content,
            "tool-1.0.0-linux-x64.tar.gz",
            "sha256",
            "tool-1.0.0-linux-x64.tar.gz.sha256",
        );

        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.algorithm, "sha256");
        assert_eq!(
            r.hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_parse_checksum_content_with_filename_suffix() {
        // Checksum file with format: "<hash>  filename" should match the filename
        let content =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  tool.tar.gz";

        // Should return the hash when target matches the filename
        let result = parse_checksum_content(content, "tool.tar.gz", "sha256", "tool.sha256");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            r.hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        // Should return None when target doesn't match the filename
        let result = parse_checksum_content(content, "other-file.tar.gz", "sha256", "tool.sha256");
        assert!(
            result.is_none(),
            "Should not return hash for wrong target file"
        );
    }

    #[test]
    fn test_checksum_result_format() {
        let result = ChecksumResult {
            algorithm: "sha256".to_string(),
            hash: "abc123".to_string(),
            source_file: "checksums.txt".to_string(),
        };

        assert_eq!(result.to_string_formatted(), "sha256:abc123");
    }

    #[test]
    fn test_asset_creation() {
        let asset = Asset::new("file.tar.gz", "https://example.com/file.tar.gz");
        assert_eq!(asset.name, "file.tar.gz");
        assert_eq!(asset.url, "https://example.com/file.tar.gz");
    }

    // ========== Platform Detection Tests ==========

    #[test]
    fn test_asset_picker_functionality() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-x86_64.tar.gz".to_string(),
            "tool-1.0.0-windows-x86_64.zip".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_asset_picker_functionality_mixed() {
        // mixed archive/binary formats like in babs/multiping
        let assets = vec![
            "tool-1.0.0-linux-x86_64.xz".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-x86_64.xz".to_string(),
            "tool-1.0.0-darwin-aarch64.xz".to_string(),
            "tool-1.0.0-mingw-w64-x86_64.zip".to_string(),
        ];

        let picked = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");

        let picked = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "tool-1.0.0-darwin-aarch64.xz");

        let picked = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None)
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "tool-1.0.0-mingw-w64-x86_64.zip");
    }

    #[test]
    fn test_asset_scoring() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);

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
        // Architecture mismatch should result in negative score
        assert!(
            score_linux_arm < 0,
            "Architecture mismatch should be negative, got {}",
            score_linux_arm
        );
    }

    #[test]
    fn test_archive_preference() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-x86_64".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_platform_detection_from_url() {
        // Test Node.js URL
        let url = "https://nodejs.org/dist/v22.17.1/node-v22.17.1-darwin-arm64.tar.gz";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Macos);
        assert_eq!(platform.arch, AssetArch::Arm64);
        assert_eq!(platform.to_platform_string(), "macos-arm64");

        // Test Linux x64 URL
        let url = "https://github.com/BurntSushi/ripgrep/releases/download/14.0.3/ripgrep-14.0.3-x86_64-unknown-linux-musl.tar.gz";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Linux);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.libc, Some(AssetLibc::Musl));
        assert_eq!(platform.to_platform_string(), "linux-x64");

        // Test Windows URL
        let url =
            "https://github.com/cli/cli/releases/download/v2.336.0/gh_2.336.0_windows_amd64.zip";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Windows);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.to_platform_string(), "windows-x64");

        // Test URL without platform info
        let url = "https://example.com/generic-tool.tar.gz";
        let platform = detect_platform_from_url(url);
        assert!(platform.is_none());
    }

    #[test]
    fn test_platform_string_conversion() {
        let platform = DetectedPlatform {
            os: AssetOs::Linux,
            arch: AssetArch::X64,
            libc: Some(AssetLibc::Gnu),
        };
        assert_eq!(platform.to_platform_string(), "linux-x64");

        let platform = DetectedPlatform {
            os: AssetOs::Macos,
            arch: AssetArch::Arm64,
            libc: None,
        };
        assert_eq!(platform.to_platform_string(), "macos-arm64");

        let platform = DetectedPlatform {
            os: AssetOs::Windows,
            arch: AssetArch::X86,
            libc: None,
        };
        assert_eq!(platform.to_platform_string(), "windows-x86");
    }

    #[test]
    fn test_windows_msvc_preference() {
        let qsv_assets = vec![
            "qsv-8.1.1-x86_64-pc-windows-gnu.zip".to_string(),
            "qsv-8.1.1-x86_64-pc-windows-msvc.zip".to_string(),
        ];

        let picker = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None);
        let picked = picker.pick_best_asset(&qsv_assets).unwrap();
        assert_eq!(picked, "qsv-8.1.1-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn test_for_target_with_libc_qualifier() {
        use crate::backend::platform_target::PlatformTarget;
        use crate::platform::Platform;

        let assets = vec![
            "tool-1.0.0-linux-x86_64-gnu.tar.gz".to_string(),
            "tool-1.0.0-linux-x86_64-musl.tar.gz".to_string(),
        ];

        // Test with musl qualifier
        let platform = Platform::parse("linux-x64-musl").unwrap();
        let target = PlatformTarget::new(platform);
        let result = AssetMatcher::new()
            .for_target(&target)
            .pick_from(&assets)
            .unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64-musl.tar.gz");

        // Test with gnu qualifier
        let platform = Platform::parse("linux-x64-gnu").unwrap();
        let target = PlatformTarget::new(platform);
        let result = AssetMatcher::new()
            .for_target(&target)
            .pick_from(&assets)
            .unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64-gnu.tar.gz");
    }

    #[test]
    fn test_parse_checksum_content_returns_none_for_missing_file() {
        // SHASUMS file that doesn't contain the target file
        let content = r#"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  tool-linux.tar.gz
abc123def456abc123def456abc123def456abc123def456abc123def456abcd  tool-darwin.tar.gz"#;

        // Request checksum for a file that's not in the SHASUMS
        let result = parse_checksum_content(
            content,
            "tool-windows.tar.gz", // Not in the file
            "sha256",
            "SHA256SUMS",
        );

        // Should return None, not the first hash
        assert!(
            result.is_none(),
            "Should return None when target file is not in SHASUMS"
        );
    }
    #[test]
    fn test_zip_scoring() {
        // Test Windows preference for .zip
        let picker_win = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None);
        let score_win_zip = picker_win.score_asset("tool-1.0.0-windows-x86_64.zip");
        let score_win_tar = picker_win.score_asset("tool-1.0.0-windows-x86_64.tar.gz");

        assert!(
            score_win_zip > score_win_tar,
            "Windows should prefer .zip (zip: {}, tar: {})",
            score_win_zip,
            score_win_tar
        );

        // Test Linux penalty for .zip
        let picker_linux = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let score_linux_zip = picker_linux.score_asset("tool-1.0.0-linux-x86_64.zip");
        let score_linux_tar = picker_linux.score_asset("tool-1.0.0-linux-x86_64.tar.gz");

        assert!(
            score_linux_tar > score_linux_zip,
            "Linux should prefer .tar.gz over .zip (zip: {}, tar: {})",
            score_linux_zip,
            score_linux_tar
        );
    }

    #[test]
    fn test_artifactbundle_penalty() {
        // Test that .artifactbundle files are penalized (they have different internal structure)
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None);

        // Test .artifactbundle.zip (like sourcery-2.2.7.artifactbundle.zip)
        let assets = vec![
            "sourcery-2.2.7-macos-arm64.zip".to_string(),
            "sourcery-2.2.7.artifactbundle.zip".to_string(),
        ];
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(
            picked, "sourcery-2.2.7-macos-arm64.zip",
            ".artifactbundle.zip should be penalized"
        );

        // Test plain .artifactbundle
        let assets = vec![
            "tool-1.0.0-darwin-arm64.tar.gz".to_string(),
            "tool-1.0.0.artifactbundle".to_string(),
        ];
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(
            picked, "tool-1.0.0-darwin-arm64.tar.gz",
            ".artifactbundle should be penalized"
        );

        // Verify penalty scores
        let score_regular = picker.score_asset("sourcery-2.2.7-macos-arm64.zip");
        let score_bundle_zip = picker.score_asset("sourcery-2.2.7.artifactbundle.zip");
        let score_bundle = picker.score_asset("tool.artifactbundle");

        assert!(
            score_regular > score_bundle_zip,
            "Regular zip should score higher than .artifactbundle.zip (regular: {}, bundle: {})",
            score_regular,
            score_bundle_zip
        );
        assert!(
            score_bundle < 0 || score_bundle < score_regular - 20,
            ".artifactbundle should have penalty applied"
        );
    }

    #[test]
    fn test_arch_mismatch_rejected() {
        // Regression test for https://github.com/jdx/mise/discussions/7628
        // When the requested architecture is not available, we should NOT silently
        // fall back to a different architecture (e.g., x86_64 when arm64 is requested)
        let picker = AssetPicker::with_libc("linux".to_string(), "aarch64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-darwin-arm64.tar.gz".to_string(),
            "tool-1.0.0-windows-x86_64.zip".to_string(),
        ];

        // Should return None because linux-arm64 is not available
        let picked = picker.pick_best_asset(&assets);
        assert!(
            picked.is_none(),
            "Should not fall back to x86_64 when arm64 is requested but unavailable"
        );

        // Verify the score is negative for arch mismatch
        let score = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz");
        assert!(
            score < 0,
            "Architecture mismatch should result in negative score, got {}",
            score
        );
    }

    #[test]
    fn test_metadata_penalty() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.asc".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.sha256".to_string(),
            "release-notes.txt".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");

        // Ensure penalties are applied
        let score_tar = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz");
        let score_asc = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.asc");
        let score_sha = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.sha256");
        let score_txt = picker.score_asset("release-notes.txt");

        assert!(
            score_tar > score_asc,
            "Tarball should score higher than signature"
        );
        assert!(
            score_tar > score_sha,
            "Tarball should score higher than checksum"
        );
        assert!(
            score_tar > score_txt,
            "Tarball should score higher than text file"
        );

        // Metadata should have negative score contribution from penalties
        assert!(score_asc < 0 || score_asc < score_tar - 50);
    }

    // ========== Provenance Picker Tests ==========

    #[test]
    fn test_pick_best_provenance_selects_matching_platform() {
        // Regression test for https://github.com/jdx/mise/discussions/7462
        // When multiple provenance files exist, select the one matching the target platform
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "buildx-v0.30.1.linux-amd64".to_string(),
            "buildx-v0.30.1.darwin-amd64.provenance.json".to_string(),
            "buildx-v0.30.1.linux-amd64.provenance.json".to_string(),
            "buildx-v0.30.1.windows-amd64.provenance.json".to_string(),
        ];

        let picked = picker.pick_best_provenance(&assets).unwrap();
        assert_eq!(
            picked, "buildx-v0.30.1.linux-amd64.provenance.json",
            "Should select Linux provenance for Linux target"
        );
    }

    #[test]
    fn test_pick_best_provenance_darwin() {
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-amd64.provenance.json".to_string(),
            "tool-1.0.0-darwin-arm64.provenance.json".to_string(),
            "tool-1.0.0-darwin-amd64.provenance.json".to_string(),
        ];

        let picked = picker.pick_best_provenance(&assets).unwrap();
        assert_eq!(
            picked, "tool-1.0.0-darwin-arm64.provenance.json",
            "Should select darwin-arm64 provenance for macOS arm64 target"
        );
    }

    #[test]
    fn test_pick_best_provenance_windows() {
        let picker = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "buildkit-v0.26.3.darwin-amd64.provenance.json".to_string(),
            "buildkit-v0.26.3.linux-amd64.provenance.json".to_string(),
            "buildkit-v0.26.3.windows-amd64.provenance.json".to_string(),
            "buildkit-v0.26.3.windows-amd64.tar.gz".to_string(),
        ];

        let picked = picker.pick_best_provenance(&assets).unwrap();
        assert_eq!(
            picked, "buildkit-v0.26.3.windows-amd64.provenance.json",
            "Should select Windows provenance for Windows target"
        );
    }

    #[test]
    fn test_pick_best_provenance_intoto() {
        // Test with .intoto.jsonl format (SLSA provenance)
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-linux-amd64.tar.gz".to_string(),
            "tool-darwin-amd64.intoto.jsonl".to_string(),
            "tool-linux-amd64.intoto.jsonl".to_string(),
        ];

        let picked = picker.pick_best_provenance(&assets).unwrap();
        assert_eq!(
            picked, "tool-linux-amd64.intoto.jsonl",
            "Should select Linux .intoto.jsonl for Linux target"
        );
    }

    #[test]
    fn test_pick_best_provenance_none_available() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-amd64.tar.gz".to_string(),
            "tool-1.0.0-linux-amd64.sha256".to_string(),
        ];

        let picked = picker.pick_best_provenance(&assets);
        assert!(
            picked.is_none(),
            "Should return None when no provenance files exist"
        );
    }

    #[test]
    fn test_pick_best_provenance_single_provenance() {
        // When only one provenance exists, return it even if platform doesn't match
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-amd64.tar.gz".to_string(),
            "tool-1.0.0.provenance.json".to_string(), // No platform info
        ];

        let picked = picker.pick_best_provenance(&assets).unwrap();
        assert_eq!(
            picked, "tool-1.0.0.provenance.json",
            "Should return the only provenance file available"
        );
    }

    #[test]
    fn test_vsix_vs_gz() {
        let picker = AssetPicker::with_libc("macos".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "rust-analyzer-x86_64-apple-darwin.gz".to_string(),
            "rust-analyzer-x86_64-apple-darwin.vsix".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "rust-analyzer-x86_64-apple-darwin.gz");
    }
}
