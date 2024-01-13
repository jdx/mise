use console::style;
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::cli::args::ForgeArg;
use eyre::Result;
use versions::{Chunk, Version};

use crate::config::Config;
use crate::forge::{AForge, Forge};
use crate::hash::hash_to_str;
use crate::toolset::{ToolVersionOptions, ToolVersionRequest};
use crate::{dirs, forge};

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolVersionRequest,
    pub forge: ForgeArg,
    pub version: String,
    pub opts: ToolVersionOptions,
}

impl ToolVersion {
    pub fn new(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        opts: ToolVersionOptions,
        version: String,
    ) -> Self {
        ToolVersion {
            forge: tool.get_fa(),
            version,
            request,
            opts,
        }
    }

    pub fn resolve(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        opts: ToolVersionOptions,
        latest_versions: bool,
    ) -> Result<Self> {
        if !tool.is_installed() {
            let tv = Self::new(tool, request.clone(), opts, request.version());
            return Ok(tv);
        }
        let tv = match request.clone() {
            ToolVersionRequest::Version(_, v) => {
                Self::resolve_version(tool, request, latest_versions, &v, opts)?
            }
            ToolVersionRequest::Prefix(_, prefix) => {
                Self::resolve_prefix(tool, request, &prefix, opts)?
            }
            ToolVersionRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(tool, request, latest_versions, &sub, &orig_version, opts)?,
            _ => {
                let version = request.version();
                Self::new(tool, request, opts, version)
            }
        };
        Ok(tv)
    }

    pub fn get_forge(&self) -> AForge {
        forge::get(&self.forge)
    }

    pub fn install_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolVersionRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(),
        };
        dirs::INSTALLS.join(self.forge.pathname()).join(pathname)
    }
    pub fn install_short_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolVersionRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_short_pathname(),
        };
        let sp = dirs::INSTALLS.join(self.forge.pathname()).join(pathname);
        if sp.exists() {
            sp
        } else {
            self.install_path()
        }
    }
    pub fn cache_path(&self) -> PathBuf {
        dirs::CACHE
            .join(self.forge.pathname())
            .join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        dirs::DOWNLOADS
            .join(self.forge.pathname())
            .join(self.tv_pathname())
    }
    pub fn latest_version(&self, tool: &dyn Forge) -> Result<String> {
        let tv = self.request.resolve(tool, self.opts.clone(), true)?;
        Ok(tv.version)
    }
    pub fn style(&self) -> String {
        format!(
            "{}{}",
            style(&self.forge.pathname()).blue().for_stderr(),
            style(&format!("@{}", &self.version)).for_stderr()
        )
    }
    fn tv_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version(_, _) => self.version.to_string(),
            ToolVersionRequest::Prefix(_, _) => self.version.to_string(),
            ToolVersionRequest::Sub { .. } => self.version.to_string(),
            ToolVersionRequest::Ref(_, r) => format!("ref-{}", r),
            ToolVersionRequest::Path(_, p) => format!("path-{}", hash_to_str(p)),
            ToolVersionRequest::System(_) => "system".to_string(),
        }
    }
    fn tv_short_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version(_, v) => v.to_string(),
            _ => self.tv_pathname(),
        }
    }

    fn resolve_version(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        latest_versions: bool,
        v: &str,
        opts: ToolVersionOptions,
    ) -> Result<ToolVersion> {
        let config = Config::get();
        let v = config.resolve_alias(tool, v)?;
        match v.split_once(':') {
            Some(("ref", r)) => {
                return Ok(Self::resolve_ref(tool, r.to_string(), opts));
            }
            Some(("path", p)) => {
                return Self::resolve_path(tool, PathBuf::from(p), opts);
            }
            Some(("prefix", p)) => {
                return Self::resolve_prefix(tool, request, p, opts);
            }
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(tool, request, latest_versions, sub, v, opts);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(tool, request.clone(), opts.clone(), v));
        if !tool.is_installed() {
            return build(v);
        }

        let existing = build(v.clone())?;
        if tool.is_version_installed(&existing) {
            // if the version is already installed, no need to fetch all the remote versions
            return Ok(existing);
        }

        if v == "latest" {
            if !latest_versions {
                if let Some(v) = tool.latest_installed_version(None)? {
                    return build(v);
                }
            }
            if let Some(v) = tool.latest_version(None)? {
                return build(v);
            }
        }
        if !latest_versions {
            let matches = tool.list_installed_versions_matching(&v)?;
            if matches.contains(&v) {
                return build(v);
            }
            if let Some(v) = matches.last() {
                return build(v.clone());
            }
        }
        let matches = tool.list_versions_matching(&v)?;
        if matches.contains(&v) {
            return build(v);
        }
        Self::resolve_prefix(tool, request, &v, opts)
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    fn resolve_sub(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        latest_versions: bool,
        sub: &str,
        v: &str,
        opts: ToolVersionOptions,
    ) -> Result<Self> {
        let v = match v {
            "latest" => tool.latest_version(None)?.unwrap(),
            _ => Config::get().resolve_alias(tool, v)?,
        };
        let v = version_sub(&v, sub);
        Self::resolve_version(tool, request, latest_versions, &v, opts)
    }

    fn resolve_prefix(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        prefix: &str,
        opts: ToolVersionOptions,
    ) -> Result<Self> {
        let matches = tool.list_versions_matching(prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(Self::new(tool, request, opts, v.to_string()))
    }

    fn resolve_ref(tool: &dyn Forge, r: String, opts: ToolVersionOptions) -> Self {
        let request = ToolVersionRequest::Ref(tool.get_fa(), r);
        let version = request.version();
        Self::new(tool, request, opts, version)
    }

    fn resolve_path(
        tool: &dyn Forge,
        path: PathBuf,
        opts: ToolVersionOptions,
    ) -> Result<ToolVersion> {
        let path = fs::canonicalize(path)?;
        let request = ToolVersionRequest::Path(tool.get_fa(), path);
        let version = request.version();
        Ok(Self::new(tool, request, opts, version))
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.forge.pathname(), &self.version)
    }
}

impl PartialEq for ToolVersion {
    fn eq(&self, other: &Self) -> bool {
        self.forge.pathname() == other.forge.pathname() && self.version == other.version
    }
}
impl Eq for ToolVersion {}
impl PartialOrd for ToolVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ToolVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.forge.pathname().cmp(&other.forge.pathname()) {
            Ordering::Equal => self.version.cmp(&other.version),
            o => o,
        }
    }
}
impl Hash for ToolVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.forge.pathname().hash(state);
        self.version.hash(state);
    }
}

/// subtracts sub from orig and removes suffix
/// e.g. version_sub("18.2.3", "2") -> "16"
/// e.g. version_sub("18.2.3", "0.1") -> "18.1"
fn version_sub(orig: &str, sub: &str) -> String {
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

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_version_sub() {
        assert_str_eq!(version_sub("18.2.3", "2"), "16");
        assert_str_eq!(version_sub("18.2.3", "0.1"), "18.1");
    }
}
