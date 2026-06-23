use crate::config::Settings;
use eyre::{Result, bail};
use std::{collections::BTreeMap, fmt};

/// Represents a target platform for lockfile operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub qualifier: Option<String>,
}

impl Platform {
    /// Parse a platform string in the format "os-arch" or "os-arch-qualifier"
    /// Qualifier may contain hyphens (e.g., "musl-baseline")
    pub fn parse(platform_str: &str) -> Result<Self> {
        let parts: Vec<&str> = platform_str.split('-').collect();

        match parts.len() {
            0 | 1 => bail!(
                "Invalid platform format '{}'. Expected 'os-arch' or 'os-arch-qualifier'",
                platform_str
            ),
            2 => Ok(Platform {
                os: parts[0].to_string(),
                arch: parts[1].to_string(),
                qualifier: None,
            }),
            _ => {
                // Join remaining parts as qualifier (handles compound qualifiers like "musl-baseline")
                let qualifier = parts[2..].join("-");
                Ok(Platform {
                    os: parts[0].to_string(),
                    arch: parts[1].to_string(),
                    qualifier: Some(qualifier),
                })
            }
        }
    }

    /// Get the current platform from system information.
    /// On Linux, detects musl vs glibc at runtime and sets the qualifier accordingly.
    pub fn current() -> Self {
        let settings = Settings::get();
        let os = settings.os().to_string();
        let qualifier = if os == "linux" {
            match settings.libc() {
                Some("musl") => Some("musl".to_string()),
                Some("gnu") => None,
                _ if is_musl_system() => Some("musl".to_string()),
                _ => None,
            }
        } else {
            None
        };
        Platform {
            os,
            arch: settings.arch().to_string(),
            qualifier,
        }
    }

    pub fn libc(&self) -> Option<&str> {
        self.qualifier
            .as_deref()?
            .split('-')
            .find_map(|part| match part {
                "gnu" | "glibc" => Some("gnu"),
                "musl" => Some("musl"),
                _ => None,
            })
    }

    /// Validate that this platform is supported
    pub fn validate(&self) -> Result<()> {
        // Validate OS
        match self.os.as_str() {
            "linux" | "macos" | "windows" => {}
            _ => bail!(
                "Unsupported OS '{}'. Supported: linux, macos, windows",
                self.os
            ),
        }

        // Validate architecture
        match self.arch.as_str() {
            "x64" | "arm64" | "x86" | "loongarch64" | "riscv64" => {}
            _ => bail!(
                "Unsupported architecture '{}'. Supported: x64, arm64, x86, loongarch64, riscv64",
                self.arch
            ),
        }

        // Validate qualifier if present
        if let Some(qualifier) = &self.qualifier {
            match qualifier.as_str() {
                "gnu" | "glibc" | "musl" | "msvc" | "baseline" | "musl-baseline" => {}
                _ => bail!(
                    "Unsupported qualifier '{}'. Supported: gnu, glibc, musl, msvc, baseline, musl-baseline",
                    qualifier
                ),
            }
        }

        Ok(())
    }

    /// Convert to platform key format used in lockfiles
    pub fn to_key(&self) -> String {
        match &self.qualifier {
            Some(qualifier) => format!("{}-{}-{}", self.os, self.arch, qualifier),
            None => format!("{}-{}", self.os, self.arch),
        }
    }

    /// Parse multiple platform strings, validating each one
    pub fn parse_multiple(platform_strings: &[String]) -> Result<Vec<Self>> {
        let mut platforms = Vec::new();

        for platform_str in platform_strings {
            let platform = Self::parse(platform_str)?;
            platform.validate()?;
            platforms.push(platform);
        }

        // Remove duplicates and sort
        platforms.sort();
        platforms.dedup();

        Ok(platforms)
    }

    /// Get a list of commonly supported platforms
    pub fn common_platforms() -> Vec<Self> {
        vec![
            Platform::parse("linux-x64").unwrap(),
            Platform::parse("linux-x64-musl").unwrap(),
            Platform::parse("linux-arm64").unwrap(),
            Platform::parse("linux-arm64-musl").unwrap(),
            Platform::parse("macos-x64").unwrap(),
            Platform::parse("macos-arm64").unwrap(),
            Platform::parse("windows-x64").unwrap(),
        ]
    }

