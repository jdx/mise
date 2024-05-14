use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use eyre::Result;
use versions::{Chunk, Version};
use xx::file;

use crate::cli::args::ForgeArg;
use crate::forge;
use crate::forge::Forge;
use crate::toolset::{ToolVersion, ToolVersionOptions};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolVersionRequest {
    Version {
        forge: ForgeArg,
        version: String,
        options: ToolVersionOptions,
    },
    Prefix {
        forge: ForgeArg,
        prefix: String,
        options: ToolVersionOptions,
    },
    Ref {
        forge: ForgeArg,
        ref_: String,
        options: ToolVersionOptions,
    },
    Sub {
        forge: ForgeArg,
        sub: String,
        orig_version: String,
    },
    Path(ForgeArg, PathBuf),
    System(ForgeArg),
}

impl ToolVersionRequest {
    pub fn new(forge: ForgeArg, s: &str) -> eyre::Result<Self> {
        let s = match s.split_once('-') {
            Some(("ref", r)) => format!("ref:{}", r),
            _ => s.to_string(),
        };
        Ok(match s.split_once(':') {
            Some(("ref", r)) => Self::Ref {
                forge,
                ref_: r.to_string(),
                options: Default::default(),
            },
            Some(("prefix", p)) => Self::Prefix {
                forge,
                prefix: p.to_string(),
                options: Default::default(),
            },
            Some(("path", p)) => Self::Path(forge, PathBuf::from(p)),
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                forge,
                sub: p.split_once('-').unwrap().1.to_string(),
                orig_version: v.to_string(),
            },
            None => {
                if s == "system" {
                    Self::System(forge)
                } else {
                    Self::Version {
                        forge,
                        version: s,
                        options: Default::default(),
                    }
                }
            }
            _ => bail!("invalid tool version request: {s}"),
        })
    }
    pub fn new_opts(forge: ForgeArg, s: &str, options: ToolVersionOptions) -> eyre::Result<Self> {
        let mut tvr = Self::new(forge, s)?;
        match &mut tvr {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => *o = options,
            _ => Default::default(),
        }
        Ok(tvr)
    }
    pub fn forge(&self) -> &ForgeArg {
        match self {
            Self::Version { forge: f, .. }
            | Self::Prefix { forge: f, .. }
            | Self::Ref { forge: f, .. }
            | Self::Path(f, _)
            | Self::Sub { forge: f, .. }
            | Self::System(f) => f,
        }
    }
    pub fn dependencies(&self) -> eyre::Result<Vec<ForgeArg>> {
        let forge = forge::get(self.forge());
        forge.get_dependencies(self)
    }
    pub fn version(&self) -> String {
        match self {
            Self::Version { version: v, .. } => v.clone(),
            Self::Prefix { prefix: p, .. } => format!("prefix:{p}"),
            Self::Ref { ref_: r, .. } => format!("ref:{r}"),
            Self::Path(_, p) => format!("path:{}", p.display()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{}:{}", sub, orig_version),
            Self::System(_) => "system".to_string(),
        }
    }

    pub fn options(&self) -> ToolVersionOptions {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => o.clone(),
            _ => Default::default(),
        }
    }

    pub fn is_installed(&self) -> bool {
        // TODO: dispatch to forge
        match self {
            Self::System(_) => true,
            _ => self.install_path().is_some_and(|p| p.exists()),
        }
    }

    pub fn install_path(&self) -> Option<PathBuf> {
        match self {
            Self::Version { forge, version, .. } => Some(forge.installs_path.join(version)),
            Self::Ref { forge, ref_, .. } => {
                Some(forge.installs_path.join(format!("ref-{}", ref_)))
            }
            Self::Sub {
                forge,
                sub,
                orig_version,
            } => self
                .local_resolve(orig_version)
                .inspect_err(|e| warn!("ToolRequest.local_resolve: {e:#}"))
                .unwrap_or_default()
                .map(|v| forge.installs_path.join(version_sub(&v, sub.as_str()))),
            Self::Prefix { forge, prefix, .. } => match file::ls(&forge.installs_path) {
                Ok(installs) => {
                    let path = installs.iter().find(|p| p.starts_with(prefix));
                    path.map(|p| forge.installs_path.join(p))
                }
                Err(_) => None,
            },
            Self::Path(_, path) => Some(path.clone()),
            Self::System(_) => None,
        }
    }

    pub fn local_resolve(&self, v: &str) -> eyre::Result<Option<String>> {
        let forge = forge::get(self.forge());
        let matches = forge.list_installed_versions_matching(v)?;
        if matches.iter().any(|m| m == v) {
            return Ok(Some(v.to_string()));
        }
        if let Some(v) = matches.last() {
            return Ok(Some(v.to_string()));
        }
        Ok(None)
    }

    pub fn resolve(&self, plugin: &dyn Forge, latest_versions: bool) -> Result<ToolVersion> {
        ToolVersion::resolve(plugin, self.clone(), latest_versions)
    }
}

/// subtracts sub from orig and removes suffix
/// e.g. version_sub("18.2.3", "2") -> "16"
/// e.g. version_sub("18.2.3", "0.1") -> "18.1"
pub fn version_sub(orig: &str, sub: &str) -> String {
    let mut orig = Version::new(orig).unwrap();
    let sub = Version::new(sub).unwrap();
    while orig.chunks.0.len() > sub.chunks.0.len() {
        orig.chunks.0.pop();
    }
    for (i, orig_chunk) in orig.clone().chunks.0.iter().enumerate() {
        let m = sub.nth(i).unwrap();
        orig.chunks.0[i] = Chunk::Numeric(orig_chunk.single_digit().unwrap() - m);
    }
    orig.to_string()
}

impl Display for ToolVersionRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.forge(), self.version())
    }
}

#[cfg(test)]
mod tests {
    use super::version_sub;

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
