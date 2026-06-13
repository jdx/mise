//! `[bootstrap]` config section: machine-global bootstrapping.
//!
//! This is `[bootstrap.packages]` — declarative system packages installed
//! by `mise bootstrap packages install` — `[dotfiles]` — declarative config
//! files applied by `mise dotfiles apply` — `[bootstrap.macos.defaults]`
//! — declarative macOS user defaults — and `[bootstrap.user].login_shell`.
//! These are intentionally not part of `[tools]`: they're unversioned,
//! machine-global settings and resources, not mise's per-project toolset.

use std::path::Path;
use std::sync::Arc;

use eyre::bail;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::Config;
use crate::config::ConfigMap;
use crate::system::defaults::{DefaultsRequest, DefaultsValue};
use crate::system::launchd::{LaunchdRequest, LaunchdTomlConfig};
use crate::system::packages::{PackageRequest, SystemPackageManager};

pub mod defaults;
pub mod edits;
pub mod files;
pub mod launchd;
pub mod login_shell;
pub mod packages;
pub(crate) mod sudo;

/// `[bootstrap]` as parsed from a single mise.toml
#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapTomlConfig {
    /// `"manager:package"` -> version (`"latest"` or a manager-native pin).
    /// String-keyed so configs using managers from newer mise versions (dnf,
    /// pacman, winget, ...) parse fine on older ones.
    #[serde(default)]
    pub packages: IndexMap<String, String>,
    /// macOS-specific bootstrap config.
    #[serde(default)]
    pub macos: BootstrapMacosTomlConfig,
    /// User-specific bootstrap config.
    #[serde(default)]
    pub user: BootstrapUserTomlConfig,
    /// Homebrew-specific bootstrap package config.
    #[serde(default)]
    pub brew: SystemBrewTomlConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapUserTomlConfig {
    /// desired login shell for the current user, applied with `chsh -s`
    #[serde(default)]
    pub login_shell: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct BootstrapMacosTomlConfig {
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
/// anyway.
pub fn parse_use_spec(spec: &str) -> eyre::Result<(String, PackageRequest)> {
    let (mgr, rest) = parse_spec(spec)?;
    if is_brew_manager(&mgr) {
        return Ok((
            mgr,
            PackageRequest {
                name: rest,
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
                name: rest,
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
            for (domain, entries) in sys.macos.defaults {
                match entries {
                    toml::Value::Table(entries) => {
                        for (key, value) in entries {
                            merged.insert((domain.clone(), key), value);
                        }
                    }
                    _ => warn!(
                        "[bootstrap.macos.defaults]: expected a table of key/value pairs for domain '{domain}'"
                    ),
                }
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

fn brew_taps_from_config(config: &Config) -> IndexMap<String, String> {
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
}
