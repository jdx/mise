//! System package managers (apt, brew) for the `[system.packages]` config section.
//!
//! These are machine-global, unversioned packages — deliberately separate from
//! the `Backend` system, which manages per-project, version-pinned dev tools.

use std::sync::Arc;

use async_trait::async_trait;

use crate::result::Result;

pub mod apt;
pub mod brew;
pub mod dnf;
pub mod pacman;

/// A single package entry from `[system.packages]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageRequest {
    /// raw string from config, passed through to the manager
    /// (e.g. "libssl-dev", "curl=8.5.0-2", "postgresql@17")
    pub raw: String,
    /// package name (apt: portion before '='; brew: full formula name incl. "@17")
    pub name: String,
    /// apt: pinned version after '='; brew: None (the version lives in the formula name)
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageState {
    Installed {
        version: String,
    },
    Missing,
    /// installed, but the version pinned in config doesn't match
    VersionMismatch {
        installed: String,
    },
}

#[derive(Debug, Clone)]
pub struct PackageStatus {
    pub request: PackageRequest,
    pub state: PackageState,
}

#[derive(Debug, Default)]
pub struct InstallOpts {
    /// print what would be done without doing it
    pub dry_run: bool,
    /// apt: force `apt-get update` before installing
    pub update: bool,
}

#[async_trait]
pub trait SystemPackageManager: Send + Sync {
    /// config key, e.g. "apt", "brew"
    fn name(&self) -> &'static str;

    /// whether this manager can run on this machine (OS + required binaries).
    /// Entries for unavailable managers are silently skipped so configs can be
    /// shared across platforms.
    fn is_available(&self) -> bool;

    /// human-readable reason `is_available()` is false, for `status`/`doctor`
    fn unavailable_reason(&self) -> String;

    /// Query installed state. Must be side-effect free and never elevate.
    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>>;

    /// Install the given packages (already filtered to missing/mismatched).
    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()>;

    /// Parse a raw config entry into a request. Default splits no version out.
    fn parse_request(&self, raw: &str) -> PackageRequest {
        PackageRequest {
            raw: raw.to_string(),
            name: raw.to_string(),
            version: None,
        }
    }
}

pub fn all_managers() -> Vec<Arc<dyn SystemPackageManager>> {
    vec![
        Arc::new(apt::AptManager::new()),
        Arc::new(brew::BrewManager::new()),
        Arc::new(dnf::DnfManager::new()),
        Arc::new(pacman::PacmanManager::new()),
    ]
}

pub fn get_manager(name: &str) -> Option<Arc<dyn SystemPackageManager>> {
    all_managers().into_iter().find(|m| m.name() == name)
}
