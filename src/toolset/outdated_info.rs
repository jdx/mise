use crate::semver::{chunkify_version, split_version_prefix};
use crate::toolset;
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource, ToolVersion};
use crate::{Result, config::Config};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fmt::{Display, Formatter},
    path::PathBuf,
    sync::Arc,
};
use tabled::Tabled;
use versions::Version;

#[derive(Debug, Serialize, Clone, Tabled, PartialEq, Eq, Hash)]
pub struct OutdatedInfo {
    pub name: String,
    #[serde(skip)]
    #[tabled(skip)]
    pub tool_request: ToolRequest,
    #[serde(skip)]
    #[tabled(skip)]
    pub tool_version: ToolVersion,
    pub requested: String,
    #[tabled(display("Self::display_current"))]
    pub current: Option<String>,
    #[tabled(display("Self::display_bump"))]
    pub bump: Option<String>,
    pub latest: String,
    pub source: ToolSource,
}

impl OutdatedInfo {
    pub fn new(config: &Arc<Config>, tv: ToolVersion, latest: String) -> Result<Self> {
        let t = tv.backend()?;
        let current = if t.is_version_installed(config, &tv, true) {
            Some(tv.version.clone())
        } else {
            None
        };
        let oi = Self {
            source: tv.request.source().clone(),
            name: tv.ba().short.to_string(),
            current,
            requested: tv.request.version(),
            tool_request: tv.request.clone(),
            tool_version: tv,
            bump: None,
            latest,
        };
        Ok(oi)
    }

    pub async fn resolve(
        config: &Arc<Config>,
        tv: ToolVersion,
        bump: bool,
        opts: &ResolveOptions,
    ) -> eyre::Result<Option<Self>> {
        let t = tv.backend()?;
        // prefix is something like "temurin-" or "corretto-"
        let (prefix, prefix_version) = split_version_prefix(&tv.request.version());
        let use_backend_latest =
            bump || (opts.inactive && tv.request.source() == &ToolSource::Unknown);

        let latest_result = if use_backend_latest {
            let prefix = prefixed_latest_query(&prefix, &prefix_version);
            // For bumps and installed-but-inactive tools (`--no-source`), use backend latest.
            t.latest_version(config, prefix, opts.before_date).await
        } else {
            tv.latest_version_with_opts(config, opts)
                .await
                .map(Option::from)
        };
        let latest = match latest_result {
            Ok(Some(latest)) => latest,
            Ok(None) => {
                warn!("Error getting latest version for {t}: no latest version found");
                return Ok(None);
            }
            Err(e) => {
                warn!("Error getting latest version for {t}: {e:#}");
                return Ok(None);
            }
        };
        let mut oi = Self::new(config, tv, latest)?;
        if opts.inactive && oi.source == ToolSource::Unknown {
            // Installed-but-inactive tools have no config source, so their request
            // is usually pinned to the currently installed version. With --no-source we
            // want to install the discovered latest version instead.
            let backend = oi.tool_request.ba().clone();
            let source = oi.tool_request.source().clone();
            let options = oi.tool_request.options();
            oi.tool_request = ToolRequest::new_opts(backend, &oi.latest, options, source)?;
        }
        if oi
            .current
            .as_ref()
            .is_some_and(|c| !toolset::is_outdated_version(c, &oi.latest))
        {
            // Check if this is a rolling version (like "nightly") with a new checksum
            let rolling_outdated = t
                .is_rolling_version_outdated(config, &oi.tool_version.request.version())
                .await;
            if !rolling_outdated {
                trace!("skipping up-to-date version {}", oi.tool_version);
                return Ok(None);
            }
            trace!(
                "rolling version {} has updates (checksum changed)",
                oi.tool_version.request.version()
            );
        }
        if bump {
            let old = oi.tool_version.request.version();
            let old = old.strip_prefix(&prefix).unwrap_or_default();
            let new = oi.latest.strip_prefix(&prefix).unwrap_or_default();
            if let Some(bumped_version) = check_semver_bump(old, new)
                && bumped_version != oi.tool_version.request.version()
            {
                oi.bump = match oi.tool_request.clone() {
                    ToolRequest::Version {
                        version: _version,
                        backend,
                        options,
                        source,
                    } => {
                        oi.tool_request = ToolRequest::Version {
                            backend,
                            options,
                            source,
                            version: format!("{prefix}{bumped_version}"),
                        };
                        Some(oi.tool_request.version())
                    }
                    ToolRequest::Prefix {
                        prefix: _prefix,
                        backend,
                        options,
                        source,
                    } => {
                        oi.tool_request = ToolRequest::Prefix {
                            backend,
                            options,
                            source,
                            prefix: format!("{prefix}{bumped_version}"),
                        };
                        Some(oi.tool_request.version())
                    }
                    _ => {
                        warn!("upgrading non-version tool requests");
                        None
                    }
                }
            }
        }
        Ok(Some(oi))
    }

