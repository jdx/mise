use crate::platform::Platform;

/// Represents a target platform for lockfile metadata fetching
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformTarget {
    pub platform: Platform,
}

impl PlatformTarget {
    pub fn new(platform: Platform) -> Self {
        Self { platform }
    }

    pub fn from_current() -> Self {
        Self::new(Platform::current())
    }

    pub fn os_name(&self) -> &str {
        &self.platform.os
    }

    pub fn arch_name(&self) -> &str {
        &self.platform.arch
    }

    pub fn qualifier(&self) -> Option<&str> {
        self.platform.qualifier.as_deref()
    }

    pub fn to_key(&self) -> String {
        self.platform.to_key()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_target_creation() {
        let platform = Platform::parse("linux-x64").unwrap();
        let target = PlatformTarget::new(platform.clone());

        assert_eq!(target.platform, platform);
        assert_eq!(target.os_name(), "linux");
        assert_eq!(target.arch_name(), "x64");
        assert_eq!(target.qualifier(), None);
        assert_eq!(target.to_key(), "linux-x64");
    }

    #[test]
    fn test_platform_target_with_qualifier() {
        let platform = Platform::parse("linux-x64-musl").unwrap();
        let target = PlatformTarget::new(platform);

        assert_eq!(target.os_name(), "linux");
        assert_eq!(target.arch_name(), "x64");
        assert_eq!(target.qualifier(), Some("musl"));
        assert_eq!(target.to_key(), "linux-x64-musl");
    }

    #[test]
    fn test_from_current() {
        let target = PlatformTarget::from_current();
        let current_platform = Platform::current();

        assert_eq!(target.platform, current_platform);
    }
}
