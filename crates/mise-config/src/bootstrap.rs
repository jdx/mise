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
    /// system packages to install with `mise bootstrap packages apply`, keyed by "manager:package", or by a bare package name that mise installs with one manager chosen from system_packages.prefer (e.g. "apt:libssl-dev", "brew:postgresql@17", "dnf:openssl-devel", "pacman:base-devel")
    // String-keyed so configs using managers from newer mise versions (dnf, pacman, winget, ...)
    // parse fine on older ones.
    #[serde(default)]
    #[schemars(extend("propertyNames" = {"pattern": "^([A-Za-z0-9_-]+:.+|[^:\\s]+)$"}))]
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
    /// [experimental] settings that bootstrap settings plugins apply, keyed by plugin name; which keys a plugin accepts is up to the plugin
    // Raw tables: the plugin, not mise, knows their keys. mise flattens them to dotted keys.
    #[serde(default)]
    #[schemars(schema_with = "plugin_settings")]
    pub settings: IndexMap<String, toml::Table>,
}

fn plugin_settings(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let macos = generator.subschema_for::<MacosSettingsTomlConfig>();
    schemars::json_schema!({
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z0-9_.-]+$"},
        "properties": {"macos": macos},
        "additionalProperties": {
            "type": "object",
            "description": "settings for the bootstrap settings plugin of this name"
        }
    })
}

/// macOS preferences, written the way `defaults write` writes them; `macos` is built in, not a plugin
// The preference keys of `BootstrapMacosTomlConfig`, under their new home. `move_macos_settings`
// moves them back there, so the rest of mise reads one place.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct MacosSettingsTomlConfig {
    /// Dock preferences (com.apple.dock)
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_dock")]
    pub dock: IndexMap<String, toml::Value>,
    /// Finder preferences (com.apple.finder)
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_finder")]
    pub finder: IndexMap<String, toml::Value>,
    /// keyboard preferences
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_keyboard")]
    pub keyboard: IndexMap<String, toml::Value>,
    /// trackpad preferences
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_trackpad")]
    pub trackpad: IndexMap<String, toml::Value>,
    /// any preference, by domain and key
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_defaults")]
    pub defaults: IndexMap<String, toml::Value>,
    /// Explicit macOS preferences with a selectable host scope
    #[serde(default)]
    pub defaults_entries: Vec<BootstrapMacosDefaultsEntry>,
    /// applications to open at login (System Settings > General > Login Items & Extensions)
    #[serde(default)]
    #[schemars(with = "Option<login_items::LoginItemsTomlConfig>")]
    pub login_items: Option<MacosLoginItemsSettings>,
}

/// `[bootstrap.settings.macos.login_items]` as written, so the move can tell an omitted `apps`
/// from an explicitly empty one.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct MacosLoginItemsSettings {
    #[serde(default)]
    pub apps: Option<Vec<String>>,
    #[serde(default)]
    pub exclusive: Option<bool>,
}

/// What [`BootstrapTomlConfig::move_macos_settings`] found.
#[derive(Debug, Default)]
pub struct MacosSettingsMove {
    /// Preferences were set in the deprecated `[bootstrap.macos]` tables.
    pub legacy: bool,
    /// Keys under `[bootstrap.settings.macos]` that mean nothing, as dotted paths.
    pub unknown: Vec<String>,
    /// `[bootstrap.settings.macos]` could not be read; the config must be rejected.
    pub error: Option<String>,
}

