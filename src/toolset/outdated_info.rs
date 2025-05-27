use crate::toolset;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion};
use crate::{Result, config::Config};
use serde_derive::Serialize;
use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};
use tabled::Tabled;
use versions::{Mess, Version, Versioning};

#[derive(Debug, Serialize, Clone, Tabled)]
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
    ) -> eyre::Result<Option<Self>> {
        let t = tv.backend()?;
        // prefix is something like "temurin-" or "corretto-"
        let prefix = xx::regex!(r"^[a-zA-Z-]+-")
            .find(&tv.request.version())
            .map(|m| m.as_str().to_string());
        let latest_result = if bump {
            t.latest_version(config, prefix.clone()).await
        } else {
            tv.latest_version(config).await.map(Option::from)
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
        if oi
            .current
            .as_ref()
            .is_some_and(|c| !toolset::is_outdated_version(c, &oi.latest))
        {
            trace!("skipping up-to-date version {}", oi.tool_version);
            return Ok(None);
        }
        if bump {
            let prefix = prefix.unwrap_or_default();
            let old = oi.tool_version.request.version();
            let old = old.strip_prefix(&prefix).unwrap_or_default();
            let new = oi.latest.strip_prefix(&prefix).unwrap_or_default();
            if let Some(bumped_version) = check_semver_bump(old, new) {
                if bumped_version != oi.tool_version.request.version() {
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

/// check if the new version is a bump from the old version and return the new version
/// at the same specificity level as the old version
/// used with `mise outdated --bump` to determine what new semver range to use
/// given old: "20" and new: "21.2.3", return Some("21")
fn check_semver_bump(old: &str, new: &str) -> Option<String> {
    if old == "latest" {
        return Some("latest".to_string());
    }
    if let Some(("prefix", old_)) = old.split_once(':') {
        return check_semver_bump(old_, new);
    }
    let old_chunks = chunkify_version(old);
    let new_chunks = chunkify_version(new);
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

/// split a version number into chunks
/// given v: "1.2-3a4" return ["1", ".2", "-3", "a4"]
fn chunkify_version(v: &str) -> Vec<String> {
    fn chunkify(m: &Mess, sep0: &str, chunks: &mut Vec<String>) {
        for (i, chunk) in m.chunks.iter().enumerate() {
            let sep = if i == 0 { sep0 } else { "." };
            chunks.push(format!("{sep}{chunk}"));
        }
        if let Some((next_sep, next_mess)) = &m.next {
            chunkify(next_mess, next_sep.to_string().as_ref(), chunks)
        }
    }

    let mut chunks = vec![];
    // don't parse "latest", otherwise bump from latest to any version would have one chunk only
    if v != "latest" {
        if let Some(v) = Versioning::new(v) {
            let m = match v {
                Versioning::Ideal(sem_ver) => sem_ver.to_mess(),
                Versioning::General(version) => version.to_mess(),
                Versioning::Complex(mess) => mess,
            };
            chunkify(&m, "", &mut chunks);
        }
    }
    chunks
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

    use super::{check_semver_bump, is_outdated_version};

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
    }
}
