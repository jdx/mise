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
