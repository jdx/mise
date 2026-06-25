//! `[bootstrap]` config section: machine-global bootstrapping.
//!
//! This is `[bootstrap.packages]` — declarative system packages installed
//! by `mise bootstrap packages apply` — `[bootstrap.repos]` — declarative
//! git checkouts — `[dotfiles]` — declarative config files applied by
//! `mise bootstrap dotfiles apply` — `[bootstrap.mise_shell_activate]`
//! shell activation setup — `[bootstrap.macos.defaults]` — declarative macOS
//! user defaults — `[bootstrap.macos.launchd.agents]` — declarative macOS
//! LaunchAgents — `[bootstrap.linux.systemd.units]` — declarative Linux
//! systemd user services — `[bootstrap.user].login_shell` — and
//! `[bootstrap.hooks]` bootstrap phase hooks.
//! These are intentionally not part of `[tools]`: they're unversioned,
//! machine-global settings and resources, not mise's per-project toolset.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::bail;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::{Config, ConfigMap};
use crate::system::defaults::{DefaultsRequest, DefaultsValue};
use crate::system::launchd::{LaunchdRequest, LaunchdTomlConfig};
use crate::system::packages::{PackageRequest, SystemPackageManager};
use crate::system::repos::{RepoRequest, RepoTomlConfig};
use crate::system::shell_activation::{
    ShellActivationMode, ShellActivationRequest, ShellActivationShell, ShellActivationTarget,
};
use crate::system::systemd::{SystemdRequest, SystemdTomlConfig};

pub mod defaults;
pub mod edits;
pub mod files;
pub mod hooks;
pub mod launchd;
pub mod login_shell;
pub mod packages;
pub mod repos;
pub mod shell_activation;
pub(crate) mod sudo;
pub mod systemd;

/// `[bootstrap]` as parsed from a single mise.toml
#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapTomlConfig {
    /// `"manager:package"` -> version (`"latest"` or a manager-native pin).
    /// String-keyed so configs using managers from newer mise versions (dnf,
    /// pacman, winget, ...) parse fine on older ones.
    #[serde(default)]
    pub packages: IndexMap<String, String>,
    /// `"~/path"` -> git repo checkout.
    #[serde(default)]
    pub repos: IndexMap<String, RepoTomlConfig>,
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
    /// Shell activation setup. Values stay raw TOML so future options can warn
    /// and be skipped without rejecting the whole config.
    #[serde(default)]
    pub mise_shell_activate: IndexMap<String, toml::Value>,
    /// Bootstrap phase hooks. Values stay raw TOML so newer hook shapes can
    /// warn and be skipped without rejecting the whole config.
    #[serde(default)]
    pub hooks: IndexMap<String, toml::Value>,
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
    /// `[bootstrap.macos.launchd.agents.<name>]`: declarative macOS user
    /// LaunchAgents rendered to ~/Library/LaunchAgents.
    #[serde(default)]
    pub launchd: BootstrapMacosLaunchdTomlConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapMacosLaunchdTomlConfig {
    /// User LaunchAgents, keyed by a short stable name. mise gives these a
    /// `dev.mise.<name>` label when rendering the plist.
    #[serde(default)]
    pub agents: IndexMap<String, LaunchdTomlConfig>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapLinuxTomlConfig {
    /// `[bootstrap.linux.systemd.units.<name>]`: declarative systemd user
    /// services rendered to ~/.config/systemd/user.
    #[serde(default)]
    pub systemd: BootstrapLinuxSystemdTomlConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapLinuxSystemdTomlConfig {
    /// User services, keyed by a short stable name. mise gives these a
    /// `dev.mise.<name>.service` unit name when rendering the unit file.
    #[serde(default)]
    pub units: IndexMap<String, SystemdTomlConfig>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemBrewTomlConfig {
    /// `[bootstrap.brew.taps]`: `owner/tap` -> GitHub git URL. Like
    /// `[plugins]`, this lets shared config name tap remotes while package
    /// entries stay focused on formulae/casks.
    #[serde(default)]
    pub taps: IndexMap<String, String>,
}

/// `[dotfiles]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(transparent)]
pub struct DotfilesTomlConfig(pub IndexMap<String, toml::Value>);

/// Packages for one manager, aggregated across the config hierarchy
pub struct ManagerPackages {
    pub manager: Arc<dyn SystemPackageManager>,
    pub requests: Vec<PackageRequest>,
    /// excluded by the `system_packages.managers` setting — surfaced by
    /// status/doctor (nothing is silently invisible), skipped by install
    /// and the missing-packages hint
    pub disabled: bool,
}

/// Split a `"manager:package"` spec (config key or CLI argument). Only the
/// first `:` separates — apt arch qualifiers ("apt:gcc:arm64") and brew
/// versioned formula names ("brew:postgresql@17") stay part of the package.
pub fn parse_spec(spec: &str) -> eyre::Result<(String, String)> {
    match spec.split_once(':') {
        Some((mgr, pkg)) if !mgr.is_empty() && !pkg.is_empty() => {
            Ok((mgr.to_string(), pkg.to_string()))
        }
        _ => bail!(
            "invalid system package spec '{spec}': expected '<manager>:<package>' (e.g. \"apt:curl\")"
        ),
    }
}

/// Split a `mise bootstrap packages use` spec `manager:package[@version]` into its parts.
///
/// `@version` mirrors `mise use tool@version`; `@latest` (or no `@`) means no
/// pin. brew and brew-cask are exempt from `@` parsing: `@` is part of
/// Homebrew names (`postgresql@17` — that name IS brew's versioning
/// mechanism), and bottles/casks can't be installed at a pinned version
/// anyway. mas uses numeric ADAM IDs only.
pub fn parse_use_spec(spec: &str) -> eyre::Result<(String, PackageRequest)> {
    let (mgr, rest) = parse_spec(spec)?;
    let rest = normalize_use_spec_package_name(&mgr, &rest)?;
    validate_package_name(&mgr, rest)?;
    if is_opaque_package_manager(&mgr) {
        return Ok((
            mgr,
            PackageRequest {
                name: rest.to_string(),
                version: None,
                tap_url: None,
            },
        ));
    }
    match rest.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => Ok((
            mgr,
            PackageRequest {
                name: name.to_string(),
                version: (version != "latest").then(|| version.to_string()),
                tap_url: None,
            },
        )),
        Some(_) => {
            bail!("invalid system package spec '{spec}': expected '<manager>:<package>[@version]'")
        }
        None => Ok((
            mgr,
            PackageRequest {
                name: rest.to_string(),
                version: None,
                tap_url: None,
            },
        )),
    }
}

/// Build [`ManagerPackages`] from already-parsed requests (used by
/// `mise bootstrap packages use`, where version pins come from the CLI spec). Unknown or
/// settings-excluded managers are hard errors.
pub fn packages_from_requests(
    by_mgr: IndexMap<String, Vec<PackageRequest>>,
) -> eyre::Result<Vec<ManagerPackages>> {
    resolve_managers(by_mgr, true)
}

pub fn attach_brew_tap_urls(config: &Config, by_mgr: &mut IndexMap<String, Vec<PackageRequest>>) {
    let brew_taps = brew_taps_from_config(config);
    for mgr in ["brew", "brew-cask"] {
        if let Some(requests) = by_mgr.get_mut(mgr) {
            for request in requests {
                request.tap_url =
                    brew_tap_name(&request.name).and_then(|tap| brew_taps.get(tap).cloned());
            }
        }
    }
}

/// Aggregate `[bootstrap.packages]` across all loaded config files.
///
/// Keys union global -> local; a more local config overrides the version pin
/// of a key the global config declared. Malformed keys and unknown managers
/// warn (forward compatibility) and are skipped. The
/// `system_packages.managers` setting restricts which managers are used at
/// all.
pub fn packages_from_config(config: &Config) -> Vec<ManagerPackages> {
    let brew_taps = brew_taps_from_config(config);
    packages_from_config_files_with_brew_taps(&config.config_files, &brew_taps)
}

/// Aggregate `[bootstrap.packages]` across a specific set of config files.
pub fn packages_from_config_files(config_files: &ConfigMap) -> Vec<ManagerPackages> {
    packages_from_config_files_with_brew_taps(config_files, &IndexMap::new())
}

fn packages_from_config_files_with_brew_taps(
    config_files: &ConfigMap,
    brew_taps: &IndexMap<String, String>,
) -> Vec<ManagerPackages> {
    let mut merged: IndexMap<String, String> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (spec, version) in sys.packages {
                merged.insert(spec, version);
            }
        }
    }
    let mut by_mgr: IndexMap<String, Vec<PackageRequest>> = IndexMap::new();
    for (spec, version) in merged {
        match parse_spec(&spec) {
            Ok((mgr, name)) => {
                let version = (version != "latest").then_some(version);
                if let Err(err) = validate_package_name(&mgr, &name) {
                    warn!("[bootstrap.packages]: {err}");
                    continue;
                }
                let tap_url = if is_brew_manager(&mgr) {
                    brew_tap_name(&name).and_then(|tap| brew_taps.get(tap).cloned())
                } else {
                    None
                };
                by_mgr.entry(mgr).or_default().push(PackageRequest {
                    name,
                    version,
                    tap_url,
                });
            }
            Err(err) => warn!("[bootstrap.packages]: {err}"),
        }
    }
    resolve_managers(by_mgr, false).expect("non-strict resolve is infallible")
}

