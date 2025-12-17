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

use super::platform_target::PlatformTarget;
use super::static_helpers::get_filename_from_url;
use crate::config::Settings;
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
}

impl AssetPicker {
    pub fn new(target_os: String, target_arch: String) -> Self {
        Self::with_libc(target_os, target_arch, None)
    }

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
        }
    }

    /// Picks the best asset from available options
    pub fn pick_best_asset(&self, assets: &[String]) -> Option<String> {
        let candidates = self.filter_archive_assets(assets);
        let mut scored_assets = self.score_all_assets(&candidates);
        scored_assets.sort_by(|a, b| b.0.cmp(&a.0));
        scored_assets
            .first()
            .filter(|(score, _)| *score > 0)
            .map(|(_, asset)| asset.clone())
    }

    fn filter_archive_assets(&self, assets: &[String]) -> Vec<String> {
        let archive_assets: Vec<String> = assets
            .iter()
            .filter(|name| ARCHIVE_EXTENSIONS.iter().any(|ext| name.ends_with(ext)))
            .cloned()
            .collect();

        if archive_assets.is_empty() {
            assets.to_vec()
        } else {
            archive_assets
        }
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
        0
    }

    fn score_arch_match(&self, asset: &str) -> i32 {
        for (arch, pattern) in ARCH_PATTERNS.iter() {
            if pattern.is_match(asset) {
                return if arch.matches_target(&self.target_arch) {
                    50
                } else {
                    -25
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
        if ARCHIVE_EXTENSIONS.iter().any(|ext| asset.ends_with(ext)) {
            10
        } else {
            0
        }
    }

    fn score_build_penalties(&self, asset: &str) -> i32 {
        let mut penalty = 0;
        if asset.contains("debug") || asset.contains("test") {
            penalty -= 20;
        }
        if asset.contains(".artifactbundle") {
            penalty -= 30;
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
        let is_archive = |name: &str| ARCHIVE_EXTENSIONS.iter().any(|ext| name.ends_with(ext));

        let filtered: Vec<String> = assets
            .iter()
            .filter(|a| !self.exclude.contains(*a))
            .filter(|a| self.filter.as_ref().is_none_or(|f| f(a)))
            // If allow_binary is false, filter out non-archive files
            .filter(|a| self.allow_binary || is_archive(a))
            .cloned()
            .collect();

        // If prefer_archive is true and we have archives, return only archives
        if self.prefer_archive {
            let archives: Vec<String> =
                filtered.iter().filter(|a| is_archive(a)).cloned().collect();
            if !archives.is_empty() {
                return archives;
            }
        }

        filtered
    }

    fn create_picker(&self) -> Option<AssetPicker> {
        let os = self.target_os.as_ref()?;
        let arch = self.target_arch.as_ref()?;
        Some(AssetPicker::with_libc(
            os.clone(),
            arch.clone(),
            self.target_libc.clone(),
        ))
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

        // Check if score meets minimum threshold
        if score < self.min_score {
            let os = self.target_os.as_deref().unwrap_or("unknown");
            let arch = self.target_arch.as_deref().unwrap_or("unknown");
            bail!(
                "Best matching asset '{}' has score {} which is below minimum threshold {}\nPlatform: {}-{}",
                best,
                score,
                self.min_score,
                os,
                arch
            );
        }

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

    /// Create assets from names and a base URL pattern
    pub fn from_names_with_base_url(names: &[String], base_url: &str) -> Vec<Self> {
        names
            .iter()
            .map(|name| Self {
                name: name.clone(),
                url: format!("{}/{}", base_url.trim_end_matches('/'), name),
            })
            .collect()
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

/// Convenience function to find and fetch checksum for an asset
///
/// # Arguments
/// * `assets` - List of assets with URLs
/// * `asset_name` - The asset to find checksum for
///
/// # Returns
/// * `Some(ChecksumResult)` if found and parsed successfully
/// * `None` if no checksum found or parsing failed
pub async fn fetch_checksum_for_asset(
    assets: &[Asset],
    asset_name: &str,
) -> Option<ChecksumResult> {
    ChecksumFetcher::new(assets)
        .fetch_checksum_for(asset_name)
        .await
}

/// Find a signature file URL for an asset
///
/// # Arguments
/// * `assets` - List of assets with URLs
/// * `asset_name` - The asset to find signature for
///
/// # Returns
/// * `Some(Asset)` containing the signature file
/// * `None` if no signature found
pub fn find_signature_asset(assets: &[Asset], asset_name: &str) -> Option<Asset> {
    let asset_names: Vec<String> = assets.iter().map(|a| a.name.clone()).collect();
    let matcher = AssetMatcher::new();

    if let Some(sig_filename) = matcher.find_signature_for(asset_name, &asset_names) {
        assets.iter().find(|a| a.name == sig_filename).cloned()
    } else {
        None
    }
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

        let names = vec!["a.tar.gz".to_string(), "b.tar.gz".to_string()];
        let assets = Asset::from_names_with_base_url(&names, "https://example.com/releases/");
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[0].url, "https://example.com/releases/a.tar.gz");
        assert_eq!(assets[1].url, "https://example.com/releases/b.tar.gz");
    }

    #[test]
    fn test_find_signature_asset() {
        let assets = vec![
            Asset::new(
                "tool-1.0.0-linux-x64.tar.gz",
                "https://example.com/tool-1.0.0-linux-x64.tar.gz",
            ),
            Asset::new(
                "tool-1.0.0-linux-x64.tar.gz.sig",
                "https://example.com/tool-1.0.0-linux-x64.tar.gz.sig",
            ),
        ];

        let sig = find_signature_asset(&assets, "tool-1.0.0-linux-x64.tar.gz");
        assert!(sig.is_some());
        assert_eq!(sig.unwrap().name, "tool-1.0.0-linux-x64.tar.gz.sig");

        let no_sig = find_signature_asset(&assets, "other-file.tar.gz");
        assert!(no_sig.is_none());
    }

    // ========== Platform Detection Tests (from asset_detector) ==========

    #[test]
    fn test_asset_picker_functionality() {
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());
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
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());

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
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());
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

        let picker = AssetPicker::new("windows".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&qsv_assets).unwrap();
        assert_eq!(picked, "qsv-8.1.1-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn test_with_libc_setting_respected() {
        // Test that explicit libc setting is passed through to AssetPicker
        let assets = vec![
            "tool-1.0.0-linux-x86_64-gnu.tar.gz".to_string(),
            "tool-1.0.0-linux-x86_64-musl.tar.gz".to_string(),
        ];

        // Explicitly request musl
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .with_libc("musl");

        let result = matcher.pick_from(&assets).unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64-musl.tar.gz");

        // Explicitly request gnu
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .with_libc("gnu");

        let result = matcher.pick_from(&assets).unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64-gnu.tar.gz");
    }

    #[test]
    fn test_allow_binary_false_filters_binaries() {
        let assets = vec![
            "tool-linux-x64".to_string(),        // binary (no extension)
            "tool-linux-x64.tar.gz".to_string(), // archive
        ];

        // With allow_binary = false, should only get archives
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .allow_binary(false);

        let result = matcher.pick_from(&assets).unwrap();
        assert_eq!(result.name, "tool-linux-x64.tar.gz");

        // Verify the binary was filtered out
        let all = matcher.find_all(&assets);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "tool-linux-x64.tar.gz");
    }

    #[test]
    fn test_prefer_archive_filters_when_archives_exist() {
        let assets = vec![
            "tool-linux-x64".to_string(),        // binary
            "tool-linux-x64.tar.gz".to_string(), // archive
            "tool-linux-x64.zip".to_string(),    // archive
        ];

        // With prefer_archive = true, should only consider archives
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .prefer_archive(true);

        let all = matcher.find_all(&assets);
        // Should only have the archives, not the binary
        assert_eq!(all.len(), 2);
        assert!(
            all.iter()
                .all(|m| m.name.ends_with(".tar.gz") || m.name.ends_with(".zip"))
        );
    }

    #[test]
    fn test_prefer_archive_allows_binary_when_no_archives() {
        let assets = vec![
            "tool-linux-x64".to_string(), // binary only
        ];

        // With prefer_archive = true but no archives, should still match binary
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .prefer_archive(true);

        let result = matcher.pick_from(&assets).unwrap();
        assert_eq!(result.name, "tool-linux-x64");
    }

    #[test]
    fn test_min_score_respected_by_pick_from() {
        let assets = vec![
            "tool-unknown-platform.tar.gz".to_string(), // Low score - no OS/arch match
        ];

        // With a high min_score, should fail to match
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .min_score(100); // Require high score

        let result = matcher.pick_from(&assets);
        assert!(
            result.is_err(),
            "Should fail when best score is below min_score"
        );
    }

    #[test]
    fn test_min_score_passes_when_met() {
        let assets = vec![
            "tool-linux-x86_64.tar.gz".to_string(), // High score - matches OS and arch
        ];

        // With a reasonable min_score, should succeed
        let matcher = AssetMatcher::new()
            .with_os("linux")
            .with_arch("x86_64")
            .min_score(50);

        let result = matcher.pick_from(&assets);
        assert!(result.is_ok(), "Should succeed when score meets min_score");
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
}
