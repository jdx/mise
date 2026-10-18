mod decl;
pub use decl::*;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::defaults::HostScope;
use crate::launchd::LaunchdTomlConfig;
use crate::schema::refs;
use crate::systemd::SystemdTomlConfig;
use crate::{
    accounts, compose, firewall, login_items, managed_files, packages::PackageTomlConfig, remote,
    repos, secrets, services_common as services, templating::Templated,
};

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// machine-global bootstrapping (system packages, repos, macOS defaults, launchd agents, login shell)
// `[bootstrap]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapTomlConfig {
    /// Managed system files keyed by absolute target path
    #[serde(default)]
    pub files: IndexMap<String, managed_files::ManagedFileTomlConfig>,
    /// Managed system directories keyed by absolute target path
    #[serde(default)]
    pub directories: IndexMap<String, managed_files::ManagedDirectoryTomlConfig>,
    /// the dotfile groups this machine applies; when unset, every group applies. Entries without a group always apply, and a more local config's list replaces the others
    #[schemars(inner(regex(pattern = r"^[A-Za-z0-9_.-]+$")))]
    pub dotfile_groups: Option<Vec<String>>,
    /// deprecated and will be removed in mise 2027.3.3; move each selected root into a conf.d folder instead (https://mise.jdx.dev/configuration.html#conf-d-folders)
    // Independent configuration roots whose declarative files and dotfiles participate in
    // bootstrap composition.
    #[schemars(extend("deprecated" = true))]
    pub config_roots: Option<Vec<String>>,
    /// OpenSSH targets used by `mise bootstrap remote`
    #[serde(default)]
    pub remote: remote::RemoteTomlConfig,
    /// System services and portable user services managed by mise bootstrap, keyed by service name
    #[serde(default)]
    pub services: IndexMap<String, services::ServiceTomlConfig>,
    /// Docker Compose projects managed with `mise bootstrap compose`, keyed by project name. Text fields support Tera templates.
    #[serde(default)]
    pub compose: IndexMap<String, Templated<compose::ComposeTomlConfig>>,
    /// package manager plugins to install with `mise bootstrap plugins apply`, keyed by manager name
    #[serde(default)]
    #[schemars(schema_with = "plugin_urls")]
    pub plugins: IndexMap<String, String>,
    /// system packages to install with `mise bootstrap packages apply`, keyed by "manager:package" (e.g. "apt:libssl-dev", "brew:postgresql@17", "dnf:openssl-devel", "pacman:base-devel")
    // String-keyed so configs using managers from newer mise versions (dnf, pacman, winget, ...)
    // parse fine on older ones.
    #[serde(default)]
    #[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z0-9_-]+:.+$"}))]
    pub packages: IndexMap<String, PackageTomlConfig>,
    /// git repositories to clone or update with `mise bootstrap repos apply`, keyed by target path
    #[serde(default)]
    pub repos: IndexMap<String, repos::RepoTomlConfig>,
    /// macOS-specific bootstrap config
    #[serde(default)]
    pub macos: BootstrapMacosTomlConfig,
    /// Linux-specific bootstrap config
    #[serde(default)]
    pub linux: BootstrapLinuxTomlConfig,
    /// current-user bootstrap settings
    #[serde(default)]
    pub user: BootstrapUserTomlConfig,
    /// apt-specific bootstrap package config
    #[serde(default)]
    pub apt: SystemAptTomlConfig,
    /// dnf-specific bootstrap package config
    #[serde(default)]
    pub dnf: SystemDnfTomlConfig,
    /// Homebrew-specific bootstrap package config
    #[serde(default)]
    pub brew: SystemBrewTomlConfig,
    // Bootstrap phase hooks. Values stay raw TOML so newer hook shapes can warn and be skipped
    // without rejecting the whole config, which is also why their schema is written by hand.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_hooks")]
    pub hooks: IndexMap<String, toml::Value>,
    // Shell activation setup. Values stay raw TOML so future options can warn and be skipped
    // without rejecting the whole config.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_mise_shell_activate")]
    pub mise_shell_activate: IndexMap<String, toml::Value>,
    /// sensitive bootstrap inputs read from environment variables, keyed by secret name
    #[serde(default)]
    #[schemars(extend("propertyNames" = {
        "pattern": "^[A-Za-z0-9_.-]+$",
        "description": "secret name using ASCII letters, digits, '.', '_' or '-'"
    }))]
    pub secrets: IndexMap<String, secrets::SecretTomlConfig>,
    /// [experimental] Linux groups that bootstrap creates or removes, keyed by group name
    #[serde(default)]
    #[schemars(extend("propertyNames" = {
        "pattern": "^[A-Za-z_][A-Za-z0-9_-]*\\$?$",
        "maxLength": 32,
        "description": "at most 32 ASCII letters, digits, '_' or '-', starting with a letter or '_', with an optional trailing '$'"
    }))]
    pub groups: IndexMap<String, accounts::GroupTomlConfig>,
    /// [experimental] Linux users that bootstrap creates or removes, keyed by user name
    #[serde(default)]
    #[schemars(extend("propertyNames" = {
        "pattern": "^[A-Za-z_][A-Za-z0-9_-]*\\$?$",
        "maxLength": 32,
        "description": "at most 32 ASCII letters, digits, '_' or '-', starting with a letter or '_', with an optional trailing '$'"
    }))]
    pub users: IndexMap<String, accounts::UserTomlConfig>,
}