    /// Check if this is a Windows platform
    pub fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    /// Check if this is a macOS platform
    pub fn is_macos(&self) -> bool {
        self.os == "macos"
    }

    /// Check if this is a Linux platform
    pub fn is_linux(&self) -> bool {
        self.os == "linux"
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_key())
    }
}

impl From<String> for Platform {
    fn from(s: String) -> Self {
        Self::parse(&s).unwrap_or_else(|_| {
            // Fallback to current platform if parsing fails
            Self::current()
        })
    }
}

impl From<&str> for Platform {
    fn from(s: &str) -> Self {
        Self::parse(s).unwrap_or_else(|_| {
            // Fallback to current platform if parsing fails
            Self::current()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxOsRelease {
    pub id: String,
    pub version_id: String,
    pub id_like: Vec<String>,
}

impl LinuxOsRelease {
    fn parse(content: &str) -> Option<Self> {
        let mut values = BTreeMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            values.insert(key.trim().to_string(), parse_os_release_value(value.trim()));
        }

        Some(Self {
            id: values.remove("ID")?,
            version_id: values.remove("VERSION_ID").unwrap_or_default(),
            id_like: values
                .remove("ID_LIKE")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        })
    }

    #[cfg(target_os = "linux")]
    fn ids(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.id.as_str()).chain(self.id_like.iter().map(String::as_str))
    }
}

pub fn linux_os_release() -> Option<&'static LinuxOsRelease> {
    use std::sync::LazyLock;
    static OS_RELEASE: LazyLock<Option<LinuxOsRelease>> =
        LazyLock::new(|| read_linux_os_release("/etc/os-release"));
    OS_RELEASE.as_ref()
}

fn read_linux_os_release(path: &str) -> Option<LinuxOsRelease> {
    LinuxOsRelease::parse(&std::fs::read_to_string(path).ok()?)
}

fn parse_os_release_value(value: &str) -> String {
    let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') else {
        return value.to_string();
    };

    let mut parsed = String::new();
    let mut chars = value[quote.len_utf8()..].chars();
    while let Some(ch) = chars.next() {
        if ch == quote {
            break;
        }
        if quote == '"' && ch == '\\' {
            if let Some(next) = chars.next() {
                parsed.push(next);
            }
        } else {
            parsed.push(ch);
        }
    }
    parsed
}

/// Detect the current libc variant on Linux.
///
/// Returns `Some("gnu")` on glibc Linux, `Some("musl")` on musl Linux,
/// `None` on non-Linux or when the variant can't be determined (e.g. minimal
/// containers compiled against an unusual target_env).
///
/// Detection order on Linux:
///   1. `/etc/os-release` ID/ID_LIKE — strong signal for known musl distros.
///      Necessary because compat shims like `gcompat` on Alpine install
///      `/lib/ld-linux-*` alongside `/lib/ld-musl-*`, which would otherwise
///      cause the linker-based fallback to misclassify the system as glibc.
///   2. Linker file presence in `/lib` and `/lib64`.
///   3. Compile-time target (`target_env`) — for scratch/busybox containers
///      with no linker files.
#[cfg(target_os = "linux")]
pub fn detect_libc() -> Option<&'static str> {
    use std::sync::LazyLock;
    static DETECTED: LazyLock<Option<&'static str>> = LazyLock::new(|| {
        if linux_os_release().is_some_and(linux_os_release_is_musl) {
            return Some("musl");
        }
        for dir in ["/lib", "/lib64"] {
            if has_file_prefix(dir, "ld-linux-") {
                return Some("gnu");
            }
        }
        for dir in ["/lib", "/lib64"] {
            if has_file_prefix(dir, "ld-musl-") {
                return Some("musl");
            }
        }
        if cfg!(target_env = "musl") {
            return Some("musl");
        }
        if cfg!(target_env = "gnu") {
            return Some("gnu");
        }
        None
    });
    *DETECTED
}

