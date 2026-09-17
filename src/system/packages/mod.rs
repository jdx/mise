//! Host package managers (apk, apt, aur, brew, brew-cask, flatpak, flatpak-user, macos-app, mas, winget) for the `[bootstrap.packages]` config section.
//!
//! These are host-owned, unversioned packages — deliberately separate from
//! the `Backend` system, which manages per-project, version-pinned dev tools.

use std::sync::Arc;

use async_trait::async_trait;

use crate::result::Result;
use crate::system::ManagerPackageOptions;

pub(crate) mod apk;
pub(crate) mod apt;
pub(crate) mod aur;
#[cfg(unix)]
pub(crate) mod brew;
pub(crate) mod dnf;
pub(crate) mod flatpak;
pub(crate) mod mas;
pub(crate) mod nix;
pub(crate) mod pacman;
pub(crate) mod plugin;
pub(crate) mod winget;

/// A single package entry from `[bootstrap.packages]` — the part after the
/// `manager:` prefix of a `"manager:package" = "version"` config entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageRequest {
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
    /// Desired declarative state. Explicit CLI package requests are always
    /// present; table-form config entries may request removal.
    pub desired: PackageDesiredState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) enum PackageDesiredState {
    #[default]
    Present,
    Absent,
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
pub(crate) enum PackageState {
    Installed {
        version: String,
    },
    /// Installed cask whose upstream definition declares `auto_updates`.
    /// The version is the cask receipt version, not necessarily the live app
    /// bundle version after it has updated itself.
    #[cfg(unix)]
    InstalledAutoUpdates {
        version: String,
    },
    Missing,
    /// installed, but a manager-owned record needs local repair
    #[cfg_attr(windows, allow(dead_code))]
    NeedsRepair {
        installed: String,
    },
    /// installed, but the version pinned in config doesn't match
    VersionMismatch {
        installed: String,
    },
    /// The manager is available on this host, but this individual package is
    /// not supported on the current platform.
    #[cfg(unix)]
    Unavailable {
        reason: String,
    },
}

impl PackageState {
    pub(crate) fn is_installed(&self) -> bool {
        match self {
            Self::Installed { .. } => true,
            #[cfg(unix)]
            Self::InstalledAutoUpdates { .. } => true,
            _ => false,
        }
    }

    pub(crate) fn auto_updates(&self) -> bool {
        match self {
            #[cfg(unix)]
            Self::InstalledAutoUpdates { .. } => true,
            _ => false,
        }
    }

    #[cfg(unix)]
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable {
            reason: reason.into(),
        }
    }

    pub(crate) fn is_unavailable(&self) -> bool {
        #[cfg(unix)]
        if matches!(self, Self::Unavailable { .. }) {
            return true;
        }
        false
    }

    pub(crate) fn unavailable_reason(&self) -> Option<&str> {
        #[cfg(unix)]
        if let Self::Unavailable { reason } = self {
            return Some(reason);
        }
        None
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PackageStatus {
    pub request: PackageRequest,
    pub state: PackageState,
}

#[derive(Debug, Default)]
pub(crate) struct InstallOpts {
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
pub(crate) trait SystemPackageManager: Send + Sync {
    /// config key, e.g. "apt", "brew"
    fn name(&self) -> &str;

    /// whether this manager can run on this machine (OS + required binaries).
    /// Entries for unavailable managers are silently skipped so configs can be
    /// shared across platforms.
    fn is_available(&self) -> bool;

    /// human-readable reason `is_available()` is false, for `status`/`doctor`
    fn unavailable_reason(&self) -> String;

    /// Return why this manager cannot run, or `None` when it is available.
    ///
    /// The async form lets plugin managers resolve host binaries from mise's
    /// global toolset as well as the process PATH and shims. Built-in managers
    /// use the synchronous checks above by default.
    async fn unavailable_reason_async(&self) -> Option<String> {
        (!self.is_available()).then(|| self.unavailable_reason())
    }

    /// Query installed state. Must be side-effect free and never elevate.
    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>>;

    /// Prepare for a mutating package operation before querying installed state.
    ///
    /// This hook is never called for status or dry-run operations. Managers may
    /// use it for mutation prerequisites that their read-only query cannot
    /// perform, such as accepting repository agreements.
    async fn prepare_mutation(&self, _pkgs: &[PackageRequest]) -> Result<()> {
        Ok(())
    }

    /// Whether each name exists as an installable package, positionally.
    ///
    /// This is *availability*, not installed state — [`Self::installed`]
    /// cannot answer it (apt's asks dpkg, which only knows what is already on
    /// the box). Used to resolve a plugin's candidate package names, where the
    /// same capability is packaged under different names across distro
    /// releases. Must be side-effect free and never elevate.
    ///
    /// The default reports every name as available, which makes candidate
    /// resolution pick the first one — the behavior before candidate lists
    /// existed. Managers override it where the query is cheap.
    async fn available(&self, names: &[String]) -> Result<Vec<bool>> {
        Ok(vec![true; names.len()])
    }

    /// Install the given packages (already filtered to missing, mismatched, or repairable).
    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()>;

    /// Remove packages declared with `state = "absent"`.
    async fn remove(&self, _pkgs: &[PackageRequest], _opts: &InstallOpts) -> Result<()> {
        eyre::bail!(
            "{} does not support declarative package removal",
            self.name()
        )
    }

    fn supports_remove(&self) -> bool {
        false
    }

    /// Query installed state with manager-specific declarative options.
    ///
    /// `macos-app` resolves no metadata of its own, so its status query needs
    /// the inline declaration the same way its install does. Managers without
    /// additional package options use the ordinary query unchanged.
    async fn installed_with_options(
        &self,
        pkgs: &[PackageRequest],
        _manager_options: &ManagerPackageOptions,
    ) -> Result<Vec<PackageStatus>> {
        self.installed(pkgs).await
    }

    /// Upgrade with manager-specific declarative options, for the same reason
    /// [`Self::installed_with_options`] exists.
    async fn upgrade_with_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        _manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        self.upgrade(pkgs, opts).await
    }

    /// Install with manager-specific declarative options. Managers without
    /// additional package options use the ordinary install path unchanged.
    async fn install_with_options(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        _manager_options: &ManagerPackageOptions,
    ) -> Result<()> {
        self.install(pkgs, opts).await
    }

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

    /// Whether this manager is supplied by a package plugin.
    fn is_plugin(&self) -> bool {
        false
    }

    /// The package a `[bootstrap.packages]` name resolves to, when this
    /// manager accepts more than one spelling for a single package.
    ///
    /// `[bootstrap.packages]` keys on the literal spec, so `winget:Git.Git`
    /// and `winget:git.git` are two config entries even though WinGet matches
    /// both to one package. The default `None` compares names verbatim, which
    /// is right for apt, dnf, pacman and apk: there two spellings really are
    /// two packages, and a config may legitimately declare both. A manager
    /// that folds spellings returns the folded form, and
    /// [`check_name_conflicts`] rejects entries that fold together but
    /// disagree about the package.
    fn package_identity(&self, _name: &str) -> Option<String> {
        None
    }
}

/// Rejects two `[bootstrap.packages]` entries that name one package but
/// disagree about it.
///
/// A manager that folds spellings (see
/// [`SystemPackageManager::package_identity`]) receives one [`PackageRequest`]
/// per spelling, each carrying its own desired state and version pin. Neither
/// disagreement has a defined outcome, so both are rejected rather than
/// silently resolved:
///
/// * On state, the driver computes its removal and install sets from the same
///   pre-removal status snapshot and removes first. An absent declaration
///   therefore wins for a package that is installed and a present one wins for
///   a package that is missing — the result follows the machine rather than
///   the config.
/// * On version, both pins reach the manager in a single invocation, and which
///   pin survives is that manager's arbitrary ordering.
pub(crate) fn check_name_conflicts(
    manager: &dyn SystemPackageManager,
    pkgs: &[PackageRequest],
) -> Result<()> {
    let mgr = manager.name();
    let identities = pkgs
        .iter()
        .map(|pkg| manager.package_identity(&pkg.name))
        .collect::<Vec<_>>();
    for (index, pkg) in pkgs.iter().enumerate() {
        let Some(identity) = &identities[index] else {
            continue;
        };
        let rest = pkgs[index + 1..].iter().zip(&identities[index + 1..]);
        for (other, other_identity) in rest {
            if other_identity.as_ref() != Some(identity) {
                continue;
            }
            if pkg.desired != other.desired {
                let (present, absent) = match pkg.desired {
                    PackageDesiredState::Present => (&pkg.name, &other.name),
                    PackageDesiredState::Absent => (&other.name, &pkg.name),
                };
                eyre::bail!(
                    "[bootstrap.packages]: '{mgr}:{present}' and '{mgr}:{absent}' name the same \
                     package but ask for opposite states; declare it once"
                );
            }
            if pkg.desired == PackageDesiredState::Present && pkg.version != other.version {
                eyre::bail!(
                    "[bootstrap.packages]: '{mgr}:{}' and '{mgr}:{}' name the same package but \
                     ask for different versions ({} and {}); declare it once",
                    pkg.name,
                    other.name,
                    pkg.version.as_deref().unwrap_or("latest"),
                    other.version.as_deref().unwrap_or("latest"),
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn builtin_managers() -> Vec<Arc<dyn SystemPackageManager>> {
    vec![
        Arc::new(apk::ApkManager::new()),
        Arc::new(apt::AptManager::new()),
        Arc::new(aur::AurManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewCaskManager::new()),
        #[cfg(unix)]
        Arc::new(brew::BrewCaskManager::new_macos_app()),
        Arc::new(dnf::DnfManager::new()),
        Arc::new(flatpak::FlatpakManager::new()),
        Arc::new(flatpak::FlatpakManager::new_user()),
        Arc::new(mas::MasManager::new()),
        Arc::new(nix::NixManager),
        Arc::new(pacman::PacmanManager::new()),
        Arc::new(winget::WingetManager::new()),
    ]
}

pub(crate) fn is_builtin_manager_name(name: &str) -> bool {
    builtin_managers()
        .iter()
        .any(|manager| manager.name() == name)
}

pub(crate) fn all_managers() -> Vec<Arc<dyn SystemPackageManager>> {
    let mut managers = builtin_managers();
    let builtins = managers
        .iter()
        .map(|manager| manager.name().to_string())
        .collect::<std::collections::HashSet<_>>();
    let Some(plugins) = crate::toolset::install_state::try_list_plugins() else {
        return managers;
    };
    for (name, plugin_type) in plugins.iter() {
        if *plugin_type != crate::plugins::PluginType::Package {
            continue;
        }
        if builtins.contains(name) {
            warn!(
                "package plugin '{name}' collides with a built-in package manager; ignoring plugin"
            );
            continue;
        }
        match plugin::PackagePluginManager::new(name.clone()) {
            Ok(manager) => managers.push(Arc::new(manager)),
            Err(err) => warn!("failed to load package plugin '{name}': {err:#}"),
        }
    }
    managers
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for apt/dnf/pacman/apk, where two spellings are two packages.
    struct VerbatimManager;

    #[async_trait(?Send)]
    impl SystemPackageManager for VerbatimManager {
        fn name(&self) -> &str {
            "verbatim"
        }
        fn is_available(&self) -> bool {
            true
        }
        fn unavailable_reason(&self) -> String {
            unreachable!()
        }
        async fn installed(&self, _pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
            unreachable!()
        }
        async fn install(&self, _pkgs: &[PackageRequest], _opts: &InstallOpts) -> Result<()> {
            unreachable!()
        }
    }

    /// Stands in for winget/scoop, where names fold to one package.
    struct FoldingManager;

    #[async_trait(?Send)]
    impl SystemPackageManager for FoldingManager {
        fn name(&self) -> &str {
            "folding"
        }
        fn is_available(&self) -> bool {
            true
        }
        fn unavailable_reason(&self) -> String {
            unreachable!()
        }
        fn package_identity(&self, name: &str) -> Option<String> {
            Some(name.to_ascii_lowercase())
        }
        async fn installed(&self, _pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
            unreachable!()
        }
        async fn install(&self, _pkgs: &[PackageRequest], _opts: &InstallOpts) -> Result<()> {
            unreachable!()
        }
    }

    fn request(name: &str, version: Option<&str>, desired: PackageDesiredState) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            desired,
        }
    }

    #[test]
    fn folded_names_with_opposite_states_are_rejected() {
        let pkgs = vec![
            request("Git.Git", None, PackageDesiredState::Present),
            request("git.git", None, PackageDesiredState::Absent),
        ];
        let err = check_name_conflicts(&FoldingManager, &pkgs).unwrap_err();
        let msg = err.to_string();
        // both spellings are named, so the config lines are findable
        assert!(msg.contains("'folding:Git.Git'"), "{msg}");
        assert!(msg.contains("'folding:git.git'"), "{msg}");
        assert!(msg.contains("opposite states"), "{msg}");

        // the pair is rejected whichever order it is declared in
        let reversed = vec![pkgs[1].clone(), pkgs[0].clone()];
        assert_eq!(
            check_name_conflicts(&FoldingManager, &reversed)
                .unwrap_err()
                .to_string(),
            msg
        );
    }

    #[test]
    fn folded_names_with_different_versions_are_rejected() {
        let pkgs = vec![
            request("Git.Git", Some("2.43.0"), PackageDesiredState::Present),
            request("git.git", None, PackageDesiredState::Present),
        ];
        let msg = check_name_conflicts(&FoldingManager, &pkgs)
            .unwrap_err()
            .to_string();
        assert!(
            msg.contains("different versions (2.43.0 and latest)"),
            "{msg}"
        );
    }

    #[test]
    fn folded_names_that_agree_are_left_alone() {
        let present = vec![
            request("Git.Git", Some("2.43.0"), PackageDesiredState::Present),
            request("git.git", Some("2.43.0"), PackageDesiredState::Present),
        ];
        assert!(check_name_conflicts(&FoldingManager, &present).is_ok());

        // two absent declarations carry no version to disagree about
        let absent = vec![
            request("Git.Git", None, PackageDesiredState::Absent),
            request("git.git", Some("2.43.0"), PackageDesiredState::Absent),
        ];
        assert!(check_name_conflicts(&FoldingManager, &absent).is_ok());
    }

    #[test]
    fn verbatim_names_that_differ_only_by_case_are_two_packages() {
        let pkgs = vec![
            request("Git", None, PackageDesiredState::Present),
            request("git", None, PackageDesiredState::Absent),
        ];
        assert!(check_name_conflicts(&VerbatimManager, &pkgs).is_ok());
    }

    #[test]
    fn winget_folds_package_ids() {
        let winget = winget::WingetManager::new();
        assert_eq!(
            winget.package_identity("Git.Git"),
            Some("git.git".to_string())
        );
    }

    #[test]
    fn apt_compares_package_names_verbatim() {
        assert_eq!(apt::AptManager::new().package_identity("Git"), None);
    }
}
