//! System package managers (apk, apt, brew, brew-cask, mas) for the `[bootstrap.packages]` config section.
//!
//! These are machine-global, unversioned packages — deliberately separate from
//! the `Backend` system, which manages per-project, version-pinned dev tools.

use std::sync::Arc;

use async_trait::async_trait;

use crate::result::Result;

pub mod apk;
pub mod apt;
#[cfg(unix)]
pub mod brew;
pub mod dnf;
pub mod mas;
pub mod pacman;

/// A single package entry from `[bootstrap.packages]` — the part after the
/// `manager:` prefix of a `"manager:package" = "version"` config entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageRequest {
    /// package name as written in the spec (apt: may carry an `:arch`
    /// qualifier like "gcc:arm64"; brew/brew-cask: full name incl. "@17")
    pub name: String,
    /// version pin from the config value (`"latest"` parses to None). Each
    /// manager renders this into its native pin syntax at install time
    /// (apt: `name=version`, dnf: `name-version`).
    pub version: Option<String>,
    /// manager-specific source URL. Currently used by brew tapped formulae
    /// and casks: `[bootstrap.brew.taps]` can attach a git URL to
    /// `owner/tap/name`.
    pub tap_url: Option<String>,
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
    /// force a package manager metadata refresh before installing
    pub update: bool,
}

// `?Send`: the brew manager's source-build path drives the toolset
// machinery (to provision ruby), which holds non-Send shell state across
// awaits. The driver awaits managers sequentially on one task, so the
// futures never cross threads.
#[async_trait(?Send)]
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

    /// Upgrade the given packages (already filtered to installed ones).
    /// Defaults to `install` — for brew that is exactly right (pouring a
    /// formula whose current version differs replaces the old keg), and apt/
    /// dnf/pacman override to refresh metadata first and use their native
    /// upgrade invocation.
    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.install(pkgs, opts).await
    }

    /// Can `install` satisfy a version pin? pacman (Arch repos only carry
    /// the latest version) and brew (bottles only exist for a formula's
    /// current version) cannot — their pins are status-only, and the
    /// install command skips them with a warning instead of failing the
    /// rest of the batch.
    fn supports_version_pins(&self) -> bool {
        true
    }
}

pub fn all_managers() -> Vec<Arc<dyn SystemPackageManager>> {
    vec![
        Arc::new(apk::ApkManager::new()),
        Arc::new(apt::AptManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewCaskManager::new()),
        Arc::new(dnf::DnfManager::new()),
        Arc::new(mas::MasManager::new()),
        Arc::new(pacman::PacmanManager::new()),
    ]
}

pub fn get_manager(name: &str) -> Option<Arc<dyn SystemPackageManager>> {
    all_managers().into_iter().find(|m| m.name() == name)
}
