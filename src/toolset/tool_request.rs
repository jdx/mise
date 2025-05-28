use std::path::PathBuf;
use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};

use eyre::{Result, bail};
use versions::{Chunk, Version};
use xx::file;

use crate::cli::args::BackendArg;
use crate::lockfile::LockfileTool;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::tool_version::ResolveOptions;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionOptions};
use crate::{backend, lockfile};
use crate::{backend::ABackend, config::Config};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolRequest {
    Version {
        backend: Arc<BackendArg>,
        version: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Prefix {
        backend: Arc<BackendArg>,
        prefix: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Ref {
        backend: Arc<BackendArg>,
        ref_: String,
        ref_type: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Sub {
        backend: Arc<BackendArg>,
        sub: String,
        orig_version: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Path {
        backend: Arc<BackendArg>,
        path: PathBuf,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    System {
        backend: Arc<BackendArg>,
        source: ToolSource,
        options: ToolVersionOptions,
    },
}

impl ToolRequest {
    pub fn new(backend: Arc<BackendArg>, s: &str, source: ToolSource) -> eyre::Result<Self> {
        let s = match s.split_once('-') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => format!("{ref_type}:{r}"),
            _ => s.to_string(),
        };
        Ok(match s.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => Self::Ref {
                ref_: r.to_string(),
                ref_type: ref_type.to_string(),
                options: backend.opts(),
                backend,
                source,
            },
            Some(("prefix", p)) => Self::Prefix {
                prefix: p.to_string(),
                options: backend.opts(),
                backend,
                source,
            },
            Some(("path", p)) => Self::Path {
                path: PathBuf::from(p),
                options: backend.opts(),
                backend,
                source,
            },
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                sub: p.split_once('-').unwrap().1.to_string(),
                options: backend.opts(),
                orig_version: v.to_string(),
                backend,
                source,
            },
            None => {
                if s == "system" {
                    Self::System {
                        options: backend.opts(),
                        backend,
                        source,
                    }
                } else {
                    Self::Version {
                        version: s,
                        options: backend.opts(),
                        backend,
                        source,
                    }
                }
            }
            _ => bail!("invalid tool version request: {s}"),
        })
    }
    pub fn new_opts(
        backend: Arc<BackendArg>,
        s: &str,
        options: ToolVersionOptions,
        source: ToolSource,
    ) -> eyre::Result<Self> {
        let mut tvr = Self::new(backend, s, source)?;
        match &mut tvr {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. } => *o = options,
            _ => Default::default(),
        }
        Ok(tvr)
    }
    pub fn set_source(&mut self, source: ToolSource) -> Self {
        match self {
            Self::Version { source: s, .. }
            | Self::Prefix { source: s, .. }
            | Self::Ref { source: s, .. }
            | Self::Path { source: s, .. }
            | Self::Sub { source: s, .. }
            | Self::System { source: s, .. } => *s = source,
        }
        self.clone()
    }
    pub fn ba(&self) -> &Arc<BackendArg> {
        match self {
            Self::Version { backend, .. }
            | Self::Prefix { backend, .. }
            | Self::Ref { backend, .. }
            | Self::Path { backend, .. }
            | Self::Sub { backend, .. }
            | Self::System { backend, .. } => backend,
        }
    }
    pub fn backend(&self) -> Result<ABackend> {
        self.ba().backend()
    }
    pub fn source(&self) -> &ToolSource {
        match self {
            Self::Version { source, .. }
            | Self::Prefix { source, .. }
            | Self::Ref { source, .. }
            | Self::Path { source, .. }
            | Self::Sub { source, .. }
            | Self::System { source, .. } => source,
        }
    }
    pub fn os(&self) -> &Option<Vec<String>> {
        match self {
            Self::Version { options, .. }
            | Self::Prefix { options, .. }
            | Self::Ref { options, .. }
            | Self::Path { options, .. }
            | Self::Sub { options, .. }
            | Self::System { options, .. } => &options.os,
        }
    }
    pub fn set_options(&mut self, options: ToolVersionOptions) -> &mut Self {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. }
            | Self::Sub { options: o, .. }
            | Self::Path { options: o, .. }
            | Self::System { options: o, .. } => *o = options,
        }
        self
    }
    pub fn version(&self) -> String {
        match self {
            Self::Version { version: v, .. } => v.clone(),
            Self::Prefix { prefix: p, .. } => format!("prefix:{p}"),
            Self::Ref {
                ref_: r, ref_type, ..
            } => format!("{ref_type}:{r}"),
            Self::Path { path: p, .. } => format!("path:{}", p.display()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{sub}:{orig_version}"),
            Self::System { .. } => "system".to_string(),
        }
    }

    pub fn options(&self) -> ToolVersionOptions {
        match self {
            Self::Version { options: o, .. }
            | Self::Prefix { options: o, .. }
            | Self::Ref { options: o, .. }
            | Self::Sub { options: o, .. }
            | Self::Path { options: o, .. }
            | Self::System { options: o, .. } => o.clone(),
        }
    }

    pub async fn is_installed(&self, config: &Arc<Config>) -> bool {
        if let Some(backend) = backend::get(self.ba()) {
            match self.resolve(config, &Default::default()).await {
                Ok(tv) => backend.is_version_installed(config, &tv, false),
                Err(e) => {
                    debug!("ToolRequest.is_installed: {e:#}");
                    false
                }
            }
        } else {
            false
        }
    }

    pub fn install_path(&self, config: &Config) -> Option<PathBuf> {
        match self {
            Self::Version {
                backend, version, ..
            } => Some(backend.installs_path.join(version)),
            Self::Ref {
                backend,
                ref_,
                ref_type,
                ..
            } => Some(backend.installs_path.join(format!("{ref_type}-{ref_}"))),
            Self::Sub {
                backend,
                sub,
                orig_version,
                ..
            } => self
                .local_resolve(config, orig_version)
                .inspect_err(|e| warn!("ToolRequest.local_resolve: {e:#}"))
                .unwrap_or_default()
                .map(|v| backend.installs_path.join(version_sub(&v, sub.as_str()))),
            Self::Prefix {
                backend, prefix, ..
            } => match file::ls(&backend.installs_path) {
                Ok(installs) => installs
                    .iter()
                    .find(|p| {
                        !is_runtime_symlink(p)
                            && p.file_name().unwrap().to_string_lossy().starts_with(prefix)
                    })
                    .cloned(),
                Err(_) => None,
            },
            Self::Path { path, .. } => Some(path.clone()),
            Self::System { .. } => None,
        }
    }

    pub fn lockfile_resolve(&self, config: &Config) -> Result<Option<LockfileTool>> {
        match self.source() {
            ToolSource::MiseToml(path) => {
                lockfile::get_locked_version(config, Some(path), &self.ba().short, &self.version())
            }
            _ => lockfile::get_locked_version(config, None, &self.ba().short, &self.version()),
        }
    }

    pub fn local_resolve(&self, config: &Config, v: &str) -> eyre::Result<Option<String>> {
        if let Some(lt) = self.lockfile_resolve(config)? {
            return Ok(Some(lt.version));
        }
        if let Some(backend) = backend::get(self.ba()) {
            let matches = backend.list_installed_versions_matching(v);
            if matches.iter().any(|m| m == v) {
                return Ok(Some(v.to_string()));
            }
            if let Some(v) = matches.last() {
                return Ok(Some(v.to_string()));
            }
        }
        Ok(None)
    }

    pub async fn resolve(
        &self,
        config: &Arc<Config>,
        opts: &ResolveOptions,
    ) -> Result<ToolVersion> {
        ToolVersion::resolve(config, self.clone(), opts).await
    }

    pub fn is_os_supported(&self) -> bool {
        if let Some(os) = self.os() {
            if !os.contains(&crate::cli::version::OS) {
                return false;
            }
        }
        self.ba().is_os_supported()
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

impl Display for ToolRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.ba(), self.version())
    }
}

#[cfg(test)]
mod tests {
    use super::version_sub;
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
