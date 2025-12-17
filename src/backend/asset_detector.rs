use eyre::Result;
use regex::Regex;
use std::sync::LazyLock;

use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;

/// Platform detection patterns
pub struct PlatformPatterns {
    pub os_patterns: &'static [(AssetOs, Regex)],
    pub arch_patterns: &'static [(AssetArch, Regex)],
    pub libc_patterns: &'static [(AssetLibc, Regex)],
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

static OS_PATTERNS: LazyLock<Vec<(AssetOs, Regex)>> = LazyLock::new(|| {
    vec![
        (
            AssetOs::Linux,
            // Include common Linux distro names that may appear in release asset names
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

pub static PLATFORM_PATTERNS: LazyLock<PlatformPatterns> = LazyLock::new(|| PlatformPatterns {
    os_patterns: &OS_PATTERNS,
    arch_patterns: &ARCH_PATTERNS,
    libc_patterns: &LIBC_PATTERNS,
    archive_extensions: ARCHIVE_EXTENSIONS,
});

/// Automatically detects the best asset for the current platform
pub struct AssetPicker {
    target_os: String,
    target_arch: String,
    target_libc: String,
}

impl AssetPicker {
    pub fn new(target_os: String, target_arch: String) -> Self {
        // Determine the libc variant based on target OS and how mise was built
        let target_libc = if target_os == "windows" {
            // On Windows, prefer MSVC over GNU
            "msvc".to_string()
        } else if cfg!(target_env = "musl") {
            "musl".to_string()
        } else {
            "gnu".to_string()
        };

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

        // Libc variant scoring (for Linux and Windows)
        if self.target_os == "linux" || self.target_os == "windows" {
            score += self.score_libc_match(asset);
        }

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
                    -100 // Wrong OS - penalty should be larger than arch bonus
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

    fn score_libc_match(&self, asset: &str) -> i32 {
        for (libc, pattern) in PLATFORM_PATTERNS.libc_patterns.iter() {
            if pattern.is_match(asset) {
                return if libc.matches_target(&self.target_libc) {
                    25 // Exact libc match
                } else {
                    -10 // Wrong libc
                };
            }
        }
        0 // No libc detected
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
        // Swift Package Manager artifact bundles are not suitable for direct installation
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

    // Try to detect OS
    for (os, pattern) in PLATFORM_PATTERNS.os_patterns.iter() {
        if pattern.is_match(&filename) {
            detected_os = Some(*os);
            break;
        }
    }

    // Try to detect architecture
    for (arch, pattern) in PLATFORM_PATTERNS.arch_patterns.iter() {
        if pattern.is_match(&filename) {
            detected_arch = Some(*arch);
            break;
        }
    }

    // Try to detect libc (for Linux and Windows)
    if detected_os == Some(AssetOs::Linux) || detected_os == Some(AssetOs::Windows) {
        for (libc, pattern) in PLATFORM_PATTERNS.libc_patterns.iter() {
            if pattern.is_match(&filename) {
                detected_libc = Some(*libc);
                break;
            }
        }
    }

    // Return detected platform if we have at least OS and architecture
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

/// Detects the best asset for a given target platform
/// Used for cross-platform lockfile generation
pub fn detect_asset_for_target(assets: &[String], target: &PlatformTarget) -> Result<String> {
    let target_os = match target.os_name() {
        "macos" => "darwin",
        other => other,
    };
    let target_arch = match target.arch_name() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    };

    let picker = AssetPicker::new(target_os.to_string(), target_arch.to_string());
    picker.pick_best_asset(assets).ok_or_else(|| {
        eyre::eyre!(
            "No matching asset found for platform {}-{}\nAvailable assets: {}",
            target_os,
            target_arch,
            assets.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_libc_variant_detection() {
        // Test ripgrep assets with libc variants
        let ripgrep_assets = vec![
            "ripgrep-14.1.1-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-unknown-linux-musl.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz".to_string(),
        ];

        // Test Linux x86_64 - should prefer the libc variant that matches the build environment
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        if cfg!(target_env = "musl") {
            assert_eq!(picked, "ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz");
        } else {
            assert_eq!(picked, "ripgrep-14.1.1-x86_64-unknown-linux-gnu.tar.gz");
        }

        // Test Linux aarch64 - should prefer the libc variant that matches the build environment
        let picker = AssetPicker::new("linux".to_string(), "aarch64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        if cfg!(target_env = "musl") {
            assert_eq!(picked, "ripgrep-14.1.1-aarch64-unknown-linux-musl.tar.gz");
        } else {
            assert_eq!(picked, "ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz");
        }

        // Test macOS (should not be affected by libc)
        let picker = AssetPicker::new("macos".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz");
    }

    #[test]
    fn test_libc_scoring() {
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());

        // Test that the libc variant matching the build environment scores higher
        let gnu_score = picker.score_asset("ripgrep-14.1.1-x86_64-unknown-linux-gnu.tar.gz");
        let musl_score = picker.score_asset("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz");

        if cfg!(target_env = "musl") {
            assert!(
                musl_score > gnu_score,
                "musl variant should score higher than gnu when built with musl"
            );
        } else {
            assert!(
                gnu_score > musl_score,
                "GNU variant should score higher than musl when built with gnu"
            );
        }

        // Test that non-linux assets score negatively (wrong OS penalty)
        let macos_score = picker.score_asset("ripgrep-14.1.1-x86_64-apple-darwin.tar.gz");
        assert!(
            macos_score < gnu_score,
            "macOS assets should score lower than Linux assets when using a Linux picker"
        );
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

        // Test Windows MSVC URL
        let url = "https://github.com/dathere/qsv/releases/download/8.1.1/qsv-8.1.1-x86_64-pc-windows-msvc.zip";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Windows);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.libc, Some(AssetLibc::Msvc));
        assert_eq!(platform.to_platform_string(), "windows-x64");

        // Test Windows GNU URL
        let url = "https://github.com/dathere/qsv/releases/download/8.1.1/qsv-8.1.1-x86_64-pc-windows-gnu.zip";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Windows);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.libc, Some(AssetLibc::Gnu));
        assert_eq!(platform.to_platform_string(), "windows-x64");

        // Test URL with query parameters
        let url = "https://releases.example.com/tool-linux-x64.tar.gz?token=abc123&version=1.0";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Linux);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.to_platform_string(), "linux-x64");

        // Test URL with fragment
        let url = "https://cdn.example.com/releases/tool-darwin-arm64.zip#main";
        let platform = detect_platform_from_url(url).unwrap();
        assert_eq!(platform.os, AssetOs::Macos);
        assert_eq!(platform.arch, AssetArch::Arm64);
        assert_eq!(platform.to_platform_string(), "macos-arm64");

        // Test URL without platform info
        let url = "https://example.com/generic-tool.tar.gz";
        let platform = detect_platform_from_url(url);
        assert!(platform.is_none());

        // Test malformed URL (should still work with fallback)
        let filename = "tool-windows-x86_64.exe";
        let platform = detect_platform_from_url(filename).unwrap();
        assert_eq!(platform.os, AssetOs::Windows);
        assert_eq!(platform.arch, AssetArch::X64);
        assert_eq!(platform.to_platform_string(), "windows-x64");
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
    fn test_ripgrep_real_assets() {
        // Real ripgrep assets from the example
        let ripgrep_assets = vec![
            "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz".to_string(),
            "ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-armv7-unknown-linux-gnueabihf.tar.gz".to_string(),
            "ripgrep-14.1.1-armv7-unknown-linux-musleabi.tar.gz".to_string(),
            "ripgrep-14.1.1-armv7-unknown-linux-musleabihf.tar.gz".to_string(),
            "ripgrep-14.1.1-i686-pc-windows-msvc.zip".to_string(),
            "ripgrep-14.1.1-i686-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-powerpc64-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-s390x-unknown-linux-gnu.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz".to_string(),
            "ripgrep-14.1.1-x86_64-pc-windows-gnu.zip".to_string(),
            "ripgrep-14.1.1-x86_64-pc-windows-msvc.zip".to_string(),
            "ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz".to_string(),
            "ripgrep_14.1.1-1_amd64.deb".to_string(),
        ];

        // Test Linux x86_64 - should prefer musl over other variants when only musl is available
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz");

        // Test Linux aarch64 - should prefer gnu over musl
        let picker = AssetPicker::new("linux".to_string(), "aarch64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-aarch64-unknown-linux-gnu.tar.gz");

        // Test macOS x86_64 - should not be affected by libc
        let picker = AssetPicker::new("macos".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-x86_64-apple-darwin.tar.gz");

        // Test macOS aarch64 - should not be affected by libc
        let picker = AssetPicker::new("macos".to_string(), "aarch64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-aarch64-apple-darwin.tar.gz");

        // Test Windows x86_64 - should prefer MSVC over GNU
        let picker = AssetPicker::new("windows".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&ripgrep_assets).unwrap();
        assert_eq!(picked, "ripgrep-14.1.1-x86_64-pc-windows-msvc.zip");
    }

    #[test]
    fn test_various_url_formats() {
        // Test different URL formats to ensure robustness
        let test_cases = vec![
            (
                "https://releases.example.com/tool-v1.0.0-linux-amd64.tar.gz",
                "linux-x64",
            ),
            (
                "https://github.com/owner/repo/releases/download/v1.0.0/tool_darwin_arm64.zip",
                "macos-arm64",
            ),
            (
                "https://example.com/downloads/tool-windows-x86_64.exe",
                "windows-x64",
            ),
            (
                "https://cdn.example.com/tool.1.0.0.linux.x86_64.tar.xz",
                "linux-x64",
            ),
            ("tool-macos-aarch64.tar.gz", "macos-arm64"),
            // Test URLs with query parameters and fragments
            (
                "https://releases.example.com/tool-linux-arm64.tar.gz?token=abc123&version=1.0",
                "linux-arm64",
            ),
            (
                "https://releases.example.com/tool-darwin-x64.zip?v=1.0&format=zip#download",
                "macos-x64",
            ),
            // Test encoded URLs
            (
                "https://example.com/path%20with%20spaces/tool-windows-amd64.exe",
                "windows-x64",
            ),
        ];

        for (url, expected_platform) in test_cases {
            let platform = detect_platform_from_url(url)
                .unwrap_or_else(|| panic!("Failed to detect platform from URL: {url}"));
            assert_eq!(
                platform.to_platform_string(),
                expected_platform,
                "URL: {url}"
            );
        }
    }

    #[test]
    fn test_windows_msvc_preference() {
        // Test qsv assets
        let qsv_assets = vec![
            "qsv-8.1.1-x86_64-pc-windows-gnu.zip".to_string(),
            "qsv-8.1.1-x86_64-pc-windows-msvc.zip".to_string(),
        ];

        // Windows should prefer MSVC over GNU
        let picker = AssetPicker::new("windows".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&qsv_assets).unwrap();
        assert_eq!(picked, "qsv-8.1.1-x86_64-pc-windows-msvc.zip");

        // Verify MSVC scores higher than GNU on Windows
        let msvc_score = picker.score_asset("qsv-8.1.1-x86_64-pc-windows-msvc.zip");
        let gnu_score = picker.score_asset("qsv-8.1.1-x86_64-pc-windows-gnu.zip");
        assert!(
            msvc_score > gnu_score,
            "MSVC variant should score higher than GNU on Windows"
        );
    }

    #[test]
    fn test_windows_libc_scoring() {
        let picker = AssetPicker::new("windows".to_string(), "x86_64".to_string());

        // Test that MSVC scores positively and GNU scores negatively on Windows
        let msvc_score = picker.score_asset("tool-1.0.0-x86_64-pc-windows-msvc.zip");
        let gnu_score = picker.score_asset("tool-1.0.0-x86_64-pc-windows-gnu.zip");
        let no_libc_score = picker.score_asset("tool-1.0.0-windows-x86_64.zip");

        assert!(
            msvc_score > gnu_score,
            "MSVC should score higher than GNU on Windows"
        );
        assert!(
            msvc_score > no_libc_score,
            "MSVC should score higher than assets without libc info"
        );
        assert!(
            gnu_score < no_libc_score,
            "GNU should score lower than assets without libc info on Windows"
        );
    }

    #[test]
    fn test_sourcery_macos_assets() {
        // Real Sourcery assets - the plain .zip is the macOS binary,
        // .artifactbundle.zip is a Swift Package Manager bundle (not for direct install),
        // and Ubuntu tarballs are for Linux
        let sourcery_assets = vec![
            "sourcery-2.3.0-ubuntu-22.04.5-lts-jammy-x86_64.tar.xz".to_string(),
            "sourcery-2.3.0.artifactbundle.zip".to_string(),
            "sourcery-2.3.0.zip".to_string(),
        ];

        // macOS x86_64 should pick the plain .zip (not the artifactbundle or ubuntu)
        let picker = AssetPicker::new("macos".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&sourcery_assets).unwrap();
        assert_eq!(
            picked, "sourcery-2.3.0.zip",
            "macOS should pick the plain .zip, not artifactbundle or ubuntu"
        );

        // macOS arm64 should also pick the plain .zip
        let picker = AssetPicker::new("macos".to_string(), "aarch64".to_string());
        let picked = picker.pick_best_asset(&sourcery_assets).unwrap();
        assert_eq!(
            picked, "sourcery-2.3.0.zip",
            "macOS arm64 should pick the plain .zip"
        );

        // Linux x86_64 should pick the ubuntu tarball
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());
        let picked = picker.pick_best_asset(&sourcery_assets).unwrap();
        assert_eq!(
            picked, "sourcery-2.3.0-ubuntu-22.04.5-lts-jammy-x86_64.tar.xz",
            "Linux should pick the ubuntu tarball"
        );
    }

    #[test]
    fn test_artifactbundle_penalty() {
        let picker = AssetPicker::new("macos".to_string(), "x86_64".to_string());

        // .artifactbundle.zip should score lower than plain .zip
        let plain_score = picker.score_asset("tool-1.0.0.zip");
        let artifactbundle_score = picker.score_asset("tool-1.0.0.artifactbundle.zip");

        assert!(
            plain_score > artifactbundle_score,
            "Plain .zip ({}) should score higher than .artifactbundle.zip ({})",
            plain_score,
            artifactbundle_score
        );
    }

    #[test]
    fn test_ubuntu_detected_as_linux() {
        let picker = AssetPicker::new("linux".to_string(), "x86_64".to_string());

        // Ubuntu should be detected as Linux
        let ubuntu_score = picker.score_asset("tool-ubuntu-22.04-x86_64.tar.gz");
        assert!(
            ubuntu_score > 0,
            "Ubuntu assets should score positively on Linux: {}",
            ubuntu_score
        );

        // On macOS, ubuntu should score negatively (wrong OS)
        let macos_picker = AssetPicker::new("macos".to_string(), "x86_64".to_string());
        let ubuntu_score_on_macos = macos_picker.score_asset("tool-ubuntu-22.04-x86_64.tar.gz");
        assert!(
            ubuntu_score_on_macos < ubuntu_score,
            "Ubuntu assets should score lower on macOS ({}) than Linux ({})",
            ubuntu_score_on_macos,
            ubuntu_score
        );
    }
}
