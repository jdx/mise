use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use console::style;
use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::config::Config;
use crate::forge;
use crate::forge::{AForge, Forge};
use crate::hash::hash_to_str;
use crate::toolset::{tool_version_request, ToolVersionOptions, ToolVersionRequest};

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolVersionRequest,
    pub forge: ForgeArg,
    pub version: String,
}

impl ToolVersion {
    pub fn new(tool: &dyn Forge, request: ToolVersionRequest, version: String) -> Self {
        ToolVersion {
            forge: tool.fa().clone(),
            version,
            request,
        }
    }

    pub fn resolve(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        latest_versions: bool,
    ) -> Result<Self> {
        if !tool.is_installed() {
            let tv = Self::new(tool, request.clone(), request.version());
            return Ok(tv);
        }
        let tv = match request.clone() {
            ToolVersionRequest::Version { version: v, .. } => {
                Self::resolve_version(tool, request, latest_versions, &v)?
            }
            ToolVersionRequest::Prefix { prefix, .. } => {
                Self::resolve_prefix(tool, request, &prefix)?
            }
            ToolVersionRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(tool, request, latest_versions, &sub, &orig_version)?,
            _ => {
                let version = request.version();
                Self::new(tool, request, version)
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
        self.forge.installs_path.join(pathname)
    }
    pub fn install_short_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolVersionRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_short_pathname(),
        };
        let sp = self.forge.installs_path.join(pathname);
        if sp.exists() {
            sp
        } else {
            self.install_path()
        }
    }
    pub fn cache_path(&self) -> PathBuf {
        self.forge.cache_path.join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        self.forge.downloads_path.join(self.tv_pathname())
    }
    pub fn latest_version(&self, tool: &dyn Forge) -> Result<String> {
        let tv = self.request.resolve(tool, true)?;
        Ok(tv.version)
    }
    pub fn style(&self) -> String {
        format!(
            "{}{}",
            style(&self.forge.id).blue().for_stderr(),
            style(&format!("@{}", &self.version)).for_stderr()
        )
    }
    fn tv_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version { .. } => self.version.to_string(),
            ToolVersionRequest::Prefix { .. } => self.version.to_string(),
            ToolVersionRequest::Sub { .. } => self.version.to_string(),
            ToolVersionRequest::Ref { ref_: r, .. } => format!("ref-{}", r),
            ToolVersionRequest::Path(_, p) => format!("path-{}", hash_to_str(p)),
            ToolVersionRequest::System(_) => "system".to_string(),
        }
        .replace([':', '/'], "-")
    }
    fn tv_short_pathname(&self) -> String {
        match &self.request {
            ToolVersionRequest::Version { version: v, .. } => v.to_string(),
            _ => self.tv_pathname(),
        }
        .replace([':', '/'], "-")
    }

    fn resolve_version(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        latest_versions: bool,
        v: &str,
    ) -> Result<ToolVersion> {
        let config = Config::get();
        let v = config.resolve_alias(tool, v)?;
        match v.split_once(':') {
            Some(("ref", r)) => {
                return Ok(Self::resolve_ref(tool, r.to_string(), request.options()));
            }
            Some(("path", p)) => {
                return Self::resolve_path(tool, PathBuf::from(p));
            }
            Some(("prefix", p)) => {
                return Self::resolve_prefix(tool, request, p);
            }
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(tool, request, latest_versions, sub, v);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(tool, request.clone(), v));
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
        Self::resolve_prefix(tool, request, &v)
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    fn resolve_sub(
        tool: &dyn Forge,
        request: ToolVersionRequest,
        latest_versions: bool,
        sub: &str,
        v: &str,
    ) -> Result<Self> {
        let v = match v {
            "latest" => tool.latest_version(None)?.unwrap(),
            _ => Config::get().resolve_alias(tool, v)?,
        };
        let v = tool_version_request::version_sub(&v, sub);
        Self::resolve_version(tool, request, latest_versions, &v)
    }

    fn resolve_prefix(tool: &dyn Forge, request: ToolVersionRequest, prefix: &str) -> Result<Self> {
        let matches = tool.list_versions_matching(prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(Self::new(tool, request, v.to_string()))
    }

    fn resolve_ref(tool: &dyn Forge, ref_: String, opts: ToolVersionOptions) -> Self {
        let request = ToolVersionRequest::Ref {
            forge: tool.fa().clone(),
            ref_,
            options: opts.clone(),
        };
        let version = request.version();
        Self::new(tool, request, version)
    }

    fn resolve_path(tool: &dyn Forge, path: PathBuf) -> Result<ToolVersion> {
        let path = fs::canonicalize(path)?;
        let request = ToolVersionRequest::Path(tool.fa().clone(), path);
        let version = request.version();
        Ok(Self::new(tool, request, version))
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.forge.id, &self.version)
    }
}

impl PartialEq for ToolVersion {
    fn eq(&self, other: &Self) -> bool {
        self.forge.id == other.forge.id && self.version == other.version
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
        match self.forge.id.cmp(&other.forge.id) {
            Ordering::Equal => self.version.cmp(&other.version),
            o => o,
        }
    }
}

impl Hash for ToolVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.forge.id.hash(state);
        self.version.hash(state);
    }
}
