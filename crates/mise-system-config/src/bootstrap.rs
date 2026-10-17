use indexmap::IndexMap;
use serde::Deserialize;

use crate::defaults::HostScope;
use crate::launchd::LaunchdTomlConfig;
use crate::systemd::SystemdTomlConfig;
use crate::{
    accounts, compose, firewall, login_items, managed_files, packages::PackageTomlConfig, remote,
    repos, secrets, services_common as services, templating::Templated,
};

/// `[bootstrap]` as parsed from a single mise.toml
#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapTomlConfig {
    /// Independent configuration roots whose declarative files and dotfiles
    /// participate in bootstrap composition.
    pub config_roots: Option<Vec<String>>,
    /// The `[dotfile_groups]` (and `group = "..."` entries) to apply; every
    /// group applies when unset. A later layer's list replaces earlier ones.
    pub dotfile_groups: Option<Vec<String>>,
    /// Logical secret name -> environment input declaration.
    #[serde(default)]
    pub secrets: IndexMap<String, secrets::SecretTomlConfig>,
    /// Linux group name -> declarative local group.
    #[serde(default)]
    pub groups: IndexMap<String, accounts::GroupTomlConfig>,
    /// Linux user name -> declarative local user.
    #[serde(default)]
    pub users: IndexMap<String, accounts::UserTomlConfig>,
    /// Linux systemd unit name -> declarative system service lifecycle.
    #[serde(default)]
    pub services: IndexMap<String, services::ServiceTomlConfig>,
    /// Docker Compose project name -> declarative project lifecycle.
    #[serde(default)]
    pub compose: IndexMap<String, Templated<compose::ComposeTomlConfig>>,
    /// OpenSSH targets used by `mise bootstrap remote`.
    #[serde(default)]
    pub remote: remote::RemoteTomlConfig,
    /// Package manager plugins that must be installed, keyed by manager name.
    #[serde(default)]
    pub plugins: IndexMap<String, String>,
    /// `"manager:package"` -> version/options (`"latest"` or a manager-native pin).
    /// String-keyed so configs using managers from newer mise versions (dnf,
    /// pacman, winget, ...) parse fine on older ones.
    #[serde(default)]
    pub packages: IndexMap<String, PackageTomlConfig>,
    /// Absolute target path -> declarative managed file.
    #[serde(default)]
    pub files: IndexMap<String, managed_files::ManagedFileTomlConfig>,
    /// Absolute target path -> declarative managed directory.
    #[serde(default)]
    pub directories: IndexMap<String, managed_files::ManagedDirectoryTomlConfig>,
    /// `"~/path"` -> git repo checkout.
    #[serde(default)]
    pub repos: IndexMap<String, repos::RepoTomlConfig>,
    /// macOS-specific bootstrap config.
    #[serde(default)]
    pub macos: BootstrapMacosTomlConfig,
    /// Linux-specific bootstrap config.
    #[serde(default)]
    pub linux: BootstrapLinuxTomlConfig,
    /// User-specific bootstrap config.
    #[serde(default)]
    pub user: BootstrapUserTomlConfig,
    /// Homebrew-specific bootstrap package config.
    #[serde(default)]
    pub brew: SystemBrewTomlConfig,
    /// apt-specific bootstrap package config.
    #[serde(default)]
    pub apt: SystemAptTomlConfig,
    /// dnf-specific bootstrap package config.
    #[serde(default)]
    pub dnf: SystemDnfTomlConfig,
    /// Shell activation setup. Values stay raw TOML so future options can warn
    /// and be skipped without rejecting the whole config.
    #[serde(default)]
    pub mise_shell_activate: IndexMap<String, toml::Value>,
    /// Bootstrap phase hooks. Values stay raw TOML so newer hook shapes can
    /// warn and be skipped without rejecting the whole config.
    #[serde(default)]
    pub hooks: IndexMap<String, toml::Value>,
}