/// Aggregate `[bootstrap.macos.defaults]` across all loaded config files.
///
/// (domain, key) pairs union global -> local; a more local config overrides
/// the value a global config declared. Unsupported value shapes warn
/// (forward compatibility) and are skipped.
pub fn defaults_from_config(config: &Config) -> Vec<DefaultsRequest> {
    let mut merged: IndexMap<(String, String), toml::Value> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            let mut friendly: IndexMap<(String, String), toml::Value> = IndexMap::new();
            let mut raw: IndexMap<(String, String), toml::Value> = IndexMap::new();
            merge_friendly_macos_defaults(&mut friendly, &sys.macos);
            for (domain, entries) in sys.macos.defaults {
                match entries {
                    toml::Value::Table(entries) => {
                        for (key, value) in entries {
                            raw.insert((domain.clone(), key), value);
                        }
                    }
                    _ => warn!(
                        "[bootstrap.macos.defaults]: expected a table of key/value pairs for domain '{domain}'"
                    ),
                }
            }
            for (key, value) in merge_raw_over_friendly_macos_defaults(friendly, raw) {
                merged.insert(key, value);
            }
        }
    }
    let mut out = vec![];
    for ((domain, key), value) in merged {
        match DefaultsValue::from_toml(&value) {
            Some(value) => out.push(DefaultsRequest { domain, key, value }),
            None => warn!(
                "[bootstrap.macos.defaults]: unsupported value type for {domain} {key} \
                 (expected bool, integer, float, or string)"
            ),
        }
    }
    out
}

/// Aggregate `[bootstrap.macos.launchd.agents]` across all loaded config files.
///
/// Agent names union global -> local; a more local config replaces the full
/// agent declaration from a global config. Invalid entries warn and are
/// skipped.
pub fn launchd_from_config(config: &Config) -> Vec<LaunchdRequest> {
    let mut merged: IndexMap<String, LaunchdTomlConfig> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (name, agent) in sys.macos.launchd.agents {
                merged.insert(name, agent);
            }
        }
    }
    let mut out = vec![];
    for (name, agent) in merged {
        match LaunchdRequest::from_toml(name, agent) {
            Ok(request) => out.push(request),
            Err(err) => warn!("[bootstrap.macos.launchd.agents]: {err}"),
        }
    }
    out
}