    fn display_current(current: &Option<String>) -> String {
        if let Some(current) = current {
            current.to_string()
        } else {
            "[MISSING]".to_string()
        }
    }

    fn display_bump(bump: &Option<String>) -> String {
        if let Some(bump) = bump {
            bump.to_string()
        } else {
            "[NONE]".to_string()
        }
    }
}

impl Display for OutdatedInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:<20} ", self.name)?;
        if let Some(current) = &self.current {
            write!(f, "{current:<20} ")?;
        } else {
            write!(f, "{:<20} ", "MISSING")?;
        }
        write!(f, "-> {:<10} (", self.latest)?;
        if let Some(bump) = &self.bump {
            write!(f, "bump to {bump} in ")?;
        }
        write!(f, "{})", self.source)
    }
}

fn prefixed_latest_query(prefix: &str, prefix_version: &str) -> Option<String> {
    let prefix = prefix.trim();
    if prefix.is_empty() || prefix_version.is_empty() || prefix.contains(':') {
        return None;
    }

    let query_version = chunkify_version(prefix_version)
        .into_iter()
        .next()
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| prefix_version.to_string());

    Some(format!("{prefix}{query_version}"))
}

/// check if the new version is a bump from the old version and return the new version
/// at the same specificity level as the old version
/// used with `mise outdated --bump` to determine what new semver range to use
/// given old: "20" and new: "21.2.3", return Some("21")
pub fn check_semver_bump(old: &str, new: &str) -> Option<String> {
    // Preserve known channel names as-is
    const CHANNEL_NAMES: &[&str] = &[
        "latest", "nightly", "stable", "beta", "dev", "canary", "edge", "lts",
    ];
    if CHANNEL_NAMES.iter().any(|&c| c.eq_ignore_ascii_case(old)) {
        return Some(old.to_string());
    }
    if let Some(("prefix", old_)) = old.split_once(':') {
        return check_semver_bump(old_, new);
    }
    let old_chunks = chunkify_version(old);
    let new_chunks = chunkify_version(new);
    // If old has no semver chunks but is non-empty, it's likely a channel name
    // that we didn't recognize - preserve it as-is
    if old_chunks.is_empty() && !old.is_empty() {
        return Some(old.to_string());
    }
    if !old_chunks.is_empty() && !new_chunks.is_empty() {
        if old_chunks.len() > new_chunks.len() {
            warn!(
                "something weird happened with versioning, old: {old:?}, new: {new:?}",
                old = old_chunks,
                new = new_chunks,
            );
        }
        let bump = new_chunks
            .into_iter()
            .take(old_chunks.len())
            .collect::<Vec<_>>();
        if bump == old_chunks {
            None
        } else {
            Some(bump.join(""))
        }
    } else {
        Some(new.to_string())
    }
}

/// Represents a config file update needed when a CLI-specified version doesn't match
/// the current config prefix.
pub struct ConfigBump {
    pub tool_name: String,
    pub config_path: std::path::PathBuf,
    pub old_version: String,
    pub new_version: String,
    pub new_request: ToolRequest,
}

/// Compute config bumps needed when CLI-specified versions don't match current config prefixes.
/// Returns a list of bumps to apply (or preview in dry-run mode).
pub fn compute_config_bumps(
    config: &Config,
    tool_versions: &[(&str, &str)], // (tool_short_name, cli_version)
) -> Vec<ConfigBump> {
    let config_paths = config.config_files.keys().cloned().collect();
    compute_config_bumps_for_paths(config, tool_versions, &config_paths)
}

/// Compute config bumps against a bounded set of config paths.
///
/// This lets callers that intentionally target a subset of the loaded config
/// hierarchy avoid updating shadowed parent configs.
pub fn compute_config_bumps_for_paths(
    config: &Config,
    tool_versions: &[(&str, &str)], // (tool_short_name, cli_version)
    config_paths: &BTreeSet<PathBuf>,
) -> Vec<ConfigBump> {
    let mut bumps = Vec::new();

    for &(tool_name, cli_version) in tool_versions {
        for (path, cf) in config.config_files.iter() {
            if !config_paths.contains(path) {
                continue;
            }
            if crate::config::is_global_config(path) {
                continue;
            }
            let Ok(trs) = cf.to_tool_request_set() else {
                continue;
            };

            // Find the tool by short name in this config file
            let matching = trs.tools.iter().find(|(ba, _)| ba.short == tool_name);
            let Some((_ba, requests)) = matching else {
                continue;
            };
            if requests.len() != 1 {
                continue;
            }

            let current_version = requests[0].version();
            let (prefix, _) = split_version_prefix(&current_version);
            let old = current_version
                .strip_prefix(&prefix)
                .unwrap_or(&current_version);

            if let Some(bumped) = check_semver_bump(old, cli_version)
                && bumped != old
            {
                let new_version = format!("{prefix}{bumped}");
                let new_request = match requests[0].clone() {
                    ToolRequest::Version {
                        version: _,
                        backend,
                        options,
                        source,
                    } => ToolRequest::Version {
                        version: new_version.clone(),
                        backend,
                        options,
                        source,
                    },
                    ToolRequest::Prefix {
                        prefix: _,
                        backend,
                        options,
                        source,
                    } => ToolRequest::Prefix {
                        prefix: format!("{prefix}{bumped}"),
                        backend,
                        options,
                        source,
                    },
                    other => other,
                };
                bumps.push(ConfigBump {
                    tool_name: tool_name.to_string(),
                    config_path: path.clone(),
                    old_version: current_version.to_string(),
                    new_version,
                    new_request,
                });
            }
            break;
        }
    }

    bumps
}

