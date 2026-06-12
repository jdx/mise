//! `[system]` config section: machine-global bootstrapping.
//!
//! Currently this is `[system.packages]` — declarative system packages
//! installed by `mise system install`. These are intentionally not part of
//! `[tools]`: they're unversioned, machine-global, and managed by the OS
//! package manager (or mise's own Homebrew-bottle installer), not mise's
//! per-project toolset.

use std::sync::Arc;

use eyre::bail;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::Config;
use crate::system::packages::{PackageRequest, SystemPackageManager};

pub mod packages;
pub(crate) mod sudo;

/// `[system]` as parsed from a single mise.toml
#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemTomlConfig {
    /// `"manager:package"` -> version (`"latest"` or a manager-native pin).
    /// String-keyed so configs using managers from newer mise versions (dnf,
    /// pacman, winget, ...) parse fine on older ones.
    #[serde(default)]
    pub packages: IndexMap<String, String>,
}

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

/// Split a `mise system use` spec `manager:package[@version]` into its parts.
///
/// `@version` mirrors `mise use tool@version`; `@latest` (or no `@`) means no
/// pin. brew is exempt from `@` parsing: `@` is part of brew formula *names*
/// (`postgresql@17` — that name IS brew's versioning mechanism), and brew
/// bottles can't be installed at a pinned version anyway.
pub fn parse_use_spec(spec: &str) -> eyre::Result<(String, PackageRequest)> {
    let (mgr, rest) = parse_spec(spec)?;
    if mgr == "brew" {
        return Ok((
            mgr,
            PackageRequest {
                name: rest,
                version: None,
            },
        ));
    }
    match rest.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() && !version.is_empty() => Ok((
            mgr,
            PackageRequest {
                name: name.to_string(),
                version: (version != "latest").then(|| version.to_string()),
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
            },
        )),
    }
}

/// Build [`ManagerPackages`] from already-parsed requests (used by
/// `mise system use`, where version pins come from the CLI spec). Unknown or
/// settings-excluded managers are hard errors.
pub fn packages_from_requests(
    by_mgr: IndexMap<String, Vec<PackageRequest>>,
) -> eyre::Result<Vec<ManagerPackages>> {
    resolve_managers(by_mgr, true)
}

/// Aggregate `[system.packages]` across all loaded config files.
///
/// Keys union global -> local; a more local config overrides the version pin
/// of a key the global config declared. Malformed keys and unknown managers
/// warn (forward compatibility) and are skipped. The
/// `system_packages.managers` setting restricts which managers are used at
/// all.
pub fn packages_from_config(config: &Config) -> Vec<ManagerPackages> {
    let mut merged: IndexMap<String, String> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.system_config() {
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
                by_mgr
                    .entry(mgr)
                    .or_default()
                    .push(PackageRequest { name, version });
            }
            Err(err) => warn!("[system.packages]: {err}"),
        }
    }
    resolve_managers(by_mgr, false).expect("non-strict resolve is infallible")
}

/// Build [`ManagerPackages`] from explicit CLI specs like `apt:curl`.
/// Unlike the config path, malformed specs and unknown managers are hard
/// errors. CLI specs carry no version pin — pins live in the config value.
pub fn packages_from_specs(specs: &[String]) -> eyre::Result<Vec<ManagerPackages>> {
    let mut by_mgr: IndexMap<String, Vec<PackageRequest>> = IndexMap::new();
    for spec in specs {
        let (mgr, name) = parse_spec(spec)?;
        let requests = by_mgr.entry(mgr).or_default();
        let request = PackageRequest {
            name,
            version: None,
        };
        if !requests.contains(&request) {
            requests.push(request);
        }
    }
    resolve_managers(by_mgr, true)
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
                    bail!("unknown system package manager '{name}'");
                }
                // brew is compiled out on Windows — not unknown, just
                // unsupported there
                if cfg!(windows) && name == "brew" {
                    debug!("system package manager 'brew' is not supported on windows");
                } else {
                    warn!("unknown system package manager '{name}' in [system.packages], ignoring");
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

        assert!(parse_use_spec("apt:curl@").is_err());
        assert!(parse_use_spec("noprefix").is_err());
    }
}
