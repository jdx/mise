use crate::config::Settings;
use eyre::{Result, bail};
use std::fmt;

/// Represents a target platform for lockfile operations
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub qualifier: Option<String>,
}

impl Platform {
    /// Parse a platform string in the format "os-arch" or "os-arch-qualifier"
    pub fn parse(platform_str: &str) -> Result<Self> {
        let parts: Vec<&str> = platform_str.split('-').collect();

        match parts.len() {
            2 => Ok(Platform {
                os: parts[0].to_string(),
                arch: parts[1].to_string(),
                qualifier: None,
            }),
            3 => Ok(Platform {
                os: parts[0].to_string(),
                arch: parts[1].to_string(),
                qualifier: Some(parts[2].to_string()),
            }),
            _ => bail!(
                "Invalid platform format '{}'. Expected 'os-arch' or 'os-arch-qualifier'",
                platform_str
            ),
        }
    }

    /// Get the current platform from system information
    pub fn current() -> Self {
        let settings = Settings::get();
        Platform {
            os: settings.os().to_string(),
            arch: settings.arch().to_string(),
            qualifier: None,
        }
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
            "x64" | "arm64" | "x86" => {}
            _ => bail!(
                "Unsupported architecture '{}'. Supported: x64, arm64, x86",
                self.arch
            ),
        }

        // Validate qualifier if present
        if let Some(qualifier) = &self.qualifier {
            match qualifier.as_str() {
                "gnu" | "musl" | "msvc" => {}
                _ => bail!(
                    "Unsupported qualifier '{}'. Supported: gnu, musl, msvc",
                    qualifier
                ),
            }
        }

        Ok(())
    }

    /// Check if this platform is compatible with the current system
    pub fn is_compatible_with_current(&self) -> bool {
        let current = Self::current();
        self.os == current.os && self.arch == current.arch
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
            Platform::parse("linux-arm64").unwrap(),
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

    /// Check if this uses ARM64 architecture
    pub fn is_arm64(&self) -> bool {
        self.arch == "arm64"
    }

    /// Check if this uses x64 architecture
    pub fn is_x64(&self) -> bool {
        self.arch == "x64"
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
    fn test_platform_parse_invalid() {
        assert!(Platform::parse("linux").is_err());
        assert!(Platform::parse("linux-x64-gnu-extra").is_err());
        assert!(Platform::parse("").is_err());
    }

    #[test]
    fn test_platform_validation() {
        // Valid platforms
        assert!(Platform::parse("linux-x64").unwrap().validate().is_ok());
        assert!(Platform::parse("macos-arm64").unwrap().validate().is_ok());
        assert!(Platform::parse("windows-x64").unwrap().validate().is_ok());
        assert!(Platform::parse("linux-x64-gnu").unwrap().validate().is_ok());

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
        assert!(linux_platform.is_arm64());
        assert!(!linux_platform.is_windows());
        assert!(!linux_platform.is_x64());

        let windows_platform = Platform::parse("windows-x64").unwrap();
        assert!(windows_platform.is_windows());
        assert!(windows_platform.is_x64());
        assert!(!windows_platform.is_linux());
        assert!(!windows_platform.is_arm64());
    }

    #[test]
    fn test_common_platforms() {
        let platforms = Platform::common_platforms();
        assert_eq!(platforms.len(), 5);

        let keys: Vec<String> = platforms.iter().map(|p| p.to_key()).collect();
        assert!(keys.contains(&"linux-x64".to_string()));
        assert!(keys.contains(&"linux-arm64".to_string()));
        assert!(keys.contains(&"macos-x64".to_string()));
        assert!(keys.contains(&"macos-arm64".to_string()));
        assert!(keys.contains(&"windows-x64".to_string()));
    }
}