impl BootstrapTomlConfig {
    /// Moves `[bootstrap.settings.macos]` into the preference tables of `[bootstrap.macos]`, where
    /// the rest of mise reads them. A key set in both places takes the new spelling's value.
    pub fn move_macos_settings(&mut self) -> MacosSettingsMove {
        let macos = &mut self.macos;
        let mut outcome = MacosSettingsMove {
            legacy: !(macos.dock.is_empty()
                && macos.finder.is_empty()
                && macos.keyboard.is_empty()
                && macos.trackpad.is_empty()
                && macos.defaults.is_empty()
                && macos.defaults_entries.is_empty()
                && macos.login_items.is_none()),
            ..Default::default()
        };
        // `Macos` and `MACOS` name the same section as `macos`, as they would for a plugin, so
        // none of them may be left behind to be dispatched as a settings plugin.
        let mut table: Option<toml::Table> = None;
        for name in self
            .settings
            .keys()
            .filter(|name| plugin_identity(name) == "macos")
            .cloned()
            .collect::<Vec<_>>()
        {
            let section = self.settings.shift_remove(&name).unwrap_or_default();
            match &mut table {
                Some(table) => merge_tables(table, section),
                None => table = Some(section),
            }
        }
        let Some(table) = table else {
            return outcome;
        };
        let parsed: Result<MacosSettingsTomlConfig, _> =
            serde_ignored::deserialize(toml::Value::Table(table), |path| {
                outcome.unknown.push(path.to_string())
            });
        let new = match parsed {
            Ok(new) => new,
            Err(err) => {
                outcome.error = Some(err.to_string());
                return outcome;
            }
        };
        macos.dock.extend(new.dock);
        macos.finder.extend(new.finder);
        macos.keyboard.extend(new.keyboard);
        macos.trackpad.extend(new.trackpad);
        for (domain, value) in new.defaults {
            // `-g` and `NSGlobalDomain` are one domain, read in order with the later value
            // winning, so a key the new spelling sets must not survive under another alias.
            if let toml::Value::Table(keys) = &value {
                for (other, old) in macos.defaults.iter_mut() {
                    if *other != domain
                        && defaults_domain_identity(other) == defaults_domain_identity(&domain)
                        && let toml::Value::Table(old) = old
                    {
                        old.retain(|key, _| !keys.contains_key(key));
                    }
                }
            }
            match (macos.defaults.get_mut(&domain), value) {
                (Some(toml::Value::Table(old)), toml::Value::Table(new)) => old.extend(new),
                (_, value) => {
                    macos.defaults.insert(domain, value);
                }
            }
        }
        macos.defaults_entries.extend(new.defaults_entries);
        // Merge field by field, so `exclusive = true` alone in the new spelling keeps the apps
        // the old spelling declares, while an explicit `apps = []` clears them.
        if let Some(new) = new.login_items {
            let merged = macos.login_items.get_or_insert_with(Default::default);
            if let Some(apps) = new.apps {
                merged.apps = apps;
            }
            if new.exclusive.is_some() {
                merged.exclusive = new.exclusive;
            }
        }
        outcome
    }
}

/// The identity of a plugin name: `foo_bar`, `foo-bar` and `FooBar` are the same plugin, which
/// mise installs under the kebab-case name.
pub fn plugin_identity(name: &str) -> String {
    heck::ToKebabCase::to_kebab_case(name)
}

/// Merges `higher` into `base` key by key, `higher` winning.
fn merge_tables(base: &mut toml::Table, higher: toml::Table) {
    for (key, value) in higher {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(base)), toml::Value::Table(higher)) => {
                merge_tables(base, higher)
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// The domain `defaults` reads for `domain`: `-g` and `-globalDomain` name `NSGlobalDomain`.
fn defaults_domain_identity(domain: &str) -> &str {
    match domain {
        "-g" | "-globalDomain" => "NSGlobalDomain",
        domain => domain,
    }
}

fn plugin_urls(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": {"type": "string", "description": "plugin git URL"}
    })
}

fn command_list(value: toml::Value) -> Option<Vec<toml::Value>> {
    match value {
        toml::Value::String(_) => Some(vec![value]),
        toml::Value::Array(values) => Some(values),
        _ => None,
    }
}

/// The commands of a `[bootstrap.hooks]` value, in the shapes the hook parser accepts: the
/// commands for every platform, and those of a table's `run_windows`, which replace them on Windows.
pub fn hook_commands(value: toml::Value) -> Option<(Vec<toml::Value>, Option<Vec<toml::Value>>)> {
    // a malformed value stays in the list, so the hook parser reports it, and only where it applies
    let list = |value: toml::Value| command_list(value.clone()).unwrap_or_else(|| vec![value]);
    match value {
        toml::Value::Table(mut table) => {
            let windows = table.remove("run_windows").map(list);
            let run = match table.remove("run") {
                Some(run) => list(run),
                None if windows.is_some() => vec![],
                None => return None,
            };
            Some((run, windows))
        }
        value => Some((command_list(value)?, None)),
    }
}