/// The commands of a `[bootstrap.hooks]` value, in the shapes the hook parser accepts.
pub fn hook_commands(value: toml::Value) -> Option<Vec<toml::Value>> {
    match value {
        toml::Value::String(_) => Some(vec![value]),
        toml::Value::Array(values) => Some(values),
        toml::Value::Table(mut table) => match table.remove("run")? {
            run @ toml::Value::String(_) => Some(vec![run]),
            toml::Value::Array(values) => Some(values),
            _ => None,
        },
        _ => None,
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapUserTomlConfig {
    /// desired login shell for the current user, applied with `chsh -s`
    #[serde(default)]
    pub login_shell: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapMacosTomlConfig {
    /// Friendly Dock settings that compile into `[bootstrap.macos.defaults]`.
    #[serde(default)]
    pub dock: IndexMap<String, toml::Value>,
    /// Friendly Finder settings that compile into `[bootstrap.macos.defaults]`.
    #[serde(default)]
    pub finder: IndexMap<String, toml::Value>,
    /// Friendly keyboard settings that compile into `[bootstrap.macos.defaults]`.
    #[serde(default)]
    pub keyboard: IndexMap<String, toml::Value>,
    /// Friendly trackpad settings that compile into `[bootstrap.macos.defaults]`.
    #[serde(default)]
    pub trackpad: IndexMap<String, toml::Value>,
    /// `[bootstrap.macos.defaults.<domain>]` -> key -> value. Values stay raw TOML so
    /// shapes from newer mise versions (arrays, dicts) parse fine on older
    /// ones; the domain level is also raw so a malformed section warns
    /// instead of failing the whole config.
    #[serde(default)]
    pub defaults: IndexMap<String, toml::Value>,
    /// Explicit preferences with a selectable host scope.
    #[serde(default)]
    pub defaults_entries: Vec<BootstrapMacosDefaultsEntry>,
    /// `[bootstrap.macos.launchd.agents.<name>]`: declarative macOS user
    /// LaunchAgents rendered to ~/Library/LaunchAgents.
    #[serde(default)]
    pub launchd: BootstrapMacosLaunchdTomlConfig,
    /// `[bootstrap.macos.login_items]`: applications to open at login.
    #[serde(default)]
    pub login_items: Option<login_items::LoginItemsTomlConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapMacosDefaultsEntry {
    pub domain: String,
    pub key: String,
    #[serde(default)]
    pub host: HostScope,
    #[serde(default)]
    pub path: Option<Vec<String>>,
    pub value: toml::Value,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapMacosLaunchdTomlConfig {
    /// User LaunchAgents, keyed by a short stable name. mise gives these a
    /// `dev.mise.<name>` label when rendering the plist.
    #[serde(default)]
    pub agents: IndexMap<String, Templated<LaunchdTomlConfig>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapLinuxTomlConfig {
    /// Declarative Linux host firewall policy and rules.
    #[serde(default)]
    pub firewall: Option<firewall::FirewallTomlConfig>,
    /// `[bootstrap.linux.systemd.units.<name>]`: declarative systemd user
    /// services and timers rendered to ~/.config/systemd/user.
    #[serde(default)]
    pub systemd: BootstrapLinuxSystemdTomlConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapLinuxSystemdTomlConfig {
    /// User services and timers, keyed by a short stable name. mise gives
    /// these a `dev.mise.<name>.<service|timer>` unit name when rendering.
    #[serde(default)]
    pub units: IndexMap<String, Templated<SystemdTomlConfig>>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemBrewTomlConfig {
    /// Adopt identical existing app bundles for brew-cask entries by default.
    #[cfg(unix)]
    #[serde(default)]
    pub adopt: Option<bool>,
    /// `[bootstrap.brew.taps]`: `owner/tap` -> GitHub git URL. Like
    /// `[plugins]`, this lets shared config name tap remotes while package
    /// entries stay focused on formulae/casks.
    #[serde(default)]
    pub taps: IndexMap<String, String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemAptTomlConfig {
    /// `[bootstrap.apt.sources]`: source name -> PPA or repository, added
    /// before apt packages are installed.
    #[serde(default)]
    pub sources: IndexMap<String, toml::Value>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemDnfTomlConfig {
    /// `[bootstrap.dnf.repos]`: repository name -> COPR project, enabled
    /// before dnf packages are installed.
    #[serde(default)]
    pub repos: IndexMap<String, toml::Value>,
}
