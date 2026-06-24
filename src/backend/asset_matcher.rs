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
use super::platform_tokens::is_platform_or_version_token;
use super::static_helpers::get_filename_from_url;
use crate::file::ExtractionFormat;
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
    Riscv64,
    Loongarch64,
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
            AssetArch::Riscv64 => target == "riscv64" || target == "riscv64gc",
            AssetArch::Loongarch64 => target == "loongarch64" || target == "loong64",
        }
    }
}

impl AssetLibc {
    pub fn matches_target(&self, target: &str) -> bool {
        target.split('-').any(|part| match self {
            AssetLibc::Gnu => part == "gnu" || part == "glibc",
            AssetLibc::Musl => part == "musl",
            AssetLibc::Msvc => part == "msvc",
        })
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
            AssetArch::Riscv64 => "riscv64",
            AssetArch::Loongarch64 => "loongarch64",
        };

        format!("{os_str}-{arch_str}")
    }
}

// Platform detection patterns
static OS_PATTERNS: LazyLock<Vec<(AssetOs, Regex)>> = LazyLock::new(|| {
    vec![
        (
            AssetOs::Linux,
            Regex::new(r"(?i)(?:\b|_)(?:linux|manylinux(?:[0-9_]+)?|musllinux(?:[0-9_]+)?|ubuntu|debian|fedora|centos|rhel|alpine|arch)(?:\b|_|32|64|-)")
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
        (
            AssetArch::Riscv64,
            Regex::new(r"(?i)(?:\b|_)riscv_?64(?:gc)?(?:\b|_)").unwrap(),
        ),
        (
            AssetArch::Loongarch64,
            Regex::new(r"(?i)(?:\b|_)loong(?:arch)?_?64(?:\b|_)").unwrap(),
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
            Regex::new(r"(?i)(?:\b|_)(?:gnu|glibc|manylinux(?:[0-9_]+)?)(?:\b|_)").unwrap(),
        ),
        (
            AssetLibc::Musl,
            Regex::new(r"(?i)(?:\b|_)(?:musl|musllinux(?:[0-9_]+)?)(?:\b|_)").unwrap(),
        ),
    ]
});

// ========== AssetPicker (from asset_detector) ==========

/// Automatically detects the best asset for the current platform
pub struct AssetPicker {
    target_os: String,
    target_arch: String,
    target_libc: String,
    no_app: bool,
    preferred_name: Option<String>,
    /// Substring that an asset name must contain to remain a candidate.
    /// Applied as a pre-filter before platform scoring (ubi's `matching`).
    matching: Option<String>,
    /// Regex an asset name must match to remain a candidate (ubi's `matching_regex`),
    /// compiled once when the picker is built. `Some(Ok)` is a valid pattern;
    /// `Some(Err(msg))` records that the pattern was set but failed to compile (the
    /// string is a ready-to-surface error message). Caching the compile here makes
    /// regex validity a local property of the picker rather than something that
    /// depends on call ordering between binary and provenance selection.
    matching_regex: Option<Result<Regex, String>>,
}

impl AssetPicker {
    /// Create an AssetPicker with an explicit libc setting.
    /// When no explicit libc is provided, defaults to the platform's standard libc
    /// (msvc for Windows, gnu for Linux/other). The caller is responsible for passing
    /// the correct libc qualifier from PlatformTarget — this avoids polluting
    /// cross-platform lockfile entries with the current system's libc.
    pub fn with_libc(target_os: String, target_arch: String, libc: Option<String>) -> Self {
        let target_libc = libc.unwrap_or_else(|| {
            if target_os == "windows" {
                "msvc".to_string()
            } else {
                "gnu".to_string()
            }
        });

        Self {
            target_os,
            target_arch,
            target_libc,
            no_app: false,
            preferred_name: None,
            matching: None,
            matching_regex: None,
        }
    }

    /// Set whether to avoid .app bundles (prefer standalone CLI tools)
    pub fn with_no_app(mut self, no_app: bool) -> Self {
        self.no_app = no_app;
        self
    }

    /// Prefer assets whose platform-stripped name matches the primary tool.
    pub fn with_preferred_name(mut self, preferred_name: impl Into<String>) -> Self {
        let preferred_name = preferred_name.into();
        if !preferred_name.is_empty() {
            self.preferred_name = Some(preferred_name);
        }
        self
    }

    /// Narrow candidates to assets whose name contains `matching`, before
    /// platform autodetection runs. Ports ubi's `matching` to keep a portable,
    /// autodetecting config for repos that ship multiple binaries per platform.
    pub fn with_matching(mut self, matching: impl Into<String>) -> Self {
        let matching = matching.into();
        if !matching.is_empty() {
            self.matching = Some(matching);
        }
        self
    }

    /// Narrow candidates to assets whose name matches `matching_regex`, before
    /// platform autodetection runs. Ports ubi's `matching_regex`. Empty is a no-op.
    ///
    /// The pattern is compiled here, once, and the result is cached on the picker.
    /// An invalid pattern is retained as `Some(Err(msg))` rather than dropped, so
    /// it can be surfaced as a hard error on the binary path and fails closed on
    /// the provenance path — never silently degrading to "no filter".
    pub fn with_matching_regex(mut self, matching_regex: impl Into<String>) -> Self {
        let matching_regex = matching_regex.into();
        if !matching_regex.is_empty() {
            let compiled = Regex::new(&matching_regex)
                .map_err(|e| format!("invalid matching_regex \"{matching_regex}\": {e}"));
            self.matching_regex = Some(compiled);
        }
        self
    }

    /// The compile error message when `matching_regex` was set but failed to
    /// compile, else `None`. Single source of truth for "is the cached pattern
    /// invalid?" so the binary choke point ([`AssetMatcher::match_by_auto_detection`],
    /// which hard-errors) and the provenance guard ([`Self::pick_best_provenance`],
    /// which returns `None`) decide it the same way and can't drift apart.
    fn matching_regex_error(&self) -> Option<&str> {
        match &self.matching_regex {
            Some(Err(msg)) => Some(msg.as_str()),
            _ => None,
        }
    }

    /// Apply the `matching` / `matching_regex` pre-filter to the candidate set.
    ///
    /// Returns the assets that pass the filter; when neither option is set this
    /// is the full list unchanged (so the no-matching path is byte-for-byte the
    /// previous behavior). The regex was compiled once when the picker was built,
    /// so this uses the cached result and never recompiles. An invalid pattern
    /// (`Some(Err)`) fails closed — it matches *nothing* rather than degrading to
    /// "no filter" — so a misconfiguration surfaces as "no asset found" instead
    /// of silently installing whatever plain autodetection would have picked. On
    /// the binary path that empty result is turned into a hard error upstream in
    /// [`AssetMatcher::match_by_auto_detection`].
    fn apply_matching_filter<'a>(&self, assets: &'a [String]) -> Vec<&'a String> {
        assets
            .iter()
            .filter(|asset| match &self.matching {
                Some(m) => asset.contains(m.as_str()),
                None => true,
            })
            .filter(|asset| match &self.matching_regex {
                Some(Ok(re)) => re.is_match(asset),
                // Invalid pattern: fail closed (matches nothing).
                Some(Err(_)) => false,
                None => true,
            })
            .collect()
    }

    /// Picks the best asset from available options.
    ///
    /// When multiple assets tie on score, prefers the shortest name. This handles
    /// the common case where a repo ships several binaries per platform (e.g.
    /// `tool-x64.tar.gz`, `tool-lsp-x64.tar.gz`, `tool-mcp-x64.tar.gz`) — the
    /// canonical binary's name is almost always the shortest.
    /// See: https://github.com/jdx/mise/discussions/9358
    pub fn pick_best_asset(&self, assets: &[String]) -> Option<String> {
        // Narrow by `matching`/`matching_regex` before scoring. When neither is
        // set, score the assets directly — no filtering, no intermediate clone —
        // so the no-matching path is allocation-identical to the pre-feature
        // behavior. Only when a filter is active do we materialize the narrowed
        // candidate set.
        let scored_assets = if self.matching.is_none() && self.matching_regex.is_none() {
            self.score_all_assets(assets)
        } else {
            let candidates: Vec<String> = self
                .apply_matching_filter(assets)
                .into_iter()
                .cloned()
                .collect();
            self.score_all_assets(&candidates)
        };
        scored_assets
            .into_iter()
            .filter(|(score, asset)| *score > 0 && !self.has_arch_mismatch(asset))
            .min_by(|(score_a, name_a), (score_b, name_b)| {
                score_b
                    .cmp(score_a)
                    .then_with(|| name_a.len().cmp(&name_b.len()))
                    .then_with(|| name_a.cmp(name_b))
            })
            .map(|(_, asset)| asset)
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

        // Narrow by `matching`/`matching_regex` so a multi-binary release's
        // per-binary provenance files don't cross-verify (e.g. attaching oxfmt's
        // provenance to an oxlint install). Mirrors the pre-filter the binary
        // picker applies, keeping the provenance aligned with the selected tool.
        //
        // When neither filter is set, score the provenance files directly — no
        // intermediate clone — mirroring the binary picker's no-op short-circuit
        // (`pick_best_asset`). `owned_provenance` is function-scoped so the narrowed
        // `candidates` can borrow it past the `if`.
        let owned_provenance: Vec<String>;
        let candidates: Vec<&String> = if self.matching.is_none() && self.matching_regex.is_none() {
            provenance_assets
        } else {
            // A malformed `matching_regex` is a different case from a valid filter
            // that excludes everything (handled by the fallback below). We can't
            // trust a garbage pattern to narrow anything, so refuse to pick rather
            // than fall back to the full set and risk attaching the wrong binary's
            // provenance. Production never reaches here with a bad pattern: the
            // autodetection path validates it up front and hard-errors first; the
            // `asset_pattern` path suppresses `matching` entirely for provenance
            // (`matching_for_provenance`); and the install path that reuses a cached
            // lockfile URL — which skips binary selection — validates it explicitly
            // via [`validate_matching_regex`] before any verification runs. So a bad
            // pattern is never threaded into this picker. This guard purely backstops
            // a future caller that builds a provenance picker without any of those
            // protections.
            if self.matching_regex_error().is_some() {
                return None;
            }

            // Fall back to the full provenance set when the filter excludes
            // everything: a single shared provenance file (e.g. goreleaser's
            // `multiple.intoto.jsonl`) attests every artifact in the release but
            // doesn't carry the binary name, so it would be filtered out. Dropping
            // it would silently skip verification — a downgrade — so we keep it
            // instead and let cryptographic verification decide.
            owned_provenance = provenance_assets.into_iter().cloned().collect();
            let filtered = self.apply_matching_filter(&owned_provenance);
            if filtered.is_empty() {
                owned_provenance.iter().collect()
            } else {
                filtered
            }
        };

        // Score by platform match only (no format/build penalties)
        let mut scored: Vec<(i32, &String)> = candidates
            .into_iter()
            .map(|asset| {
                let score = self.score_os_match(asset) + self.score_arch_match(asset);
                (score, asset)
            })
            .collect();

        scored.sort_by_key(|item| std::cmp::Reverse(item.0));
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
        score += self.score_preferred_name_match(asset);
        score += self.score_build_penalties(asset);
        score
    }

    /// Returns the part of the asset used for platform (OS/arch/libc) detection,
    /// with the tool's own name stripped from the front when it is known. This
    /// prevents OS/arch tokens that happen to appear in the tool name (e.g. the
    /// "arch" in "go-arch-lint", or the "win" in "win-tool") from being matched
    /// as the asset's platform. Falls back to the full asset when no preferred
    /// name is set or the asset does not start with it.
    fn platform_part<'a>(&self, asset: &'a str) -> &'a str {
        let Some(preferred_name) = self.preferred_name.as_deref() else {
            return asset;
        };
        let Some(prefix) = asset.get(..preferred_name.len()) else {
            return asset;
        };
        if !prefix.eq_ignore_ascii_case(preferred_name) {
            return asset;
        }
        let rest = &asset[preferred_name.len()..];
        // Only strip when the tool name is a complete leading token, i.e. it ends
        // at a separator or version boundary. Otherwise the prefix would cut
        // through a longer first token (e.g. preferred_name "win" inside
        // "windows-...") and corrupt detection. Mirrors the boundary handling in
        // asset_matches_preferred_name.
        match rest.chars().next() {
            None => rest,
            Some(c) if c == '-' || c == '_' || c == '.' || c.is_ascii_digit() => rest,
            _ => asset,
        }
    }

    fn score_os_match(&self, asset: &str) -> i32 {
        let asset = self.platform_part(asset);
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
        let asset = self.platform_part(asset);
        for (arch, pattern) in ARCH_PATTERNS.iter() {
            if pattern.is_match(asset) {
                return if arch.matches_target(&self.target_arch) {
                    50
                } else if *arch == AssetArch::X86
                    && AssetArch::X64.matches_target(&self.target_arch)
                {
                    // Some projects use "x86" for their x86-64 artifacts. Keep
                    // this below a real x64/amd64 match so correctly named
                    // assets win when both are present.
                    5
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

    fn has_arch_mismatch(&self, asset: &str) -> bool {
        self.score_arch_match(asset) < 0
    }

    fn score_libc_match(&self, asset: &str) -> i32 {
        let asset = self.platform_part(asset);
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
        let format = ExtractionFormat::from_file_name(asset);

        if format == ExtractionFormat::Zip {
            if self.target_os == "windows" {
                return 15;
            } else {
                return 5;
            }
        }

        if format.is_archive() {
            return 10;
        }

        // Platform-agnostic runtime archives (composer.phar, foo.jar, bar.pyz)
        // run on the language runtime, not the OS — give them the same score as
        // a regular archive so single-asset releases like composer's
        // `composer.phar` are picked instead of failing platform matching.
        // See: https://github.com/jdx/mise/discussions/9936
        //
        // `.whl` and `.gem` are intentionally NOT in this list: both have
        // platform-tagged variants whose tokens OS_PATTERNS doesn't reliably
        // catch (`manylinux2014_x86_64`, `mingw32`), so granting the bonus
        // could let a wrong-platform variant be picked. Those cases should
        // use an explicit `asset_pattern`.
        let lower = asset.to_lowercase();
        if lower.ends_with(".phar") || lower.ends_with(".jar") || lower.ends_with(".pyz") {
            return 10;
        }

        0
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
            || asset.ends_with(".cert")
            || asset.ends_with(".cer")
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

    fn score_preferred_name_match(&self, asset: &str) -> i32 {
        const PREFERRED_NAME_BONUS: i32 = 20;

        match &self.preferred_name {
            Some(preferred_name) if asset_matches_preferred_name(asset, preferred_name) => {
                PREFERRED_NAME_BONUS
            }
            _ => 0,
        }
    }
}

fn asset_matches_preferred_name(asset: &str, preferred_name: &str) -> bool {
    let asset = asset_name_stem(asset);
    let preferred_name = preferred_name
        .rsplit('/')
        .next()
        .unwrap_or(preferred_name)
        .to_lowercase();

    if asset == preferred_name {
        return true;
    }

    let Some(rest) = asset.strip_prefix(&preferred_name) else {
        return false;
    };

    if !rest.starts_with(['-', '_', '.']) {
        return false;
    }

    rest[1..]
        .split(['-', '_', '.'])
        .all(is_platform_or_version_token)
}

fn asset_name_stem(asset: &str) -> String {
    let mut name = asset.rsplit('/').next().unwrap_or(asset).to_lowercase();
    let suffixes = [
        ".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tgz", ".tar", ".zip", ".gz", ".xz", ".bz2",
        ".zst", ".phar", ".jar", ".pyz", ".exe", ".msi",
    ];

    if let Some(suffix) = suffixes.iter().find(|suffix| name.ends_with(*suffix)) {
        name.truncate(name.len() - suffix.len());
    }

    name
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

/// Validate a `matching_regex` option string, returning a hard error that names
/// the pattern if it fails to compile (an empty/`None` value is a no-op).
///
/// Binary selection already surfaces an invalid pattern via
/// [`AssetMatcher::match_by_auto_detection`], but the github backend's install
/// path can reuse a cached lockfile URL and skip binary selection entirely
/// (`install_version_`). That branch must still reject a bad pattern up front —
/// otherwise the invalid regex reaches [`AssetPicker::pick_best_provenance`],
/// which returns `None` and is read downstream as "no provenance", silently
/// skipping SLSA verification. This reuses the picker's cached-compile and error
/// message so every path decides "is the pattern valid?" identically.
pub fn validate_matching_regex(matching_regex: Option<&str>) -> Result<()> {
    let picker = AssetPicker::with_libc(String::new(), String::new(), None)
        .with_matching_regex(matching_regex.unwrap_or_default());
    if let Some(msg) = picker.matching_regex_error() {
        return Err(eyre::eyre!("{msg}"));
    }
    Ok(())
}

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
    /// Preferred primary executable/tool name for asset selection
    preferred_name: Option<String>,
    /// Substring an asset name must contain (ubi's `matching`)
    matching: Option<String>,
    /// Regex an asset name must match (ubi's `matching_regex`)
    matching_regex: Option<String>,
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

    /// Prefer assets whose platform-stripped name matches the primary tool.
    pub fn with_preferred_name(mut self, preferred_name: impl Into<String>) -> Self {
        let preferred_name = preferred_name.into();
        if !preferred_name.is_empty() {
            self.preferred_name = Some(preferred_name);
        }
        self
    }

    /// Narrow candidates to assets whose name contains `matching` before
    /// platform autodetection (ubi's `matching`). Empty is a no-op. Mirrors
    /// [`Self::with_preferred_name`]'s signature so the optional string fields
    /// are configured the same way.
    pub fn with_matching(mut self, matching: impl Into<String>) -> Self {
        let matching = matching.into();
        if !matching.is_empty() {
            self.matching = Some(matching);
        }
        self
    }

    /// Narrow candidates to assets matching `matching_regex` before platform
    /// autodetection (ubi's `matching_regex`). Empty is a no-op.
    ///
    /// This stores the *unparsed* pattern by design: the compile-once cache lives
    /// on [`AssetPicker`] (built in [`Self::create_picker`]), so validity is a
    /// local property of the picker rather than of this builder.
    pub fn with_matching_regex(mut self, matching_regex: impl Into<String>) -> Self {
        let matching_regex = matching_regex.into();
        if !matching_regex.is_empty() {
            self.matching_regex = Some(matching_regex);
        }
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
                .with_no_app(self.no_app)
                .with_preferred_name(self.preferred_name.clone().unwrap_or_default())
                .with_matching(self.matching.clone().unwrap_or_default())
                .with_matching_regex(self.matching_regex.clone().unwrap_or_default()),
        )
    }

    fn match_by_auto_detection(&self, assets: &[String]) -> Result<MatchedAsset> {
        let picker = self
            .create_picker()
            .ok_or_else(|| eyre::eyre!("Target OS and arch must be set for auto-detection"))?;

        // Reject an invalid `matching_regex` as a hard error that names it, rather
        // than letting it silently drop to plain autodetection and install the
        // wrong asset. The picker compiled the pattern once when it was built and
        // cached the result, so this just surfaces that error. This is the single
        // Result-returning choke point all binary-asset selection funnels through.
        if let Some(msg) = picker.matching_regex_error() {
            return Err(eyre::eyre!("{msg}"));
        }

        let best = picker.pick_best_asset(assets).ok_or_else(|| {
            let os = self.target_os.as_deref().unwrap_or("unknown");
            let arch = self.target_arch.as_deref().unwrap_or("unknown");
            // When a matching filter is set, surface it — otherwise an empty
            // filter result reads as "no asset for this platform", hiding that
            // the user's own `matching`/`matching_regex` excluded everything.
            // Report every active filter so a user who set both isn't told only
            // half of what narrowed the candidate set.
            let mut active_filters = Vec::new();
            if let Some(m) = &self.matching {
                active_filters.push(format!("matching=\"{m}\""));
            }
            if let Some(re) = &self.matching_regex {
                active_filters.push(format!("matching_regex=\"{re}\""));
            }
            let filter_note = if active_filters.is_empty() {
                String::new()
            } else {
                format!("\nNote: filtered by {}", active_filters.join(", "))
            };
            eyre::eyre!(
                "No matching asset found for platform {}-{}{}\nAvailable assets:\n{}",
                os,
                arch,
                filter_note,
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
pub(crate) fn detect_checksum_algorithm(filename: &str) -> String {
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

    #[test]
    fn test_asset_picker_riscv64_not_shadowed_by_s390x() {
        // Regression: uv/prek ship riscv64gc, s390x, and powerpc64 linux assets.
        // s390x/powerpc64 match no arch regex, so before riscv64 was known they tied
        // with the riscv asset at the same score and the shortest-name tiebreak
        // picked the s390x asset on a riscv64 host. riscv64 must win on riscv64 host.
        let assets = vec![
            "uv-aarch64-unknown-linux-gnu.tar.gz".to_string(),
            "uv-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "uv-s390x-unknown-linux-gnu.tar.gz".to_string(),
            "uv-powerpc64-unknown-linux-gnu.tar.gz".to_string(),
            "uv-powerpc64le-unknown-linux-gnu.tar.gz".to_string(),
            "uv-riscv64gc-unknown-linux-gnu.tar.gz".to_string(),
            "uv-riscv64gc-unknown-linux-musl.tar.gz".to_string(),
        ];
        let picked = AssetPicker::with_libc("linux".to_string(), "riscv64".to_string(), None)
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "uv-riscv64gc-unknown-linux-gnu.tar.gz");
    }

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
    fn test_asset_picker_tool_name_with_distro_alias() {
        // Regression for #10208: a tool whose name contains a Linux distro alias
        // (e.g. "go-arch-lint" -> "arch") must not have every asset classified as
        // Linux. The tool name is stripped before platform detection, so the alias
        // in the name does not shadow each asset's real OS token.
        let assets = vec![
            "go-arch-lint_1.15.0_darwin_amd64.tar.gz".to_string(),
            "go-arch-lint_1.15.0_darwin_arm64.tar.gz".to_string(),
            "go-arch-lint_1.15.0_linux_amd64.tar.gz".to_string(),
            "go-arch-lint_1.15.0_linux_arm64.tar.gz".to_string(),
            "go-arch-lint_1.15.0_windows_amd64.zip".to_string(),
        ];

        let picked = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_preferred_name("go-arch-lint")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "go-arch-lint_1.15.0_darwin_arm64.tar.gz");

        let picked = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("go-arch-lint")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "go-arch-lint_1.15.0_windows_amd64.zip");

        let picked = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("go-arch-lint")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "go-arch-lint_1.15.0_linux_amd64.tar.gz");
    }

    #[test]
    fn test_asset_picker_tool_name_with_os_token_keeps_distro_asset() {
        // Reverse guard: a tool whose name contains an OS token (e.g. "win" in
        // "win-tool") must not cause its Linux distro-alias asset to be detected as
        // that OS. Stripping the tool name before platform detection avoids moving
        // the #10208 problem onto another name/alias combination.
        let assets = vec![
            "win-tool-ubuntu-x64.tar.gz".to_string(),
            "win-tool-macos-x64.tar.gz".to_string(),
            "win-tool-windows-x64.zip".to_string(),
        ];

        let picked = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("win-tool")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "win-tool-ubuntu-x64.tar.gz");

        let picked = AssetPicker::with_libc("macos".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("win-tool")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "win-tool-macos-x64.tar.gz");

        // Complete the round-trip: the Windows target still resolves its asset.
        let picked = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("win-tool")
            .pick_best_asset(&assets)
            .unwrap();
        assert_eq!(picked, "win-tool-windows-x64.zip");
    }

    #[test]
    fn test_platform_part_only_strips_at_token_boundary() {
        // A tool name that is a prefix of an OS token (e.g. "win" inside "windows")
        // must NOT be stripped mid-token, or the OS evidence is lost. With the
        // boundary guard the full token is kept and the OS still scores as a match
        // (without it, "windows-x64.zip" would be cut to "dows-x64.zip" -> OS 0).
        let picker = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("win");
        assert_eq!(picker.score_os_match("windows-x64.zip"), 100);

        // The boundary cases that should still strip: separators, a version digit,
        // and end-of-string.
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("tool");
        assert_eq!(
            picker.platform_part("tool-linux-x64.tar.gz"),
            "-linux-x64.tar.gz"
        );
        assert_eq!(
            picker.platform_part("tool_linux_x64.tar.gz"),
            "_linux_x64.tar.gz"
        );
        assert_eq!(picker.platform_part("tool.linux.x64"), ".linux.x64");
        assert_eq!(picker.platform_part("tool1.2.3-linux"), "1.2.3-linux");
        assert_eq!(picker.platform_part("tool"), "");
        // No boundary -> not stripped.
        assert_eq!(
            picker.platform_part("toolkit-linux-x64"),
            "toolkit-linux-x64"
        );
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
    fn test_shortest_name_tiebreak_picks_canonical_binary() {
        // Repos like agent-sh/agnix ship multiple binaries per platform
        // (agnix, agnix-lsp, agnix-mcp). All score identically — the tiebreak
        // should prefer the shortest name, which is the canonical tool.
        // See: https://github.com/jdx/mise/discussions/9358
        let assets = vec![
            "agnix-lsp-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "agnix-mcp-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "agnix-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];

        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "agnix-x86_64-unknown-linux-gnu.tar.gz");

        // Should be order-independent: shuffle and confirm the same winner.
        let assets_reordered = vec![
            "agnix-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "agnix-lsp-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "agnix-mcp-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let picked = picker.pick_best_asset(&assets_reordered).unwrap();
        assert_eq!(picked, "agnix-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn test_shortest_name_tiebreak_picks_plain_bun() {
        // bun ships baseline/profile variants alongside the canonical build.
        // All tar.gz, all matching platform — shortest should win.
        let assets = vec![
            "bun-linux-x64-baseline-profile.zip".to_string(),
            "bun-linux-x64-baseline.zip".to_string(),
            "bun-linux-x64-profile.zip".to_string(),
            "bun-linux-x64.zip".to_string(),
        ];

        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "bun-linux-x64.zip");
    }

    #[test]
    fn test_preferred_name_picks_primary_binary_over_related_archive() {
        // opengrep ships both a primary CLI binary and an opengrep-core archive
        // for the same platform. Prefer the asset whose platform-stripped name
        // matches the repo/tool name.
        let assets = vec![
            "opengrep-core_osx_aarch64.tar.gz".to_string(),
            "opengrep_osx_arm64".to_string(),
        ];

        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_preferred_name("opengrep");
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "opengrep_osx_arm64");
    }

    /// Multi-binary release set used by the `matching` tests below.
    ///
    /// `oxc-project/oxc` ships both `oxlint` and `oxfmt` as separate per-platform
    /// assets in a single release. Neither is named after the repo (`oxc`).
    fn oxc_assets() -> Vec<String> {
        vec![
            "oxlint-aarch64-apple-darwin.tar.gz".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
            "oxlint-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "oxfmt-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "oxlint-i686-pc-windows-msvc.zip".to_string(),
            "oxfmt-i686-pc-windows-msvc.zip".to_string(),
        ]
    }

    #[test]
    fn test_multi_binary_release_without_matching_is_ambiguous() {
        // Demonstrates the gap that `matching` closes, using ONLY existing APIs
        // (runs against current `main` with no new production code), and guards
        // the unchanged no-matching path — `matching` must stay purely additive.
        //
        // `oxc-project/oxc` ships `oxlint` and `oxfmt` as separate per-platform
        // assets. Neither is named after the repo, so no existing signal can
        // portably select `oxlint`:
        let assets = vec![
            "oxlint-aarch64-apple-darwin.tar.gz".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
        ];

        // 1. Plain autodetection falls back to the #9358 shortest-name tiebreak
        //    and picks `oxfmt` (5 chars) over `oxlint` (6) — the wrong binary.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None);
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.tar.gz"
        );

        // 2. The #10008 repo-name preference can't rescue it either: the github
        //    backend passes preferred_name = the repo's last path segment
        //    (`oxc`), but neither asset starts with `oxc`, so there is no boost
        //    and `oxfmt` still wins. This is exactly the missing signal that
        //    `matching` supplies (see the `matching` tests below).
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_preferred_name("oxc");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_matching_narrows_multi_binary_release_to_named_binary() {
        // `matching=oxlint` supplies the signal autodetection lacks, while
        // keeping platform autodetection (ubi's `matching`, ported to github).
        let assets = oxc_assets();

        // macOS arm64 -> the darwin oxlint asset.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-aarch64-apple-darwin.tar.gz"
        );

        // The SAME config is portable: linux x64 -> the linux oxlint asset.
        // (`asset_pattern` can't do this — it discards platform autodetection.)
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_matching_selects_the_other_binary_from_the_same_release() {
        // Complements the oxlint test above: the SAME oxc release also ships
        // oxfmt, and `matching=oxfmt` selects it independently. This is what lets
        // a `tool_alias` config install both oxlint and oxfmt from one repo, each
        // picked portably (see e2e/backend/test_github_tool_alias_matching).
        let assets = oxc_assets();

        // macOS arm64 -> the darwin oxfmt asset.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxfmt");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.tar.gz"
        );

        // Portable across platforms: linux x64 -> the linux oxfmt asset.
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("oxfmt");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxfmt-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_matching_regex_narrows_multi_binary_release() {
        let assets = oxc_assets();
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching_regex("^oxlint-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_matching_still_respects_platform_autodetection() {
        // `matching` NARROWS — it does not override platform autodetection the
        // way `asset_pattern` does. With `matching=oxlint` on a macOS target but
        // only a *windows* oxlint asset surviving the filter, the result is
        // None (no asset for this OS/arch) — NOT the wrong-OS asset.
        let assets = vec![
            "oxlint-i686-pc-windows-msvc.zip".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
        ];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    #[test]
    fn test_matching_filtering_out_all_assets_returns_none() {
        // If `matching` excludes every asset there is nothing to install;
        // callers turn this None into an error naming the matching filter.
        let assets = vec!["oxfmt-aarch64-apple-darwin.tar.gz".to_string()];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    #[test]
    fn test_asset_matcher_with_matching_threads_through_to_picker() {
        // Covers the high-level builder path the github backend actually uses:
        // AssetMatcher::new().for_target(..).with_matching(..).pick_from(..).
        use crate::platform::Platform;

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let assets = oxc_assets();

        // matching threads through AssetMatcher -> AssetPicker; the linux oxlint
        // asset is chosen (autodetection still picks the OS/arch).
        let picked = AssetMatcher::new()
            .for_target(&target)
            .with_matching("oxlint")
            .pick_from(&assets)
            .unwrap()
            .name;
        assert_eq!(picked, "oxlint-x86_64-unknown-linux-gnu.tar.gz");

        // Empty matching is a no-op (the github backend passes
        // opts.matching().unwrap_or_default(), so an unset option arrives here
        // as ""); the same set is ambiguous and the shortest-name tiebreak picks
        // oxfmt, proving the no-matching path is unchanged.
        let picked = AssetMatcher::new()
            .for_target(&target)
            .with_matching("")
            .pick_from(&assets)
            .unwrap()
            .name;
        assert_eq!(picked, "oxfmt-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn test_asset_matcher_empty_matching_regex_is_noop() {
        // Twin of the empty-`matching` no-op above, for `matching_regex`. The
        // github backend passes opts.matching_regex().unwrap_or_default(), so an
        // unset option arrives as "" and must be a no-op (not a filter that
        // excludes everything). The set is then ambiguous and the shortest-name
        // tiebreak picks oxfmt — identical to the no-filter path.
        use crate::platform::Platform;

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let assets = oxc_assets();

        let picked = AssetMatcher::new()
            .for_target(&target)
            .with_matching_regex("")
            .pick_from(&assets)
            .unwrap()
            .name;
        assert_eq!(picked, "oxfmt-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn test_matching_does_not_fall_back_to_sibling_when_named_binary_missing_for_platform() {
        // The decisive safety property: when `matching` names a binary that is
        // NOT published for this platform, the result is None — it must NOT fall
        // back to a *sibling* binary that IS published here. Here oxlint ships for
        // linux and windows but not macOS, while oxfmt ships for macOS; a macOS
        // target with matching=oxlint must yield None, never the macOS oxfmt.
        let assets = vec![
            "oxlint-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "oxlint-i686-pc-windows-msvc.zip".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
        ];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    #[test]
    fn test_matching_on_windows_target() {
        // The matching tests above target macOS/linux; cover a Windows target too
        // (matching is platform-string-driven, so this guards the windows arm).
        // The oxc fixture ships i686-pc-windows-msvc assets for both binaries;
        // matching=oxlint selects the windows oxlint asset, not oxfmt.
        let assets = oxc_assets();
        let picker = AssetPicker::with_libc("windows".to_string(), "x86".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-i686-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn test_invalid_matching_regex_is_a_hard_error() {
        // A syntactically invalid `matching_regex` must be a HARD ERROR that
        // names the bad pattern — not silently ignored. Silently ignoring it
        // would fall back to plain autodetection and install the WRONG binary
        // (here: oxfmt instead of the intended oxlint) with no signal to the
        // user. This matches ubi, which rejects an invalid pattern up front.
        use crate::platform::Platform;

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let assets = oxc_assets();

        // `oxlint(` is invalid (unclosed group). Same bad pattern the e2e uses.
        let err = AssetMatcher::new()
            .for_target(&target)
            .with_matching_regex("oxlint(")
            .pick_from(&assets)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("matching_regex") && msg.contains("oxlint("),
            "error must name the option and the bad pattern, got: {msg}"
        );
    }

    #[test]
    fn test_validate_matching_regex_rejects_bad_pattern_without_a_picker() {
        // The github install path can reuse a cached lockfile URL and skip binary
        // selection — the path that normally hard-errors on a bad pattern. That
        // branch instead calls `validate_matching_regex` up front so an invalid
        // pattern still fails closed (rather than reaching the provenance picker,
        // returning `None`, and silently skipping SLSA verification). This guards
        // that the standalone validator names the option + the bad pattern, and
        // that valid/empty/None patterns are a no-op.
        let err = validate_matching_regex(Some("oxlint(")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("matching_regex") && msg.contains("oxlint("),
            "error must name the option and the bad pattern, got: {msg}"
        );

        assert!(validate_matching_regex(Some("^oxlint")).is_ok());
        assert!(validate_matching_regex(Some("")).is_ok());
        assert!(validate_matching_regex(None).is_ok());
    }

    #[test]
    fn test_matching_is_a_literal_substring_not_a_regex() {
        // `matching` is a plain substring test (str::contains), so regex
        // metacharacters in the value are LITERAL. `matching="a.c"` selects only
        // the asset whose name literally contains "a.c"; the `.` is a dot, not a
        // wildcard. The decoy "abc-..." matches `a.c` *as a regex* and is the
        // shorter name (so the shortest-name tiebreak would prefer it), so if
        // `matching` were ever treated as a regex this assertion would pick the
        // wrong asset. Use `matching_regex` when you want pattern semantics.
        let assets = vec![
            "mytool-a.c-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "abc-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("a.c");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "mytool-a.c-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_matching_and_matching_regex_combine_as_and() {
        // matching and matching_regex set TOGETHER on the same picker are ANDed:
        // an asset must satisfy both to survive the pre-filter. This is the only
        // test that chains both on one picker — the other multi-filter tests use
        // separate pickers, so they'd still pass if the two filters were ever
        // accidentally ORed in apply_matching_filter.
        let assets = oxc_assets();

        // matching="ox" admits both oxlint and oxfmt; the regex narrows to
        // oxlint. The survivor is the intersection: the darwin oxlint asset.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("ox")
            .with_matching_regex("^oxlint-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-aarch64-apple-darwin.tar.gz"
        );

        // Contradictory filters (substring wants oxfmt, regex wants oxlint)
        // intersect to nothing -> None, not a fall-back to either filter alone.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxfmt")
            .with_matching_regex("^oxlint-");
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    #[test]
    fn test_matching_substring_leaks_into_longer_sibling_name() {
        // `matching` uses substring `contains`, so a value that is a prefix of
        // another binary's name admits BOTH — it does not uniquely select. This
        // documents that footgun and shows the `matching_regex` escape hatch.
        let assets = vec![
            "tool-a-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "tool-ab-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        ];

        // "tool-a" is a substring of BOTH names, so both survive the pre-filter
        // and the shortest-name tiebreak decides. A user who actually wanted the
        // longer-named sibling would silently get the wrong one.
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("tool-a");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "tool-a-x86_64-unknown-linux-gnu.tar.gz"
        );

        // An anchored `matching_regex` disambiguates: only tool-ab matches.
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching_regex("^tool-ab-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "tool-ab-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_direct_picker_invalid_regex_fails_closed() {
        // The picker caches the compiled matching_regex. A direct AssetPicker
        // built with a bad pattern must fail CLOSED: an invalid regex matches
        // nothing (-> None), never degrading to "no filter" and silently
        // installing the autodetected asset. (The AssetMatcher path turns this
        // into the hard error covered by test_invalid_matching_regex_is_a_hard_error;
        // the provenance path returns None via
        // test_pick_best_provenance_invalid_regex_returns_none_not_fallback.)
        let assets = oxc_assets();
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching_regex("oxlint(");
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    /// Real release set for bazelbuild/buildtools v7.1.2 — three bare binaries
    /// per platform. This is the case the ubi backend covers via e2e
    /// (`e2e/cli/test_upgrade`: `ubi:bazelbuild/buildtools[matching=buildifier]`);
    /// ported here so the github backend has the same coverage at the unit level.
    fn bazel_buildtools_assets() -> Vec<String> {
        vec![
            "buildifier-darwin-amd64".to_string(),
            "buildifier-darwin-arm64".to_string(),
            "buildifier-linux-amd64".to_string(),
            "buildifier-linux-arm64".to_string(),
            "buildifier-windows-amd64.exe".to_string(),
            "buildozer-darwin-amd64".to_string(),
            "buildozer-darwin-arm64".to_string(),
            "buildozer-linux-amd64".to_string(),
            "buildozer-linux-arm64".to_string(),
            "buildozer-windows-amd64.exe".to_string(),
            "unused_deps-darwin-amd64".to_string(),
            "unused_deps-darwin-arm64".to_string(),
            "unused_deps-linux-amd64".to_string(),
            "unused_deps-linux-arm64".to_string(),
            "unused_deps-windows-amd64.exe".to_string(),
        ]
    }

    #[test]
    fn test_matching_selects_buildifier_from_bazel_buildtools() {
        // Mirrors the ubi e2e: `matching=buildifier` selects buildifier from a
        // multi-binary release, while platform autodetection still chooses the
        // correct OS/arch — so one config is portable across platforms.
        let assets = bazel_buildtools_assets();

        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("buildifier");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "buildifier-darwin-arm64"
        );

        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("buildifier");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "buildifier-linux-amd64"
        );

        // matching_regex works the same way.
        let picker = AssetPicker::with_libc("linux".to_string(), "aarch64".to_string(), None)
            .with_matching_regex("^buildifier-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "buildifier-linux-arm64"
        );
    }

    #[test]
    fn test_bazel_buildtools_without_matching_picks_shortest_not_buildifier() {
        // Documents why `matching` is needed for this repo: with three binaries
        // per platform and none named after the repo (`buildtools`), the #9358
        // shortest-name tiebreak picks `buildozer` (shorter than `buildifier`),
        // so a user wanting buildifier has no portable signal without `matching`.
        let assets = bazel_buildtools_assets();
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "buildozer-linux-amd64"
        );
    }

    /// Real release set for grpc-ecosystem/grpc-gateway v2.27.3 — two binaries
    /// per platform that SHARE the `protoc-gen-` prefix. This is the shape behind
    /// the wrong-artifact bug ubi hit (ubi #137 / mise discussion #6611), where
    /// `--matching protoc-gen-openapiv2` selected the wrong binary because ubi
    /// applied `matching` *after* arch filtering. Ported here as a regression
    /// guard for the github backend's pre-filter ordering.
    fn grpc_gateway_assets() -> Vec<String> {
        vec![
            "protoc-gen-grpc-gateway-v2.27.3-darwin-arm64".to_string(),
            "protoc-gen-grpc-gateway-v2.27.3-darwin-x86_64".to_string(),
            "protoc-gen-grpc-gateway-v2.27.3-linux-arm64".to_string(),
            "protoc-gen-grpc-gateway-v2.27.3-linux-x86_64".to_string(),
            "protoc-gen-grpc-gateway-v2.27.3-windows-x86_64.exe".to_string(),
            "protoc-gen-openapiv2-v2.27.3-darwin-arm64".to_string(),
            "protoc-gen-openapiv2-v2.27.3-darwin-x86_64".to_string(),
            "protoc-gen-openapiv2-v2.27.3-linux-arm64".to_string(),
            "protoc-gen-openapiv2-v2.27.3-linux-x86_64".to_string(),
            "protoc-gen-openapiv2-v2.27.3-windows-x86_64.exe".to_string(),
        ]
    }

    #[test]
    fn test_matching_overrides_shortest_name_tiebreak_for_shared_prefix() {
        // Regression for the wrong-artifact class of bug (ubi #137 / mise #6611).
        // grpc-gateway ships protoc-gen-grpc-gateway and protoc-gen-openapiv2,
        // sharing the `protoc-gen-` prefix. The decisive case: `matching` must be
        // able to select protoc-gen-grpc-gateway — the LONGER name, which the
        // #9358 shortest-name tiebreak would never pick on its own. This proves
        // the pre-filter genuinely overrides autodetection's tiebreak rather than
        // coinciding with it (the distinct-prefix oxc/bazel fixtures can't show
        // this, since there the wanted binary is also the shorter one).
        let assets = grpc_gateway_assets();

        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("protoc-gen-grpc-gateway");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "protoc-gen-grpc-gateway-v2.27.3-darwin-arm64"
        );

        // The same config selects protoc-gen-openapiv2 portably across platforms.
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("protoc-gen-openapiv2");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "protoc-gen-openapiv2-v2.27.3-linux-x86_64"
        );

        // `contains` is substring, but the prefix is shared safely: the openapiv2
        // matching string does NOT appear in the grpc-gateway asset name, so the
        // filter is unambiguous despite the common `protoc-gen-` prefix.
        let picker = AssetPicker::with_libc("macos".to_string(), "x86_64".to_string(), None)
            .with_matching_regex("^protoc-gen-grpc-gateway-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "protoc-gen-grpc-gateway-v2.27.3-darwin-x86_64"
        );
    }

    #[test]
    fn test_grpc_gateway_without_matching_falls_to_tiebreak() {
        // Documents why `matching` is required for this repo: without it, both
        // binaries score equally for the platform and the shortest-name tiebreak
        // decides — so a user wanting the longer-named protoc-gen-grpc-gateway has
        // no portable signal. (ubi picked grpc-gateway here via a different
        // tiebreak; the point is identical — without `matching` the choice isn't
        // the user's to make.)
        let assets = grpc_gateway_assets();
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None);
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "protoc-gen-openapiv2-v2.27.3-darwin-arm64"
        );
    }

    #[test]
    fn test_matching_is_case_sensitive_with_regex_escape_hatch() {
        // Characterization for ubi #83 (open: "match executable names
        // case-insensitively"). The github backend's `matching` is case-SENSITIVE
        // (ubi parity — it uses substring `contains`). Lock that in, and document
        // that `matching_regex` with the `(?i)` inline flag is the escape hatch
        // for users who need case-insensitive selection.
        let assets = vec![
            "OxLint-aarch64-apple-darwin.tar.gz".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
        ];

        // Wrong case excludes the intended asset -> None (case-sensitive).
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(picker.pick_best_asset(&assets), None);

        // `(?i)` makes the regex case-insensitive and selects it.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching_regex("(?i)^oxlint-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "OxLint-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_matching_and_case_insensitive_regex_each_apply_independently() {
        // When both options are set they AND, and each keeps its own case rule:
        // `matching` stays case-SENSITIVE even when `matching_regex` opts into
        // case-insensitivity via `(?i)`. So a case-insensitive regex does NOT
        // loosen the substring test — an asset must satisfy both as written.
        let assets = vec![
            "OxLint-aarch64-apple-darwin.tar.gz".to_string(),
            "oxlint-aarch64-apple-darwin.tar.gz".to_string(),
        ];

        // `(?i)^oxlint-` matches both casings, but case-sensitive `matching=oxlint`
        // still excludes the capitalized one -> only the lowercase asset survives.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint")
            .with_matching_regex("(?i)^oxlint-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "oxlint-aarch64-apple-darwin.tar.gz"
        );

        // Flip the `matching` case: case-sensitive `matching=OxLint` selects the
        // capitalized asset even though the regex matches both, proving the
        // substring test keeps its own (sensitive) case rule inside the AND.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("OxLint")
            .with_matching_regex("(?i)^oxlint-");
        assert_eq!(
            picker.pick_best_asset(&assets).unwrap(),
            "OxLint-aarch64-apple-darwin.tar.gz"
        );
    }

    #[test]
    fn test_manylinux_and_musllinux_assets_are_linux_with_libc() {
        let assets = vec![
            "opengrep-core_linux_aarch64.tar.gz".to_string(),
            "opengrep_manylinux_aarch64".to_string(),
            "opengrep_musllinux_aarch64".to_string(),
        ];

        let picker = AssetPicker::with_libc("linux".to_string(), "aarch64".to_string(), None)
            .with_preferred_name("opengrep");
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "opengrep_manylinux_aarch64");

        let picker = AssetPicker::with_libc(
            "linux".to_string(),
            "aarch64".to_string(),
            Some("musl".to_string()),
        )
        .with_preferred_name("opengrep");
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "opengrep_musllinux_aarch64");
    }

    #[test]
    fn test_x86_asset_is_x64_fallback() {
        let assets = vec![
            "opengrep-core_linux_x86.tar.gz".to_string(),
            "opengrep_manylinux_x86".to_string(),
            "opengrep_manylinux_x86.sig".to_string(),
            "opengrep_musllinux_x86".to_string(),
        ];

        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("opengrep");
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "opengrep_manylinux_x86");

        let exact_assets = vec![
            "opengrep_manylinux_x86".to_string(),
            "opengrep_manylinux_x86_64".to_string(),
        ];
        let picked = picker.pick_best_asset(&exact_assets).unwrap();
        assert_eq!(picked, "opengrep_manylinux_x86_64");

        let arm_picker = AssetPicker::with_libc("linux".to_string(), "aarch64".to_string(), None);
        assert_eq!(arm_picker.pick_best_asset(&exact_assets), None);
    }

    #[test]
    fn test_preferred_name_handles_tar_and_split_platform_tokens() {
        let assets = vec!["tool-mingw-w64-x86_64.tar".to_string()];

        let picker = AssetPicker::with_libc("windows".to_string(), "x86_64".to_string(), None)
            .with_preferred_name("tool");
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-mingw-w64-x86_64.tar");
    }

    #[test]
    fn test_platform_agnostic_phar_picked() {
        // composer ships a single platform-agnostic `composer.phar` (plus a
        // signature). `.phar` is a PHP runtime archive — it runs on any OS
        // PHP supports, so we should pick it without requiring platform tokens.
        // See: https://github.com/jdx/mise/discussions/9936
        let assets = vec!["composer.phar".to_string(), "composer.phar.asc".to_string()];

        for os in ["linux", "macos", "windows"] {
            let picker = AssetPicker::with_libc(os.to_string(), "x86_64".to_string(), None);
            let picked = picker.pick_best_asset(&assets).unwrap();
            assert_eq!(picked, "composer.phar", "should pick .phar on {os}");
        }
    }

    #[test]
    fn test_platform_agnostic_jar_picked() {
        // JVM tools commonly ship a single platform-agnostic `.jar`.
        let assets = vec!["tool.jar".to_string(), "tool.jar.sha256".to_string()];
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool.jar");
    }

    #[test]
    fn test_platform_tagged_extensions_excluded_from_bonus() {
        // Regression guard: .whl and .gem are intentionally excluded from the
        // platform-agnostic format-score bonus that .phar/.jar/.pyz get.
        // Both have platform-tagged variants (`manylinux2014_x86_64.whl`,
        // `x86_64-mingw32.gem`) whose tokens OS_PATTERNS doesn't reliably
        // catch — granting the bonus would help wrong-platform variants
        // win against the right one.
        //
        // Concretely: `.whl` and `.gem` with no other tokens should score 0
        // (filtered out), while `.jar` with no other tokens scores 10.
        let picker = AssetPicker::with_libc("macos".to_string(), "x86_64".to_string(), None);
        assert!(picker.pick_best_asset(&["tool.whl".to_string()]).is_none());
        assert!(picker.pick_best_asset(&["tool.gem".to_string()]).is_none());
        assert_eq!(
            picker.pick_best_asset(&["tool.jar".to_string()]).as_deref(),
            Some("tool.jar")
        );
    }

    #[test]
    fn test_platform_specific_still_wins_over_phar() {
        // If a release ships both platform-specific binaries and a .phar,
        // platform-specific should still win (it scores higher: 100+50+10 vs 10).
        let assets = vec![
            "tool.phar".to_string(),
            "tool-linux-x86_64.tar.gz".to_string(),
            "tool-darwin-x86_64.tar.gz".to_string(),
        ];

        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-linux-x86_64.tar.gz");
    }

    #[test]
    fn test_exe_on_non_windows_not_picked() {
        // Regression guard: a Windows .exe with no other platform tokens
        // should NOT be auto-picked on Linux just because it's the only
        // non-metadata asset. This is the failure mode that scuttled
        // https://github.com/jdx/mise/pull/8756 — preserve it.
        let assets = vec!["foo.exe".to_string()];
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        assert!(picker.pick_best_asset(&assets).is_none());
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

        // Compound qualifier still carries the libc preference.
        let platform = Platform::parse("linux-x64-musl-baseline").unwrap();
        let target = PlatformTarget::new(platform);
        let result = AssetMatcher::new()
            .for_target(&target)
            .pick_from(&assets)
            .unwrap();
        assert_eq!(result.name, "tool-1.0.0-linux-x86_64-musl.tar.gz");
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
    fn test_arch_mismatch_rejected_after_positive_bonuses() {
        let picker = AssetPicker::with_libc("linux".to_string(), "aarch64".to_string(), None)
            .with_preferred_name("cargo-msrv");
        let assets = vec![
            "cargo-msrv-aarch64-apple-darwin-v0.19.3.tgz".to_string(),
            "cargo-msrv-x86_64-apple-darwin-v0.19.3.tgz".to_string(),
            "cargo-msrv-x86_64-pc-windows-msvc-v0.19.3.zip".to_string(),
            "cargo-msrv-x86_64-unknown-linux-gnu-v0.19.3.tgz".to_string(),
            "cargo-msrv-x86_64-unknown-linux-musl-v0.19.3.tgz".to_string(),
        ];

        let score = picker.score_asset("cargo-msrv-x86_64-unknown-linux-gnu-v0.19.3.tgz");
        assert!(
            score > 0,
            "regression setup should cover wrong-arch assets rescued by bonuses, got {score}"
        );
        assert_eq!(picker.pick_best_asset(&assets), None);
    }

    #[test]
    fn test_metadata_penalty() {
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None);
        let assets = vec![
            "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.asc".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.cert".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.cer".to_string(),
            "tool-1.0.0-linux-x86_64.tar.gz.sha256".to_string(),
            "release-notes.txt".to_string(),
        ];

        let picked = picker.pick_best_asset(&assets).unwrap();
        assert_eq!(picked, "tool-1.0.0-linux-x86_64.tar.gz");

        // Ensure penalties are applied
        let score_tar = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz");
        let score_asc = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.asc");
        let score_cert = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.cert");
        let score_cer = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.cer");
        let score_sha = picker.score_asset("tool-1.0.0-linux-x86_64.tar.gz.sha256");
        let score_txt = picker.score_asset("release-notes.txt");

        assert!(
            score_tar > score_asc,
            "Tarball should score higher than signature"
        );
        assert!(
            score_tar > score_cert,
            "Tarball should score higher than certificate"
        );
        assert!(
            score_tar > score_cer,
            "Tarball should score higher than certificate"
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
    fn test_pick_best_provenance_respects_matching() {
        // A multi-binary release that ships a SEPARATE provenance file per binary
        // per platform. Both darwin provenance files score identically on
        // platform, and pick_best_provenance breaks ties by stable input order
        // (no shortest-name tiebreak), so the FIRST one wins — here oxfmt. For an
        // oxlint install that attaches oxfmt's provenance, verifying the wrong
        // digest. `matching` must narrow provenance the same way it narrows the
        // binary so the provenance follows the selected tool.
        let assets = vec![
            // oxfmt deliberately first so the unfiltered pick is the WRONG one.
            "oxfmt-aarch64-apple-darwin.intoto.jsonl".to_string(),
            "oxlint-aarch64-apple-darwin.intoto.jsonl".to_string(),
        ];

        // Without matching: positional tiebreak picks oxfmt's provenance.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None);
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.intoto.jsonl"
        );

        // matching=oxlint selects oxlint's provenance despite oxfmt being first.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxlint-aarch64-apple-darwin.intoto.jsonl"
        );

        // matching=oxfmt selects oxfmt's, independently — proves it narrows to the
        // named binary rather than just preferring a fixed one.
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxfmt");
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.intoto.jsonl"
        );
    }

    #[test]
    fn test_pick_best_provenance_respects_matching_regex() {
        // Same as above but driven by matching_regex, since both options thread
        // into the picker and both must narrow provenance.
        let assets = vec![
            "oxfmt-x86_64-unknown-linux-gnu.intoto.jsonl".to_string(),
            "oxlint-x86_64-unknown-linux-gnu.intoto.jsonl".to_string(),
        ];
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching_regex("^oxlint-");
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxlint-x86_64-unknown-linux-gnu.intoto.jsonl"
        );
    }

    #[test]
    fn test_pick_best_provenance_matching_keeps_platform_autodetection() {
        // matching narrows to the binary; platform autodetection still chooses the
        // right OS/arch among that binary's per-platform provenance files. So a
        // portable `matching=oxlint` config picks the linux oxlint provenance on a
        // linux target — not oxlint's darwin provenance, and not oxfmt's anything.
        let assets = vec![
            "oxlint-aarch64-apple-darwin.intoto.jsonl".to_string(),
            "oxlint-x86_64-unknown-linux-gnu.intoto.jsonl".to_string(),
            "oxfmt-x86_64-unknown-linux-gnu.intoto.jsonl".to_string(),
        ];
        let picker = AssetPicker::with_libc("linux".to_string(), "x86_64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxlint-x86_64-unknown-linux-gnu.intoto.jsonl"
        );
    }

    #[test]
    fn test_pick_best_provenance_matching_falls_back_to_shared_file() {
        // goreleaser-style: ONE provenance file attests every artifact in the
        // release (its subject digest list covers oxlint too). Its name doesn't
        // contain the binary name, so the matching filter would exclude it — but
        // with no per-binary provenance to fall back to, dropping it would lose
        // verification entirely. The shared file must still be returned.
        let assets = vec![
            "oxlint-aarch64-apple-darwin.tar.gz".to_string(),
            "oxfmt-aarch64-apple-darwin.tar.gz".to_string(),
            "multiple.intoto.jsonl".to_string(),
        ];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("oxlint");
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "multiple.intoto.jsonl"
        );
    }

    #[test]
    fn test_pick_best_provenance_matching_excludes_all_real_provenance_falls_back() {
        // Per-binary provenance exists but matching excludes ALL of it (e.g. a
        // typo'd or over-narrow filter). Rather than report "no provenance" and
        // silently skip verification — a downgrade — fall back to the full
        // provenance set so verification still runs (and fails loudly if the
        // digest doesn't match), mirroring how the binary path errors rather than
        // silently degrading.
        let assets = vec![
            "oxfmt-aarch64-apple-darwin.intoto.jsonl".to_string(),
            "oxlint-aarch64-apple-darwin.intoto.jsonl".to_string(),
        ];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching("does-not-exist");
        // Falls back to platform scoring over all provenance (positional tiebreak).
        assert_eq!(
            picker.pick_best_provenance(&assets).unwrap(),
            "oxfmt-aarch64-apple-darwin.intoto.jsonl"
        );
    }

    #[test]
    fn test_pick_best_provenance_invalid_regex_returns_none_not_fallback() {
        // Defense-in-depth: an INVALID matching_regex must NOT fall back to the
        // full provenance set (which could attach the wrong binary's provenance).
        // This is deliberately DIFFERENT from a VALID but over-narrow filter (see
        // test_pick_best_provenance_matching_excludes_all_real_provenance_falls_back),
        // which DOES fall back so verification still runs and fails loudly. A
        // malformed pattern can't be trusted to narrow anything, so we refuse to
        // pick rather than guess at a provenance file.
        //
        // In production this is unreachable — binary selection validates the regex
        // up front and hard-errors first (test_invalid_matching_regex_is_a_hard_error)
        // — so this guards against a future refactor that reaches a provenance
        // picker without first resolving (and validating) the binary. The compiled
        // regex is cached on the picker, so validity is a local property of the
        // picker rather than something that depends on call ordering.
        let assets = vec![
            "oxfmt-aarch64-apple-darwin.intoto.jsonl".to_string(),
            "oxlint-aarch64-apple-darwin.intoto.jsonl".to_string(),
        ];
        let picker = AssetPicker::with_libc("macos".to_string(), "aarch64".to_string(), None)
            .with_matching_regex("oxlint("); // invalid: unclosed group
        assert_eq!(picker.pick_best_provenance(&assets), None);
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
