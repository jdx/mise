use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use eyre::{bail, Result};
use versions::{Chunk, Version};
use xx::file;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolSource, ToolVersion, ToolVersionOptions};
use crate::{backend, lockfile};

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ToolRequest {
    Version {
        backend: BackendArg,
        version: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Prefix {
        backend: BackendArg,
        prefix: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Ref {
        backend: BackendArg,
        ref_: String,
        ref_type: String,
        options: ToolVersionOptions,
        source: ToolSource,
    },
    Sub {
        backend: BackendArg,
        sub: String,
        orig_version: String,
        source: ToolSource,
    },
    Path(BackendArg, PathBuf, ToolSource),
    System(BackendArg, ToolSource),
}

impl ToolRequest {
    pub fn new(backend: BackendArg, s: &str, source: ToolSource) -> eyre::Result<Self> {
        let s = match s.split_once('-') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => format!("{ref_type}:{r}"),
            _ => s.to_string(),
        };
        Ok(match s.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => Self::Ref {
                ref_: r.to_string(),
                ref_type: ref_type.to_string(),
                options: backend.opts.clone().unwrap_or_default(),
                backend,
                source,
            },
            Some(("prefix", p)) => Self::Prefix {
                prefix: p.to_string(),
                options: backend.opts.clone().unwrap_or_default(),
                backend,
                source,
            },
            Some(("path", p)) => Self::Path(backend, PathBuf::from(p), source),
            Some((p, v)) if p.starts_with("sub-") => Self::Sub {
                sub: p.split_once('-').unwrap().1.to_string(),
                orig_version: v.to_string(),
                backend,
                source,
            },
            None => {
                if s == "system" {
                    Self::System(backend, source)
                } else {
                    Self::Version {
                        version: s,
                        options: backend.opts.clone().unwrap_or_default(),
                        backend,
                        source,
                    }
                }
            }
            _ => bail!("invalid tool version request: {s}"),
        })
    }
    pub fn new_opts(
        backend: BackendArg,
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
            | Self::Path(_, _, s)
            | Self::Sub { source: s, .. }
            | Self::System(_, s) => *s = source,
        }
        self.clone()
    }
    pub fn backend(&self) -> &BackendArg {
        match self {
            Self::Version { backend: f, .. }
            | Self::Prefix { backend: f, .. }
            | Self::Ref { backend: f, .. }
            | Self::Path(f, ..)
            | Self::Sub { backend: f, .. }
            | Self::System(f, ..) => f,
        }
    }
    pub fn source(&self) -> &ToolSource {
        match self {
            Self::Version { source, .. }
            | Self::Prefix { source, .. }
            | Self::Ref { source, .. }
            | Self::Path(.., source)
            | Self::Sub { source, .. }
            | Self::System(_, source) => source,
        }
    }
    pub fn dependencies(&self) -> eyre::Result<Vec<BackendArg>> {
        let backend = backend::get(self.backend());
        backend.get_all_dependencies(self)
    }
    pub fn version(&self) -> String {
        match self {
            Self::Version { version: v, .. } => v.clone(),
            Self::Prefix { prefix: p, .. } => format!("prefix:{p}"),
            Self::Ref {
                ref_: r, ref_type, ..
            } => format!("{ref_type}:{r}"),
            Self::Path(_, p, ..) => format!("path:{}", p.display()),
            Self::Sub {
                sub, orig_version, ..
            } => format!("sub-{}:{}", sub, orig_version),
            Self::System(..) => "system".to_string(),
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
        let backend = backend::get(self.backend());
        let tv = ToolVersion::new(backend.as_ref(), self.clone(), self.version());

        backend.is_version_installed(&tv, false)
    }

    pub fn install_path(&self) -> Option<PathBuf> {
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
                .local_resolve(orig_version)
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
            Self::Path(_, path, ..) => Some(path.clone()),
            Self::System(..) => None,
        }
    }

    pub fn lockfile_resolve(&self) -> Result<Option<String>> {
        if let Some(path) = self.source().path() {
            return lockfile::get_locked_version(path, &self.backend().short);
        }
        Ok(None)
    }

    pub fn local_resolve(&self, v: &str) -> eyre::Result<Option<String>> {
        if let Some(v) = self.lockfile_resolve()? {
            return Ok(Some(v));
        }
        let backend = backend::get(self.backend());
        let matches = backend.list_installed_versions_matching(v)?;
        if matches.iter().any(|m| m == v) {
            return Ok(Some(v.to_string()));
        }
        if let Some(v) = matches.last() {
            return Ok(Some(v.to_string()));
        }
        Ok(None)
    }

    pub fn resolve(&self, plugin: &dyn Backend, latest_versions: bool) -> Result<ToolVersion> {
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

impl Display for ToolRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.backend(), self.version())
    }
}

#[cfg(test)]
mod tests {
    use super::version_sub;
    use crate::backend::reset;
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    #[test]
    fn test_version_sub() {
        reset();
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