/// Apply config bumps by writing the new versions to their config files.
pub fn apply_config_bumps(config: &Config, bumps: &[ConfigBump]) -> Result<()> {
    for bump in bumps {
        let Some(cf) = config.config_files.get(&bump.config_path) else {
            continue;
        };
        let Ok(trs) = cf.to_tool_request_set() else {
            continue;
        };
        let Some((ba, _)) = trs.tools.iter().find(|(ba, _)| ba.short == bump.tool_name) else {
            continue;
        };
        cf.replace_versions(ba, vec![bump.new_request.clone()])?;
        cf.save()?;
    }
    Ok(())
}

pub fn is_outdated_version(current: &str, latest: &str) -> bool {
    if let (Some(c), Some(l)) = (Version::new(current), Version::new(latest)) {
        c.lt(&l)
    } else {
        current != latest
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use test_log::test;

    use super::{check_semver_bump, is_outdated_version, prefixed_latest_query};

    #[test]
    fn test_is_outdated_version() {
        assert_eq!(is_outdated_version("1.10.0", "1.12.0"), true);
        assert_eq!(is_outdated_version("1.12.0", "1.10.0"), false);

        assert_eq!(
            is_outdated_version("1.10.0-SNAPSHOT", "1.12.0-SNAPSHOT"),
            true
        );
        assert_eq!(
            is_outdated_version("1.12.0-SNAPSHOT", "1.10.0-SNAPSHOT"),
            false
        );

        assert_eq!(
            is_outdated_version("temurin-17.0.0", "temurin-17.0.1"),
            true
        );
        assert_eq!(
            is_outdated_version("temurin-17.0.1", "temurin-17.0.0"),
            false
        );
    }

    #[test]
    fn test_check_semver_bump() {
        std::assert_eq!(check_semver_bump("20", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20.0", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20.0.0", "20.0.0"), None);
        std::assert_eq!(check_semver_bump("20", "21.0.0"), Some("21".to_string()));
        std::assert_eq!(
            check_semver_bump("20.0", "20.1.0"),
            Some("20.1".to_string())
        );
        std::assert_eq!(
            check_semver_bump("20.0.0", "20.0.1"),
            Some("20.0.1".to_string())
        );
        std::assert_eq!(
            check_semver_bump("20.0.1", "20.1"),
            Some("20.1".to_string())
        );
        std::assert_eq!(
            check_semver_bump("2024-09-16", "2024-10-21"),
            Some("2024-10-21".to_string())
        );
        std::assert_eq!(
            check_semver_bump("20.0a1", "20.0a2"),
            Some("20.0a2".to_string())
        );
        std::assert_eq!(check_semver_bump("v20", "v20.0.0"), None);
        std::assert_eq!(check_semver_bump("v20.0", "v20.0.0"), None);
        std::assert_eq!(check_semver_bump("v20.0.0", "v20.0.0"), None);
        std::assert_eq!(check_semver_bump("v20", "v21.0.0"), Some("v21".to_string()));
        std::assert_eq!(
            check_semver_bump("v20.0.0", "v20.0.1"),
            Some("v20.0.1".to_string())
        );
        std::assert_eq!(
            check_semver_bump("latest", "20.0.0"),
            Some("latest".to_string())
        );
        // Channel names like "nightly", "stable", "beta" should be preserved
        std::assert_eq!(
            check_semver_bump("nightly", "0.10.0"),
            Some("nightly".to_string())
        );
        std::assert_eq!(
            check_semver_bump("stable", "0.10.0"),
            Some("stable".to_string())
        );
        std::assert_eq!(
            check_semver_bump("beta", "1.0.0-beta.1"),
            Some("beta".to_string())
        );
    }

    #[test]
    fn test_prefixed_latest_query() {
        assert_eq!(
            prefixed_latest_query("temurin-", "17.0.7+7"),
            Some("temurin-17".to_string())
        );
        assert_eq!(
            prefixed_latest_query("temurin-", "17-ea"),
            Some("temurin-17".to_string())
        );
        assert_eq!(
            prefixed_latest_query("corretto-", "2024-09-16"),
            Some("corretto-2024".to_string())
        );
        assert_eq!(prefixed_latest_query("prefix:1.", "24"), None);
        assert_eq!(prefixed_latest_query("", "17.0.7"), None);
        assert_eq!(prefixed_latest_query("temurin-", ""), None);
    }
}
