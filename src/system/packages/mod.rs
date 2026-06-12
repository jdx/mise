//! System package managers (apt, brew) for the `[system.packages]` config section.
//!
//! These are machine-global, unversioned packages — deliberately separate from
//! the `Backend` system, which manages per-project, version-pinned dev tools.

use std::sync::Arc;

use async_trait::async_trait;

use crate::result::Result;

pub mod apt;
#[cfg(unix)]
pub mod brew;
pub mod dnf;
pub mod pacman;

/// A single package entry from `[system.packages]` — the part after the
/// `manager:` prefix of a `"manager:package" = "version"` config entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageRequest {
    /// package name as written in the spec (apt: may carry an `:arch`
    /// qualifier like "gcc:arm64"; brew: full formula name incl. "@17")
    pub name: String,
    /// version pin from the config value (`"latest"` parses to None). Each
    /// manager renders this into its native pin syntax at install time
    /// (apt: `name=version`, dnf: `name-version`).
    pub version: Option<String>,
}

impl std::fmt::Display for PackageRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.version {
            Some(v) => write!(f, "{}@{}", self.name, v),
            None => write!(f, "{}", self.name),
        }
    }
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
}

pub fn all_managers() -> Vec<Arc<dyn SystemPackageManager>> {
    vec![
        Arc::new(apt::AptManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewManager::new()),
        Arc::new(dnf::DnfManager::new()),
        Arc::new(pacman::PacmanManager::new()),
    ]
}

pub fn get_manager(name: &str) -> Option<Arc<dyn SystemPackageManager>> {
    all_managers().into_iter().find(|m| m.name() == name)
}