#[cfg(not(target_os = "linux"))]
pub fn detect_libc() -> Option<&'static str> {
    None
}

#[cfg(target_os = "linux")]
fn linux_os_release_is_musl(release: &LinuxOsRelease) -> bool {
    // Known musl-libc distros. Compat shims (gcompat) don't change this — the
    // underlying libc is still musl.
    const MUSL_DISTROS: &[&str] = &["alpine", "postmarketos", "chimera"];
    release.ids().any(|id| MUSL_DISTROS.contains(&id))
}

#[cfg(target_os = "linux")]
fn has_file_prefix(dir: &str, prefix: &str) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with(prefix))
        })
        .unwrap_or(false)
}

fn is_musl_system() -> bool {
    detect_libc() == Some("musl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_parse_basic() {
        let platform = Platform::parse("linux-x64").unwrap();
        assert_eq!(platform.os, "linux");
        assert_eq!(platform.arch, "x64");
        assert_eq!(platform.qualifier, None);
    }

    #[test]
    fn test_platform_parse_with_qualifier() {
        let platform = Platform::parse("linux-x64-gnu").unwrap();
        assert_eq!(platform.os, "linux");
        assert_eq!(platform.arch, "x64");
        assert_eq!(platform.qualifier, Some("gnu".to_string()));
    }

    #[test]
    fn test_platform_parse_with_compound_qualifier() {
        // Compound qualifiers like "musl-baseline" should parse correctly
        let platform = Platform::parse("linux-x64-musl-baseline").unwrap();
        assert_eq!(platform.os, "linux");
        assert_eq!(platform.arch, "x64");
        assert_eq!(platform.qualifier, Some("musl-baseline".to_string()));

        // Verify round-trip: parse -> to_key -> parse
        assert_eq!(platform.to_key(), "linux-x64-musl-baseline");
        let reparsed = Platform::parse(&platform.to_key()).unwrap();
        assert_eq!(reparsed.qualifier, Some("musl-baseline".to_string()));
    }

    #[test]
    fn test_platform_parse_invalid() {
        assert!(Platform::parse("linux").is_err());
        assert!(Platform::parse("").is_err());
    }

    #[test]
    fn test_platform_validation() {
        // Valid platforms
        assert!(Platform::parse("linux-x64").unwrap().validate().is_ok());
        assert!(Platform::parse("macos-arm64").unwrap().validate().is_ok());
        assert!(Platform::parse("windows-x64").unwrap().validate().is_ok());
        assert!(Platform::parse("linux-x64-gnu").unwrap().validate().is_ok());
        assert!(
            Platform::parse("linux-x64-glibc")
                .unwrap()
                .validate()
                .is_ok()
        );

        // Invalid OS
        assert!(Platform::parse("invalid-x64").unwrap().validate().is_err());

        // Invalid arch
        assert!(
            Platform::parse("linux-invalid")
                .unwrap()
                .validate()
                .is_err()
        );

        // Invalid qualifier
        assert!(
            Platform::parse("linux-x64-invalid")
                .unwrap()
                .validate()
                .is_err()
        );
    }

    #[test]
    fn test_platform_to_key() {
        let platform1 = Platform::parse("linux-x64").unwrap();
        assert_eq!(platform1.to_key(), "linux-x64");

        let platform2 = Platform::parse("linux-x64-gnu").unwrap();
        assert_eq!(platform2.to_key(), "linux-x64-gnu");
    }

    #[test]
    fn test_platform_multiple_parsing() {
        let platform_strings = vec![
            "linux-x64".to_string(),
            "macos-arm64".to_string(),
            "linux-x64".to_string(), // duplicate should be removed
        ];

        let platforms = Platform::parse_multiple(&platform_strings).unwrap();
        assert_eq!(platforms.len(), 2);
        assert_eq!(platforms[0].to_key(), "linux-x64");
        assert_eq!(platforms[1].to_key(), "macos-arm64");
    }

    #[test]
    fn test_platform_helpers() {
        let linux_platform = Platform::parse("linux-arm64").unwrap();
        assert!(linux_platform.is_linux());
        assert!(!linux_platform.is_windows());

        let windows_platform = Platform::parse("windows-x64").unwrap();
        assert!(windows_platform.is_windows());
        assert!(!windows_platform.is_linux());
    }

    #[test]
    fn test_common_platforms() {
        let platforms = Platform::common_platforms();
        assert_eq!(platforms.len(), 7);

        let keys: Vec<String> = platforms.iter().map(|p| p.to_key()).collect();
        assert!(keys.contains(&"linux-x64".to_string()));
        assert!(keys.contains(&"linux-x64-musl".to_string()));
        assert!(keys.contains(&"linux-arm64".to_string()));
        assert!(keys.contains(&"linux-arm64-musl".to_string()));
        assert!(keys.contains(&"macos-x64".to_string()));
        assert!(keys.contains(&"macos-arm64".to_string()));
        assert!(keys.contains(&"windows-x64".to_string()));
    }

    #[cfg(all(target_os = "linux", target_env = "musl"))]
    #[test]
    fn test_musl_binary_detects_musl() {
        // A musl-compiled binary should always detect musl, even in
        // minimal containers with no linker files (scratch, busybox).
        assert!(
            is_musl_system(),
            "musl-compiled binary should detect musl system"
        );
    }

    #[cfg(all(target_os = "linux", target_env = "musl"))]
    #[test]
    fn test_current_platform_has_musl_qualifier() {
        // A musl-compiled binary should always have the musl qualifier,
        // even in minimal containers with no linker files.
        let platform = Platform::current();
        assert_eq!(
            platform.qualifier.as_deref(),
            Some("musl"),
            "musl-compiled binary should have musl qualifier, got: {}",
            platform.to_key()
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_os_release_alpine_id_is_musl() {
        let release =
            LinuxOsRelease::parse("NAME=\"Alpine Linux\"\nID=alpine\nVERSION_ID=3.22.4\n").unwrap();
        assert!(linux_os_release_is_musl(&release));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_os_release_id_like_alpine_is_musl() {
        let release = LinuxOsRelease::parse("ID=postmarketos\nID_LIKE=\"alpine\"\n").unwrap();
        assert!(linux_os_release_is_musl(&release));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_os_release_debian_returns_false() {
        let release = LinuxOsRelease::parse("ID=debian\nID_LIKE=\"\"\n").unwrap();
        assert!(!linux_os_release_is_musl(&release));
    }

    #[test]
    fn test_os_release_missing_id_returns_none() {
        assert_eq!(LinuxOsRelease::parse("NAME=\"Missing ID\"\n"), None);
    }

    #[test]
    fn test_os_release_comments_and_blank_lines_do_not_short_circuit() {
        // Regression: previously `split_once('=')?` returned None on the first
        // comment or blank line, causing the function to ignore the `ID=` line
        // that came after and silently fall back to linker-based detection.
        let release =
            LinuxOsRelease::parse("# this is a comment\n\nNAME=\"Alpine Linux\"\nID=alpine\n")
                .unwrap();
        assert_eq!(release.id, "alpine");
    }

    #[test]
    fn test_os_release_whitespace_around_key_tolerated() {
        let release = LinuxOsRelease::parse("  ID = alpine \n").unwrap();
        assert_eq!(release.id, "alpine");
    }

    #[test]
    fn test_linux_os_release_parse_id_version_and_id_like() {
        let release = LinuxOsRelease::parse(
            r#"
NAME="Ubuntu"
ID=ubuntu
VERSION_ID="24.04"
ID_LIKE="debian"
"#,
        )
        .unwrap();
        assert_eq!(release.id, "ubuntu");
        assert_eq!(release.version_id, "24.04");
        assert_eq!(release.id_like, vec!["debian"]);
    }

    #[test]
    fn test_linux_os_release_parse_quoted_escapes() {
        let release = LinuxOsRelease::parse(
            r#"
ID="custom\"id"
VERSION_ID='1.2'
"#,
        )
        .unwrap();
        assert_eq!(release.id, "custom\"id");
        assert_eq!(release.version_id, "1.2");
    }
}