fn plugin_urls(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {"type": "string", "description": "plugin git URL"}
    })
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

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapUserTomlConfig {
    /// current user's desired login shell, applied with `chsh -s`; a template that must render to an absolute path
    // The value is rendered and trimmed before the absolute-path check, so the schema also takes
    // surrounding whitespace and a leading template.
    #[serde(default)]
    #[schemars(regex(pattern = r"^\s*(?:/|\{[{%])"))]
    pub login_shell: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapMacosTomlConfig {
    // The curated tables compile into `[bootstrap.macos.defaults]`. They and `defaults` stay raw
    // TOML so shapes from newer mise versions parse fine on older ones, and a malformed domain
    // warns instead of failing the whole config.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_dock")]
    pub dock: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_finder")]
    pub finder: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_keyboard")]
    pub keyboard: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_trackpad")]
    pub trackpad: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_defaults")]
    pub defaults: IndexMap<String, toml::Value>,
    /// Explicit macOS preferences with a selectable host scope
    #[serde(default)]
    pub defaults_entries: Vec<BootstrapMacosDefaultsEntry>,
    /// macOS launchd bootstrap config
    #[serde(default)]
    pub launchd: BootstrapMacosLaunchdTomlConfig,
    /// applications to open at login (System Settings > General > Login Items & Extensions)
    #[serde(default)]
    pub login_items: Option<login_items::LoginItemsTomlConfig>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BootstrapMacosDefaultsEntry {
    /// preferences domain, such as "com.apple.dock" or "NSGlobalDomain"; -g and -globalDomain mean NSGlobalDomain
    pub domain: String,
    /// preference key within the domain
    pub key: String,
    /// any writes the preference for every host; current writes it for this machine only, like defaults -currentHost
    #[serde(default)]
    pub host: HostScope,
    /// Dictionary keys below the preference key; omitted to replace the whole value
    #[serde(default)]
    #[schemars(length(min = 1))]
    pub path: Option<Vec<String>>,
    /// desired value, written with the matching property list type; replaces the selected value in full
    #[schemars(schema_with = "defaults_value")]
    pub value: toml::Value,
}

fn defaults_value(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "anyOf": [
            {"type": "boolean"},
            {"type": "number"},
            {"type": "string"},
            {"type": "array"},
            {"type": "object"}
        ]
    })
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapMacosLaunchdTomlConfig {
    /// macOS user LaunchAgents to write and load with `mise bootstrap macos launchd-agents apply`
    // Keyed by a short stable name. mise gives these a `dev.mise.<name>` label when rendering
    // the plist.
    #[serde(default)]
    #[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z0-9._-]+$"}))]
    pub agents: IndexMap<String, Templated<LaunchdTomlConfig>>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapLinuxTomlConfig {
    /// Linux host firewall policy and rules managed with `mise bootstrap firewall`
    #[serde(default)]
    pub firewall: Option<firewall::FirewallTomlConfig>,
    /// systemd user service and timer bootstrap config
    #[serde(default)]
    pub systemd: BootstrapLinuxSystemdTomlConfig,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapLinuxSystemdTomlConfig {
    /// systemd user services and timers to write and start with `mise bootstrap linux systemd-units apply`
    // Keyed by a short stable name. mise gives these a `dev.mise.<name>.<service|timer>` unit
    // name when rendering.
    #[serde(default)]
    #[schemars(extend("propertyNames" = {"pattern": "^[A-Za-z0-9._@-]+$"}))]
    pub units: IndexMap<String, Templated<SystemdTomlConfig>>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct SystemBrewTomlConfig {
    /// adopt existing app artifacts by default for configured brew-cask packages
    // Parsed on every platform so a config shared with Windows, where brew does not run, is not
    // reported as having an unknown field.
    #[serde(default)]
    pub adopt: Option<bool>,
    /// Homebrew tap names mapped to custom git URLs
    // Like `[plugins]`, this lets shared config name tap remotes while package entries stay
    // focused on formulae and casks.
    #[serde(default)]
    #[schemars(extend("additionalProperties" = {"type": "string", "description": "git URL of the tap's GitHub repository, used instead of https://github.com/<owner>/homebrew-<tap>.git"}))]
    pub taps: IndexMap<String, String>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct SystemAptTomlConfig {
    // `[bootstrap.apt.sources]`: source name -> PPA or repository, added before apt packages are
    // installed. Raw TOML, parsed by mise-system.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_apt_sources")]
    pub sources: IndexMap<String, toml::Value>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct SystemDnfTomlConfig {
    // `[bootstrap.dnf.repos]`: repository name -> COPR project, enabled before dnf packages are
    // installed. Raw TOML, parsed by mise-system.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_dnf_repos")]
    pub repos: IndexMap<String, toml::Value>,
}