/// Hook commands `shared` runs before `own`, keeping each side's Windows commands apart.
pub fn merge_hook_commands(
    (mut run, mut windows): (Vec<toml::Value>, Option<Vec<toml::Value>>),
    (own_run, own_windows): (Vec<toml::Value>, Option<Vec<toml::Value>>),
) -> toml::Value {
    if windows.is_none() && own_windows.is_none() {
        run.extend(own_run);
        return toml::Value::Array(run);
    }
    // a side without `run_windows` runs its `run` commands there too
    let mut merged_windows = windows.take().unwrap_or_else(|| run.clone());
    merged_windows.extend(own_windows.unwrap_or_else(|| own_run.clone()));
    run.extend(own_run);
    let mut table = toml::map::Map::new();
    table.insert("run".into(), toml::Value::Array(run));
    table.insert("run_windows".into(), toml::Value::Array(merged_windows));
    toml::Value::Table(table)
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapUserTomlConfig {
    /// current user's desired login shell, applied with `chsh -s`; an absolute path or a bare shell name resolved after packages are installed; may be a template
    // The value is rendered and trimmed before the check, so the schema also takes
    // surrounding whitespace and a leading template; a relative path with a separator is rejected.
    #[serde(default)]
    #[schemars(regex(pattern = r"^\s*(?:/|\{[{%]|[^/\\{\s][^/\\]*$)"))]
    pub login_shell: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct BootstrapMacosTomlConfig {
    // The curated tables compile into `[bootstrap.macos.defaults]`. They and `defaults` stay raw
    // TOML so shapes from newer mise versions parse fine on older ones, and a malformed domain
    // warns instead of failing the whole config.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_dock", extend("deprecated" = true))]
    pub dock: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_finder", extend("deprecated" = true))]
    pub finder: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_keyboard", extend("deprecated" = true))]
    pub keyboard: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_trackpad", extend("deprecated" = true))]
    pub trackpad: IndexMap<String, toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_macos_defaults", extend("deprecated" = true))]
    pub defaults: IndexMap<String, toml::Value>,
    /// deprecated: use [[bootstrap.settings.macos.defaults_entries]]
    #[serde(default)]
    #[schemars(extend("deprecated" = true))]
    pub defaults_entries: Vec<BootstrapMacosDefaultsEntry>,
    /// macOS launchd bootstrap config
    #[serde(default)]
    pub launchd: BootstrapMacosLaunchdTomlConfig,
    /// deprecated: use [bootstrap.settings.macos.login_items]
    #[serde(default)]
    #[schemars(extend("deprecated" = true))]
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
    /// write Homebrew's cask metadata after mise installs a brew-cask package, so `brew upgrade` and `brew outdated` manage it afterwards
    // Parsed on every platform for the same reason as `adopt`.
    #[serde(default)]
    pub homebrew_metadata: Option<bool>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(shared: &str, own: &str) -> toml::Value {
        let parse = |raw: &str| {
            let table: toml::Table = raw.parse().unwrap();
            hook_commands(table["hook"].clone()).unwrap()
        };
        merge_hook_commands(parse(shared), parse(own))
    }

    #[test]
    fn merged_hooks_keep_each_sides_windows_commands() {
        let value = merged(
            r#"hook = { run = "echo unix", run_windows = "echo win" }"#,
            r#"hook = "echo own""#,
        );
        assert_eq!(
            value["run"].as_array().unwrap(),
            &["echo unix".into(), "echo own".into()]
        );
        assert_eq!(
            value["run_windows"].as_array().unwrap(),
            &["echo win".into(), "echo own".into()]
        );

        let value = merged(
            r#"hook = { run_windows = ["echo win"] }"#,
            r#"hook = { run = "echo own" }"#,
        );
        assert_eq!(value["run"].as_array().unwrap(), &["echo own".into()]);
        assert_eq!(
            value["run_windows"].as_array().unwrap(),
            &["echo win".into(), "echo own".into()]
        );
    }

    #[test]
    fn merged_hooks_without_windows_commands_stay_a_plain_array() {
        let value = merged(r#"hook = "echo a""#, r#"hook = { run = ["echo b"] }"#);
        assert_eq!(
            value.as_array().unwrap(),
            &["echo a".into(), "echo b".into()]
        );
    }

    #[test]
    fn merged_hooks_keep_malformed_windows_commands_for_the_parser() {
        let value = merged(
            r#"hook = { run = "echo unix", run_windows = 1 }"#,
            r#"hook = "echo own""#,
        );
        assert_eq!(
            value["run_windows"].as_array().unwrap(),
            &[toml::Value::Integer(1), "echo own".into()]
        );
    }

    #[test]
    fn merged_hooks_keep_windows_commands_beside_a_malformed_run() {
        let value = merged(
            r#"hook = { run = 1, run_windows = "echo win" }"#,
            r#"hook = "echo own""#,
        );
        assert_eq!(
            value["run_windows"].as_array().unwrap(),
            &["echo win".into(), "echo own".into()]
        );
        assert_eq!(value["run"].as_array().unwrap()[0], toml::Value::Integer(1));
    }
}

#[cfg(test)]
mod macos_settings_tests {
    use super::*;

    fn bootstrap(toml: &str) -> BootstrapTomlConfig {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn new_spelling_moves_into_the_preference_tables() {
        let mut config = bootstrap(
            r#"
            [settings.macos.dock]
            autohide = true
            [settings.macos.defaults."com.apple.screencapture"]
            type = "png"
            [settings.other]
            key = 1
            "#,
        );
        let moved = config.move_macos_settings();
        assert!(!moved.legacy);
        assert!(moved.unknown.is_empty());
        assert_eq!(config.macos.dock["autohide"], toml::Value::Boolean(true));
        assert_eq!(
            config.macos.defaults["com.apple.screencapture"]["type"],
            toml::Value::String("png".into())
        );
        assert!(!config.settings.contains_key("macos"));
        assert!(config.settings.contains_key("other"));
    }

    #[test]
    fn every_spelling_of_macos_is_the_macos_section() {
        let mut config = bootstrap(
            r#"
            [settings.Macos.dock]
            autohide = true
            tilesize = 48
            [settings.MACOS.finder]
            show_all_files = true
            [settings.macos.dock]
            tilesize = 64
            "#,
        );
        let moved = config.move_macos_settings();
        assert!(moved.error.is_none());
        assert!(config.settings.is_empty());
        assert_eq!(config.macos.dock["autohide"], toml::Value::Boolean(true));
        assert_eq!(config.macos.dock["tilesize"], toml::Value::Integer(64));
        assert_eq!(
            config.macos.finder["show_all_files"],
            toml::Value::Boolean(true)
        );
    }

    #[test]
    fn new_spelling_wins_over_every_alias_of_a_domain() {
        let mut config = bootstrap(
            r#"
            [macos.defaults.-g]
            SomeKey = false
            Other = 1
            [macos.defaults.NSGlobalDomain]
            SomeKey = false
            [settings.macos.defaults.-g]
            SomeKey = true
            "#,
        );
        config.move_macos_settings();
        let defaults = &config.macos.defaults;
        assert_eq!(defaults["-g"]["SomeKey"], toml::Value::Boolean(true));
        assert_eq!(defaults["-g"]["Other"], toml::Value::Integer(1));
        assert!(defaults["NSGlobalDomain"].get("SomeKey").is_none());
    }

    #[test]
    fn new_spelling_wins_over_the_deprecated_one() {
        let mut config = bootstrap(
            r#"
            [macos.dock]
            autohide = false
            tilesize = 48
            [macos.defaults."com.apple.screencapture"]
            type = "jpg"
            disable-shadow = true
            [settings.macos.dock]
            autohide = true
            [settings.macos.defaults."com.apple.screencapture"]
            type = "png"
            [settings.macos.typo]
            x = 1
            "#,
        );
        let moved = config.move_macos_settings();
        assert!(moved.legacy);
        assert_eq!(moved.unknown, vec!["typo".to_string()]);
        assert_eq!(config.macos.dock["autohide"], toml::Value::Boolean(true));
        assert_eq!(config.macos.dock["tilesize"], toml::Value::Integer(48));
        let shots = &config.macos.defaults["com.apple.screencapture"];
        assert_eq!(shots["type"], toml::Value::String("png".into()));
        assert_eq!(shots["disable-shadow"], toml::Value::Boolean(true));
    }

    #[test]
    fn login_items_merge_field_by_field() {
        let mut config = bootstrap(
            r#"
            [macos.login_items]
            apps = ["/Applications/Rectangle.app"]
            [settings.macos.login_items]
            exclusive = true
            "#,
        );
        config.move_macos_settings();
        let items = config.macos.login_items.unwrap();
        assert_eq!(items.apps, vec!["/Applications/Rectangle.app".to_string()]);
        assert_eq!(items.exclusive, Some(true));

        let mut config = bootstrap(
            r#"
            [macos.login_items]
            apps = ["/Applications/Rectangle.app"]
            [settings.macos.login_items]
            apps = []
            "#,
        );
        config.move_macos_settings();
        assert!(config.macos.login_items.unwrap().apps.is_empty());
    }
}
