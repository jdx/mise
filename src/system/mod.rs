//! `[system]` config section: machine-global bootstrapping.
//!
//! Currently this is `[system.packages]` — declarative system packages
//! installed by `mise system install`. These are intentionally not part of
//! `[tools]`: they're unversioned, machine-global, and managed by the OS
//! package manager (or mise's own Homebrew-bottle installer), not mise's
//! per-project toolset.

use std::sync::Arc;

use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::Config;
use crate::system::packages::{PackageRequest, SystemPackageManager};

pub mod packages;
pub(crate) mod sudo;

/// `[system]` as parsed from a single mise.toml
#[derive(Debug, Default, Clone, Deserialize)]
pub struct SystemTomlConfig {
    /// manager name -> package strings. String-keyed so configs using
    /// managers from newer mise versions (dnf, pacman, winget, ...) parse
    /// fine on older ones.
    #[serde(default)]
    pub packages: IndexMap<String, Vec<String>>,
}

/// Packages for one manager, aggregated across the config hierarchy
pub struct ManagerPackages {
    pub manager: Arc<dyn SystemPackageManager>,
    pub requests: Vec<PackageRequest>,
}

/// Aggregate `[system.packages]` across all loaded config files.
///
/// Additive union, global -> local, deduped preserving first-seen order: a
/// project config can add requirements on top of the global ones but not
/// remove them. Unknown managers warn (forward compatibility) and are
/// skipped. The `system_packages.managers` setting restricts which managers
/// are used at all.
pub fn packages_from_config(config: &Config) -> Vec<ManagerPackages> {
    let enabled = crate::config::Settings::get()
        .system_packages
        .managers
        .clone();
    let mut raw: IndexMap<String, Vec<String>> = IndexMap::new();
    // config_files is ordered local -> global; reverse for global -> local
    for cf in config.config_files.values().rev() {
        if let Some(sys) = cf.system_config() {
            for (mgr, pkgs) in sys.packages {
                let entry = raw.entry(mgr).or_default();
                for p in pkgs {
                    if !entry.contains(&p) {
                        entry.push(p);
                    }
                }
            }
        }
    }
    raw.into_iter()
        .filter_map(|(name, raws)| {
            if let Some(enabled) = &enabled
                && !enabled.contains(&name)
            {
                debug!("system package manager '{name}' disabled by system_packages.managers");
                return None;
            }
            match packages::get_manager(&name) {
                Some(manager) => {
                    let requests = raws.iter().map(|r| manager.parse_request(r)).collect();
                    Some(ManagerPackages { manager, requests })
                }
                None => {
                    // brew is compiled out on Windows — not unknown, just
                    // unsupported there
                    if cfg!(windows) && name == "brew" {
                        debug!("system package manager 'brew' is not supported on windows");
                    } else {
                        warn!(
                            "unknown system package manager '{name}' in [system.packages], ignoring"
                        );
                    }
                    None
                }
            }
        })
        .collect()
}