/// Aggregate `[bootstrap.repos]` across all loaded config files.
///
/// Repo paths union global -> local; a more local config replaces the full
/// repo declaration for the same expanded path. Invalid entries warn and are
/// skipped.
pub fn repos_from_config(config: &Config) -> Vec<RepoRequest> {
    let mut merged: IndexMap<PathBuf, RepoRequest> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (path_raw, repo) in sys.repos {
                match RepoRequest::from_toml(path_raw.clone(), repo) {
                    Ok(request) => {
                        merged.insert(request.path.clone(), request);
                    }
                    Err(err) => warn!("[bootstrap.repos].\"{path_raw}\": {err}"),
                }
            }
        }
    }
    merged.into_values().collect()
}

/// Count macOS defaults declared in one config file, including friendly
/// sections that compile into raw defaults entries.
pub fn macos_defaults_entry_count(macos: &BootstrapMacosTomlConfig) -> usize {
    let mut friendly: IndexMap<(String, String), toml::Value> = IndexMap::new();
    let mut raw: IndexMap<(String, String), toml::Value> = IndexMap::new();
    let mut malformed_domains = 0usize;
    merge_friendly_macos_defaults(&mut friendly, macos);
    for (domain, entries) in &macos.defaults {
        match entries {
            toml::Value::Table(entries) => {
                for (key, value) in entries {
                    raw.insert((domain.clone(), key.clone()), value.clone());
                }
            }
            _ => malformed_domains += 1,
        }
    }
    merge_raw_over_friendly_macos_defaults(friendly, raw).len() + malformed_domains
}

fn merge_raw_over_friendly_macos_defaults(
    mut friendly: IndexMap<(String, String), toml::Value>,
    raw: IndexMap<(String, String), toml::Value>,
) -> IndexMap<(String, String), toml::Value> {
    for (key, value) in raw {
        friendly.insert(key, value);
    }
    friendly
}

fn merge_friendly_macos_defaults(
    out: &mut IndexMap<(String, String), toml::Value>,
    macos: &BootstrapMacosTomlConfig,
) {
    merge_dock_defaults(out, &macos.dock);
    merge_finder_defaults(out, &macos.finder);
    merge_keyboard_defaults(out, &macos.keyboard);
    merge_trackpad_defaults(out, &macos.trackpad);
}

#[derive(Clone, Copy)]
struct FriendlyDefaultSpec<'a> {
    section: &'a str,
    key: &'a str,
    defaults_key: &'a str,
    expected: fn(&toml::Value) -> bool,
    expected_type: &'a str,
}

fn insert_friendly_default(
    out: &mut IndexMap<(String, String), toml::Value>,
    domain: &str,
    spec: FriendlyDefaultSpec<'_>,
    value: toml::Value,
) {
    if (spec.expected)(&value) {
        out.insert((domain.to_string(), spec.defaults_key.to_string()), value);
    } else {
        let FriendlyDefaultSpec {
            section,
            key,
            expected_type,
            ..
        } = spec;
        warn!(
            "[bootstrap.macos.{section}].{key}: unsupported value type \
             (expected {expected_type})"
        );
    }
}

fn insert_friendly_multi_domain_default(
    out: &mut IndexMap<(String, String), toml::Value>,
    domains: &[&str],
    spec: FriendlyDefaultSpec<'_>,
    value: toml::Value,
) {
    if (spec.expected)(&value) {
        for domain in domains {
            out.insert(
                (domain.to_string(), spec.defaults_key.to_string()),
                value.clone(),
            );
        }
    } else {
        let FriendlyDefaultSpec {
            section,
            key,
            expected_type,
            ..
        } = spec;
        warn!(
            "[bootstrap.macos.{section}].{key}: unsupported value type \
             (expected {expected_type})"
        );
    }
}

fn is_bool(value: &toml::Value) -> bool {
    matches!(value, toml::Value::Boolean(_))
}

fn is_integer(value: &toml::Value) -> bool {
    matches!(value, toml::Value::Integer(_))
}

fn merge_dock_defaults(
    out: &mut IndexMap<(String, String), toml::Value>,
    entries: &IndexMap<String, toml::Value>,
) {
    for (key, value) in entries {
        match key.as_str() {
            "autohide" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "autohide",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "orientation" => match value {
                toml::Value::String(s) if matches!(s.as_str(), "bottom" | "left" | "right") => {
                    out.insert(
                        ("com.apple.dock".to_string(), "orientation".to_string()),
                        value.clone(),
                    );
                }
                toml::Value::String(_) => warn!(
                    "[bootstrap.macos.dock].orientation: invalid value \
                     (expected bottom, left, or right)"
                ),
                _ => warn!(
                    "[bootstrap.macos.dock].orientation: unsupported value type (expected string)"
                ),
            },
            "tilesize" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "tilesize",
                    expected: is_integer,
                    expected_type: "integer",
                },
                value.clone(),
            ),
            "magnification" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "magnification",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "largesize" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "largesize",
                    expected: is_integer,
                    expected_type: "integer",
                },
                value.clone(),
            ),
            "show_recents" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "show-recents",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "mru_spaces" => insert_friendly_default(
                out,
                "com.apple.dock",
                FriendlyDefaultSpec {
                    section: "dock",
                    key,
                    defaults_key: "mru-spaces",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            _ => warn!("[bootstrap.macos.dock].{key}: unknown key, ignoring entry"),
        }
    }
}

fn merge_finder_defaults(
    out: &mut IndexMap<(String, String), toml::Value>,
    entries: &IndexMap<String, toml::Value>,
) {
    for (key, value) in entries {
        match key.as_str() {
            "show_all_files" => insert_friendly_default(
                out,
                "com.apple.finder",
                FriendlyDefaultSpec {
                    section: "finder",
                    key,
                    defaults_key: "AppleShowAllFiles",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "show_pathbar" => insert_friendly_default(
                out,
                "com.apple.finder",
                FriendlyDefaultSpec {
                    section: "finder",
                    key,
                    defaults_key: "ShowPathbar",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "show_status_bar" => insert_friendly_default(
                out,
                "com.apple.finder",
                FriendlyDefaultSpec {
                    section: "finder",
                    key,
                    defaults_key: "ShowStatusBar",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "show_extensions_warning" => insert_friendly_default(
                out,
                "com.apple.finder",
                FriendlyDefaultSpec {
                    section: "finder",
                    key,
                    defaults_key: "FXEnableExtensionChangeWarning",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "preferred_view_style" => match value {
                toml::Value::String(s) => {
                    let mapped = match s.as_str() {
                        "icon" => Some("icnv"),
                        "list" => Some("Nlsv"),
                        "column" => Some("clmv"),
                        "gallery" => Some("glyv"),
                        _ => None,
                    };
                    if let Some(mapped) = mapped {
                        out.insert(
                            (
                                "com.apple.finder".to_string(),
                                "FXPreferredViewStyle".to_string(),
                            ),
                            toml::Value::String(mapped.to_string()),
                        );
                    } else {
                        warn!(
                            "[bootstrap.macos.finder].preferred_view_style: invalid value \
                             (expected icon, list, column, or gallery)"
                        );
                    }
                }
                _ => warn!(
                    "[bootstrap.macos.finder].preferred_view_style: unsupported value type \
                     (expected string)"
                ),
            },
            _ => warn!("[bootstrap.macos.finder].{key}: unknown key, ignoring entry"),
        }
    }
}

fn merge_keyboard_defaults(
    out: &mut IndexMap<(String, String), toml::Value>,
    entries: &IndexMap<String, toml::Value>,
) {
    for (key, value) in entries {
        match key.as_str() {
            "key_repeat" => insert_friendly_default(
                out,
                "NSGlobalDomain",
                FriendlyDefaultSpec {
                    section: "keyboard",
                    key,
                    defaults_key: "KeyRepeat",
                    expected: is_integer,
                    expected_type: "integer",
                },
                value.clone(),
            ),
            "initial_key_repeat" => insert_friendly_default(
                out,
                "NSGlobalDomain",
                FriendlyDefaultSpec {
                    section: "keyboard",
                    key,
                    defaults_key: "InitialKeyRepeat",
                    expected: is_integer,
                    expected_type: "integer",
                },
                value.clone(),
            ),
            "press_and_hold" => insert_friendly_default(
                out,
                "NSGlobalDomain",
                FriendlyDefaultSpec {
                    section: "keyboard",
                    key,
                    defaults_key: "ApplePressAndHoldEnabled",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "fn_state" => insert_friendly_default(
                out,
                "NSGlobalDomain",
                FriendlyDefaultSpec {
                    section: "keyboard",
                    key,
                    defaults_key: "com.apple.keyboard.fnState",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            _ => warn!("[bootstrap.macos.keyboard].{key}: unknown key, ignoring entry"),
        }
    }
}

fn merge_trackpad_defaults(
    out: &mut IndexMap<(String, String), toml::Value>,
    entries: &IndexMap<String, toml::Value>,
) {
    for (key, value) in entries {
        match key.as_str() {
            "tap_to_click" => insert_friendly_multi_domain_default(
                out,
                &[
                    "com.apple.AppleMultitouchTrackpad",
                    "com.apple.driver.AppleBluetoothMultitouch.trackpad",
                ],
                FriendlyDefaultSpec {
                    section: "trackpad",
                    key,
                    defaults_key: "Clicking",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            "three_finger_drag" => insert_friendly_multi_domain_default(
                out,
                &[
                    "com.apple.AppleMultitouchTrackpad",
                    "com.apple.driver.AppleBluetoothMultitouch.trackpad",
                ],
                FriendlyDefaultSpec {
                    section: "trackpad",
                    key,
                    defaults_key: "TrackpadThreeFingerDrag",
                    expected: is_bool,
                    expected_type: "bool",
                },
                value.clone(),
            ),
            _ => warn!("[bootstrap.macos.trackpad].{key}: unknown key, ignoring entry"),
        }
    }
}

/// Aggregate `[bootstrap.linux.systemd.units]` across all loaded config files.
///
/// Unit names union global -> local; a more local config replaces the full
/// unit declaration from a global config. Invalid entries warn and are skipped.
pub fn systemd_from_config(config: &Config) -> Vec<SystemdRequest> {
    let mut merged: IndexMap<String, SystemdTomlConfig> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (name, unit) in sys.linux.systemd.units {
                merged.insert(name, unit);
            }
        }
    }
    let mut out = vec![];
    for (name, unit) in merged {
        match SystemdRequest::from_toml(name, unit) {
            Ok(request) => out.push(request),
            Err(err) => warn!("[bootstrap.linux.systemd.units]: {err}"),
        }
    }
    out
}

/// Desired login shell from the most local config that declares it.
pub fn login_shell_from_config(config: &Config) -> Option<login_shell::LoginShellRequest> {
    let mut shell = None;
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config()
            && let Some(login_shell) = sys.user.login_shell
        {
            let login_shell = login_shell.trim().to_string();
            if login_shell.is_empty() {
                warn!("[bootstrap.user].login_shell: must not be empty, ignoring entry");
                continue;
            }
            if !Path::new(&login_shell).is_absolute() {
                warn!(
                    "[bootstrap.user].login_shell: shell must be an absolute path, ignoring entry"
                );
                continue;
            }
            shell = Some(login_shell);
        }
    }
    shell.map(|shell| login_shell::LoginShellRequest { shell })
}

/// Aggregate `[bootstrap.mise_shell_activate]` across all loaded config files.
///
/// Target keys merge global -> local, with local config overriding broader
/// config. Shell keys are shortcuts that expand to that shell's default target
/// files before target-specific keys in the same config are applied. Explicit
/// `[dotfiles]` edits for the same rc file/id win over the generated shell
/// activation edit.
pub fn shell_activation_from_config(config: &Config) -> Vec<ShellActivationRequest> {
    shell_activation_from_config_files(&config.config_files)
}

pub fn shell_activation_from_config_files(config_files: &ConfigMap) -> Vec<ShellActivationRequest> {
    let explicit_files = files::files_from_config_files(config_files);
    let explicit_edits = edits::edits_from_config_files(config_files);
    let mut merged: IndexMap<ShellActivationTarget, Option<ShellActivationMode>> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            let mut shell_entries = vec![];
            let mut target_entries = vec![];
            for (key, value) in sys.mise_shell_activate {
                if let Some(shell) = ShellActivationShell::parse(&key) {
                    shell_entries.push((key, shell, value));
                } else if let Some(target) = ShellActivationTarget::parse(&key) {
                    target_entries.push((key, target, value));
                } else {
                    warn!(
                        "[bootstrap.mise_shell_activate]: unknown target '{key}' \
                         (expected {}), ignoring entry",
                        ShellActivationTarget::expected_keys()
                    );
                    continue;
                };
            }
            for (key, shell, value) in shell_entries {
                match shell_activation_setting(&key, value) {
                    Some(setting) => {
                        for &target in shell.default_targets() {
                            merge_shell_activation_target(&mut merged, target, setting);
                        }
                    }
                    None => {
                        warn_shell_activation_invalid_shell_retains_broader(&merged, &key, shell);
                    }
                }
            }
            for (key, target, value) in target_entries {
                match shell_activation_setting(&key, value) {
                    Some(setting) => {
                        merge_shell_activation_target(&mut merged, target, setting);
                    }
                    None => {
                        warn_shell_activation_invalid_target_retains_broader(&merged, &key, target);
                    }
                }
            }
        }
    }
    merged
        .into_iter()
        .filter_map(|(target, mode)| {
            mode.and_then(|mode| {
                let request = ShellActivationRequest::new(target, mode);
                if explicit_files
                    .iter()
                    .any(|file| file.target == request.edit.path)
                {
                    debug!(
                        "bootstrap: shell activation for {} skipped because [dotfiles] owns {}",
                        target.name(),
                        request.edit.path_raw
                    );
                    return None;
                }
                if explicit_edits
                    .iter()
                    .any(|edit| edit.path == request.edit.path && edit.id == request.edit.id)
                {
                    debug!(
                        "bootstrap: shell activation for {} skipped because [dotfiles] owns {}/{}",
                        target.name(),
                        request.edit.path_raw,
                        request.edit.id
                    );
                    None
                } else {
                    Some(request)
                }
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ShellActivationSetting {
    enabled: bool,
    mode: Option<ShellActivationMode>,
}

fn merge_shell_activation_target(
    merged: &mut IndexMap<ShellActivationTarget, Option<ShellActivationMode>>,
    target: ShellActivationTarget,
    setting: ShellActivationSetting,
) {
    let mode = setting
        .enabled
        .then(|| setting.mode.unwrap_or_else(|| target.default_mode()));
    merged.insert(target, mode);
}

fn shell_activation_setting(key: &str, value: toml::Value) -> Option<ShellActivationSetting> {
    match value {
        toml::Value::Boolean(enabled) => Some(ShellActivationSetting {
            enabled,
            mode: None,
        }),
        toml::Value::String(mode) => {
            let Some(mode) = ShellActivationMode::parse(&mode) else {
                warn!(
                    "[bootstrap.mise_shell_activate.{key}]: expected \"activate\" or \"shims\", \
                     ignoring entry"
                );
                return None;
            };
            Some(ShellActivationSetting {
                enabled: true,
                mode: Some(mode),
            })
        }
        toml::Value::Table(table) => {
            let unknown = table
                .keys()
                .filter(|key| !matches!(key.as_str(), "enabled" | "mode"))
                .cloned()
                .collect::<Vec<_>>();
            if !unknown.is_empty() {
                warn!(
                    "[bootstrap.mise_shell_activate.{key}]: unknown field(s) {}, ignoring entry",
                    unknown.join(", ")
                );
                return None;
            }
            let enabled = match table.get("enabled") {
                Some(toml::Value::Boolean(enabled)) => *enabled,
                Some(_) => {
                    warn!(
                        "[bootstrap.mise_shell_activate.{key}].enabled: expected bool, \
                         ignoring entry"
                    );
                    return None;
                }
                None => {
                    warn!("[bootstrap.mise_shell_activate.{key}]: missing enabled, ignoring entry");
                    return None;
                }
            };
            let mode = match table.get("mode") {
                Some(toml::Value::String(mode)) => {
                    let Some(mode) = ShellActivationMode::parse(mode) else {
                        warn!(
                            "[bootstrap.mise_shell_activate.{key}].mode: expected \"activate\" or \
                             \"shims\", ignoring entry"
                        );
                        return None;
                    };
                    Some(mode)
                }
                Some(_) => {
                    warn!(
                        "[bootstrap.mise_shell_activate.{key}].mode: expected string, ignoring entry"
                    );
                    return None;
                }
                None => None,
            };
            Some(ShellActivationSetting { enabled, mode })
        }
        _ => {
            warn!(
                "[bootstrap.mise_shell_activate.{key}]: expected bool, \"activate\", \"shims\", \
                 or table, ignoring entry"
            );
            None
        }
    }
}

fn warn_shell_activation_invalid_shell_retains_broader(
    merged: &IndexMap<ShellActivationTarget, Option<ShellActivationMode>>,
    key: &str,
    shell: ShellActivationShell,
) {
    let retained = shell
        .default_targets()
        .iter()
        .filter_map(|target| {
            merged.get(target).map(|mode| {
                format!(
                    "{}={}",
                    target.name(),
                    shell_activation_setting_display(*mode)
                )
            })
        })
        .collect::<Vec<_>>();
    if !retained.is_empty() {
        warn!(
            "[bootstrap.mise_shell_activate.{key}]: invalid entry ignored; keeping broader config \
             values {}",
            retained.join(", ")
        );
    }
}

fn warn_shell_activation_invalid_target_retains_broader(
    merged: &IndexMap<ShellActivationTarget, Option<ShellActivationMode>>,
    key: &str,
    target: ShellActivationTarget,
) {
    if let Some(mode) = merged.get(&target) {
        warn!(
            "[bootstrap.mise_shell_activate.{key}]: invalid entry ignored; keeping broader config \
             value {}",
            shell_activation_setting_display(*mode)
        );
    }
}

fn shell_activation_setting_display(setting: Option<ShellActivationMode>) -> &'static str {
    match setting {
        Some(ShellActivationMode::Activate) => "activate",
        Some(ShellActivationMode::Shims) => "shims",
        None => "disabled",
    }
}

/// Aggregate `[bootstrap.hooks]` across all loaded config files.
///
/// Hooks are additive and ordered global -> local. A hook value can be a string
/// command, an array of string commands, or a table with a `run` string/array.
pub fn hooks_from_config(config: &Config) -> Vec<hooks::BootstrapHook> {
    hooks_from_config_files(&config.config_files)
}

pub(crate) fn hooks_from_config_files(config_files: &ConfigMap) -> Vec<hooks::BootstrapHook> {
    let mut out = vec![];
    for cf in config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (phase, value) in sys.hooks {
                match hooks::BootstrapHook::from_toml(&phase, value) {
                    Ok(hooks) => out.extend(hooks),
                    Err(err) => warn!("[bootstrap.hooks.{phase}]: {err}"),
                }
            }
        }
    }
    out
}

/// Build [`ManagerPackages`] from explicit CLI specs, attaching configured
/// brew tap URLs when a config is available.
///
/// Unlike the config path, malformed specs and unknown managers are hard
/// errors. CLI specs carry no version pin — pins live in the config value.
pub fn packages_from_specs_with_config(
    specs: &[String],
    config: Option<&Config>,
) -> eyre::Result<Vec<ManagerPackages>> {
    let brew_taps = config.map(brew_taps_from_config).unwrap_or_default();
    let mut by_mgr: IndexMap<String, Vec<PackageRequest>> = IndexMap::new();
    for spec in specs {
        let (mgr, name) = parse_spec(spec)?;
        validate_package_name(&mgr, &name)?;
        let tap_url = if is_brew_manager(&mgr) {
            brew_tap_name(&name).and_then(|tap| brew_taps.get(tap).cloned())
        } else {
            None
        };
        let requests = by_mgr.entry(mgr).or_default();
        let request = PackageRequest {
            name,
            version: None,
            tap_url,
        };
        if !requests.contains(&request) {
            requests.push(request);
        }
    }
    resolve_managers(by_mgr, true)
}

pub(crate) fn brew_tap_name(name: &str) -> Option<&str> {
    let mut parts = name.split('/');
    let owner = parts.next()?;
    let tap = parts.next()?;
    let formula = parts.next()?;
    if parts.next().is_some() || owner.is_empty() || tap.is_empty() || formula.is_empty() {
        return None;
    }
    if owner == "homebrew" && (tap == "core" || tap == "cask") {
        None
    } else {
        name.rsplit_once('/').map(|(tap, _)| tap)
    }
}

fn is_brew_manager(mgr: &str) -> bool {
    matches!(mgr, "brew" | "brew-cask")
}

fn is_opaque_package_manager(mgr: &str) -> bool {
    is_brew_manager(mgr) || mgr == "mas"
}

fn normalize_use_spec_package_name<'a>(mgr: &str, name: &'a str) -> eyre::Result<&'a str> {
    if mgr == "mas"
        && let Some(name) = name.strip_suffix("@latest")
    {
        if name.is_empty() {
            bail!("invalid system package spec: expected '<manager>:<package>[@version]'");
        }
        return Ok(name);
    }
    Ok(name)
}

fn validate_package_name(mgr: &str, name: &str) -> eyre::Result<()> {
    if mgr == "mas" && !packages::mas::is_adam_id(name) {
        bail!("mas app IDs must be numeric ADAM IDs (e.g. \"mas:497799835\")");
    }
    Ok(())
}

pub(crate) fn brew_taps_from_config(config: &Config) -> IndexMap<String, String> {
    let mut brew_taps: IndexMap<String, String> = IndexMap::new();
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.bootstrap_config() {
            for (tap, url) in sys.brew.taps {
                brew_taps.insert(tap, url);
            }
        }
    }
    brew_taps
}

fn resolve_managers(
    by_mgr: IndexMap<String, Vec<PackageRequest>>,
    strict: bool,
) -> eyre::Result<Vec<ManagerPackages>> {
    let enabled = crate::config::Settings::get()
        .system_packages
        .managers
        .clone();
    let mut out = vec![];
    for (name, requests) in by_mgr {
        let disabled = enabled.as_ref().is_some_and(|e| !e.contains(&name));
        if disabled && strict {
            bail!(
                "manager '{name}' is excluded by the system_packages.managers setting \
                 (currently: {})",
                enabled.as_deref().unwrap_or_default().join(", ")
            );
        }
        match packages::get_manager(&name) {
            Some(manager) => out.push(ManagerPackages {
                manager,
                requests,
                disabled,
            }),
            None => {
                if strict {
                    bail!("unknown bootstrap package manager '{name}'");
                }
                // brew is compiled out on Windows — not unknown, just
                // unsupported there
                if cfg!(windows) && name == "brew" {
                    debug!("system package manager 'brew' is not supported on windows");
                } else {
                    warn!(
                        "unknown bootstrap package manager '{name}' in [bootstrap.packages], ignoring"
                    );
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tv(s: &str) -> toml::Value {
        s.parse().unwrap()
    }

    #[test]
    fn test_parse_use_spec() {
        let (mgr, req) = parse_use_spec("apt:curl").unwrap();
        assert_eq!(
            (mgr.as_str(), req.name.as_str(), req.version),
            ("apt", "curl", None)
        );

        let (mgr, req) = parse_use_spec("apt:curl@8.5.0-2").unwrap();
        assert_eq!(mgr, "apt");
        assert_eq!(req.name, "curl");
        assert_eq!(req.version.as_deref(), Some("8.5.0-2"));

        // @latest is the same as no pin
        let (_, req) = parse_use_spec("dnf:bash@latest").unwrap();
        assert_eq!(req.version, None);

        let (mgr, req) = parse_use_spec("apk:zlib-dev@1.3.1-r2").unwrap();
        assert_eq!(mgr, "apk");
        assert_eq!(req.name, "zlib-dev");
        assert_eq!(req.version.as_deref(), Some("1.3.1-r2"));

        // apt arch qualifiers stay in the name
        let (_, req) = parse_use_spec("apt:gcc:arm64@13.2").unwrap();
        assert_eq!(req.name, "gcc:arm64");
        assert_eq!(req.version.as_deref(), Some("13.2"));

        // brew formula names contain '@' — never treated as a version
        let (mgr, req) = parse_use_spec("brew:postgresql@17").unwrap();
        assert_eq!(mgr, "brew");
        assert_eq!(req.name, "postgresql@17");
        assert_eq!(req.version, None);

        let (mgr, req) = parse_use_spec("brew-cask:temurin@17").unwrap();
        assert_eq!(mgr, "brew-cask");
        assert_eq!(req.name, "temurin@17");
        assert_eq!(req.version, None);

        let (mgr, req) = parse_use_spec("mas:497799835").unwrap();
        assert_eq!(mgr, "mas");
        assert_eq!(req.name, "497799835");
        assert_eq!(req.version, None);

        let (mgr, req) = parse_use_spec("mas:497799835@latest").unwrap();
        assert_eq!(mgr, "mas");
        assert_eq!(req.name, "497799835");
        assert_eq!(req.version, None);

        assert!(parse_use_spec("mas:com.example.App").is_err());
        assert!(parse_use_spec("mas:497799835@1").is_err());

        assert!(parse_use_spec("apt:curl@").is_err());
        assert!(parse_use_spec("noprefix").is_err());
    }

    #[test]
    fn test_brew_tap_name() {
        assert_eq!(
            brew_tap_name("railwaycat/emacsmacport/emacs-mac"),
            Some("railwaycat/emacsmacport")
        );
        assert_eq!(brew_tap_name("homebrew/core/jq"), None);
        assert_eq!(brew_tap_name("homebrew/cask/firefox"), None);
        assert_eq!(brew_tap_name("jq"), None);
        assert_eq!(brew_tap_name("too/many/slashes/here"), None);
    }

    #[test]
    fn test_repo_request_validation() {
        let request = repos::RepoRequest::from_toml(
            "~/src/dotfiles".to_string(),
            repos::RepoTomlConfig {
                url: Some("https://github.com/jdx/dotfiles.git".to_string()),
                git_ref: Some("main".to_string()),
            },
        )
        .unwrap();
        assert!(request.path.is_absolute());
        assert_eq!(request.path_raw, "~/src/dotfiles");
        assert_eq!(request.url, "https://github.com/jdx/dotfiles.git");
        assert_eq!(request.git_ref.as_deref(), Some("main"));
        assert!(repos::RepoRequest::from_toml("relative".to_string(), Default::default()).is_err());
    }

    #[test]
    fn test_friendly_macos_defaults() {
        let mut macos = BootstrapMacosTomlConfig::default();
        macos.dock.insert("autohide".into(), tv("true"));
        macos.dock.insert("orientation".into(), tv(r#""left""#));
        macos.dock.insert("tilesize".into(), tv("48"));
        macos.dock.insert("magnification".into(), tv("true"));
        macos.dock.insert("largesize".into(), tv("96"));
        macos.dock.insert("show_recents".into(), tv("false"));
        macos.dock.insert("mru_spaces".into(), tv("false"));
        macos.finder.insert("show_all_files".into(), tv("true"));
        macos.finder.insert("show_pathbar".into(), tv("true"));
        macos.finder.insert("show_status_bar".into(), tv("true"));
        macos
            .finder
            .insert("show_extensions_warning".into(), tv("false"));
        macos
            .finder
            .insert("preferred_view_style".into(), tv(r#""list""#));
        macos.keyboard.insert("key_repeat".into(), tv("2"));
        macos.keyboard.insert("initial_key_repeat".into(), tv("15"));
        macos.keyboard.insert("press_and_hold".into(), tv("false"));
        macos.keyboard.insert("fn_state".into(), tv("true"));
        macos.trackpad.insert("tap_to_click".into(), tv("true"));
        macos
            .trackpad
            .insert("three_finger_drag".into(), tv("true"));

        let mut out = IndexMap::new();
        merge_friendly_macos_defaults(&mut out, &macos);

        assert_eq!(
            out.get(&("com.apple.dock".into(), "autohide".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "orientation".into())),
            Some(&tv(r#""left""#))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "tilesize".into())),
            Some(&tv("48"))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "magnification".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "largesize".into())),
            Some(&tv("96"))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "show-recents".into())),
            Some(&tv("false"))
        );
        assert_eq!(
            out.get(&("com.apple.dock".into(), "mru-spaces".into())),
            Some(&tv("false"))
        );
        assert_eq!(
            out.get(&("com.apple.finder".into(), "AppleShowAllFiles".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&("com.apple.finder".into(), "ShowPathbar".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&("com.apple.finder".into(), "ShowStatusBar".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&(
                "com.apple.finder".into(),
                "FXEnableExtensionChangeWarning".into()
            )),
            Some(&tv("false"))
        );
        assert_eq!(
            out.get(&("com.apple.finder".into(), "FXPreferredViewStyle".into())),
            Some(&tv(r#""Nlsv""#))
        );
        assert_eq!(
            out.get(&("NSGlobalDomain".into(), "KeyRepeat".into())),
            Some(&tv("2"))
        );
        assert_eq!(
            out.get(&("NSGlobalDomain".into(), "InitialKeyRepeat".into())),
            Some(&tv("15"))
        );
        assert_eq!(
            out.get(&("NSGlobalDomain".into(), "ApplePressAndHoldEnabled".into())),
            Some(&tv("false"))
        );
        assert_eq!(
            out.get(&("NSGlobalDomain".into(), "com.apple.keyboard.fnState".into())),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&(
                "com.apple.AppleMultitouchTrackpad".into(),
                "Clicking".into()
            )),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&(
                "com.apple.driver.AppleBluetoothMultitouch.trackpad".into(),
                "Clicking".into()
            )),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&(
                "com.apple.AppleMultitouchTrackpad".into(),
                "TrackpadThreeFingerDrag".into()
            )),
            Some(&tv("true"))
        );
        assert_eq!(
            out.get(&(
                "com.apple.driver.AppleBluetoothMultitouch.trackpad".into(),
                "TrackpadThreeFingerDrag".into()
            )),
            Some(&tv("true"))
        );
    }

    #[test]
    fn test_friendly_macos_defaults_validation() {
        let mut macos = BootstrapMacosTomlConfig::default();
        macos.dock.insert("orientation".into(), tv(r#""top""#));
        macos.dock.insert("tilesize".into(), tv("true"));
        macos.dock.insert("unknown".into(), tv("true"));
        macos
            .finder
            .insert("preferred_view_style".into(), tv(r#""coverflow""#));
        macos.keyboard.insert("key_repeat".into(), tv(r#""fast""#));
        macos.trackpad.insert("tap_to_click".into(), tv("[true]"));

        let mut out = IndexMap::new();
        merge_friendly_macos_defaults(&mut out, &macos);

        assert!(out.is_empty());
    }

    #[test]
    fn test_raw_macos_defaults_override_friendly_defaults() {
        let mut friendly = IndexMap::new();
        friendly.insert(
            ("com.apple.dock".into(), "autohide".into()),
            toml::Value::Boolean(true),
        );
        friendly.insert(
            ("com.apple.dock".into(), "tilesize".into()),
            toml::Value::Integer(48),
        );
        let mut raw = IndexMap::new();
        raw.insert(
            ("com.apple.dock".into(), "autohide".into()),
            toml::Value::Boolean(false),
        );

        let merged = merge_raw_over_friendly_macos_defaults(friendly, raw);

        assert_eq!(
            merged.get(&("com.apple.dock".into(), "autohide".into())),
            Some(&toml::Value::Boolean(false))
        );
        assert_eq!(
            merged.get(&("com.apple.dock".into(), "tilesize".into())),
            Some(&toml::Value::Integer(48))
        );
    }

    #[test]
    fn test_shell_activation_setting_bool_string_and_table_forms() {
        assert_eq!(
            shell_activation_setting("zshrc", tv("true")),
            Some(ShellActivationSetting {
                enabled: true,
                mode: None
            })
        );
        assert_eq!(
            shell_activation_setting("bashrc", tv("false")),
            Some(ShellActivationSetting {
                enabled: false,
                mode: None
            })
        );
        assert_eq!(
            shell_activation_setting("zprofile", tv(r#""shims""#)),
            Some(ShellActivationSetting {
                enabled: true,
                mode: Some(ShellActivationMode::Shims)
            })
        );
        assert_eq!(
            shell_activation_setting("fish", tv(r#"{enabled = true, mode = "activate"}"#)),
            Some(ShellActivationSetting {
                enabled: true,
                mode: Some(ShellActivationMode::Activate)
            })
        );
        assert_eq!(
            shell_activation_setting("fish", tv("{enabled = false}")),
            Some(ShellActivationSetting {
                enabled: false,
                mode: None
            })
        );
    }

    #[test]
    fn test_shell_activation_setting_skips_invalid_options() {
        assert_eq!(
            shell_activation_setting("zshrc", tv(r#"{enabled = true, prompt = true}"#)),
            None
        );
        assert_eq!(shell_activation_setting("zshrc", tv("{}")), None);
        assert_eq!(shell_activation_setting("zshrc", tv(r#""yes""#)), None);
        assert_eq!(
            shell_activation_setting("zshrc", tv(r#"{enabled = true, mode = "yes"}"#)),
            None
        );
    }

    #[test]
    fn test_local_friendly_macos_defaults_override_global_raw_defaults() {
        let mut merged = IndexMap::new();

        let global_friendly = IndexMap::new();
        let mut global_raw = IndexMap::new();
        global_raw.insert(
            ("com.apple.dock".into(), "autohide".into()),
            toml::Value::Boolean(false),
        );
        for (key, value) in merge_raw_over_friendly_macos_defaults(global_friendly, global_raw) {
            merged.insert(key, value);
        }

        let mut local_friendly = IndexMap::new();
        local_friendly.insert(
            ("com.apple.dock".into(), "autohide".into()),
            toml::Value::Boolean(true),
        );
        let local_raw = IndexMap::new();
        for (key, value) in merge_raw_over_friendly_macos_defaults(local_friendly, local_raw) {
            merged.insert(key, value);
        }

        assert_eq!(
            merged.get(&("com.apple.dock".into(), "autohide".into())),
            Some(&toml::Value::Boolean(true))
        );
    }

    #[test]
    fn test_macos_defaults_entry_count_includes_friendly_defaults() {
        let mut macos = BootstrapMacosTomlConfig::default();
        macos.dock.insert("autohide".into(), tv("true"));
        macos.trackpad.insert("tap_to_click".into(), tv("true"));
        macos.defaults.insert(
            "com.apple.dock".into(),
            tv(r#"{ autohide = false, tilesize = 48 }"#),
        );
        macos.defaults.insert("malformed".into(), tv("true"));

        assert_eq!(macos_defaults_entry_count(&macos), 5);
    }
}
