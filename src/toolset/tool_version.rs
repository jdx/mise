use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use console::style;
use eyre::Result;

use crate::backend;
use crate::backend::{ABackend, Backend};
use crate::cli::args::BackendArg;
use crate::config::Config;
#[cfg(windows)]
use crate::file;
use crate::hash::hash_to_str;
use crate::toolset::{tool_request, ToolRequest, ToolVersionOptions};

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolRequest,
    pub backend: BackendArg,
    pub version: String,
}

impl ToolVersion {
    pub fn new(tool: &dyn Backend, request: ToolRequest, version: String) -> Self {
        ToolVersion {
            backend: tool.fa().clone(),
            version,
            request,
        }
    }

    pub fn resolve(
        backend: &dyn Backend,
        request: ToolRequest,
        latest_versions: bool,
    ) -> Result<Self> {
        if let Some(plugin) = backend.plugin() {
            if !plugin.is_installed() {
                let tv = Self::new(backend, request.clone(), request.version());
                return Ok(tv);
            }
        }
        let tv = match request.clone() {
            ToolRequest::Version { version: v, .. } => {
                Self::resolve_version(backend, request, latest_versions, &v)?
            }
            ToolRequest::Prefix { prefix, .. } => Self::resolve_prefix(backend, request, &prefix)?,
            ToolRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(backend, request, latest_versions, &sub, &orig_version)?,
            _ => {
                let version = request.version();
                Self::new(backend, request, version)
            }
        };
        Ok(tv)
    }

    pub fn get_backend(&self) -> ABackend {
        backend::get(&self.backend)
    }

    pub fn install_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(),
        };
        let path = self.backend.installs_path.join(pathname);

        // handle non-symlinks on windows
        // TODO: make this a utility function in xx
        #[cfg(windows)]
        if path.is_file() {
            if let Ok(p) = file::read_to_string(&path).map(PathBuf::from) {
                let path = self.backend.installs_path.join(p);
                if path.exists() {
                    return path;
                }
            }
        }
        path
    }
    pub fn install_short_path(&self) -> PathBuf {
        if cfg!(windows) {
            return self.install_path();
        }
        let pathname = match &self.request {
            ToolRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_short_pathname(),
        };
        let sp = self.backend.installs_path.join(pathname);
        if sp.exists() {
            sp
        } else {
            self.install_path()
        }
    }
    pub fn cache_path(&self) -> PathBuf {
        self.backend.cache_path.join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        self.backend.downloads_path.join(self.tv_pathname())
    }
    pub fn latest_version(&self, tool: &dyn Backend) -> Result<String> {
        let tv = self.request.resolve(tool, true)?;
        Ok(tv.version)
    }
    pub fn style(&self) -> String {
        format!(
            "{}{}",
            style(&self.backend.full).blue().for_stderr(),
            style(&format!("@{}", &self.version)).for_stderr()
        )
    }
    fn tv_pathname(&self) -> String {
        match &self.request {
            ToolRequest::Version { .. } => self.version.to_string(),
            ToolRequest::Prefix { .. } => self.version.to_string(),
            ToolRequest::Sub { .. } => self.version.to_string(),
            ToolRequest::Ref { ref_: r, .. } => format!("ref-{}", r),
            ToolRequest::Path(_, p) => format!("path-{}", hash_to_str(p)),
            ToolRequest::System(_) => "system".to_string(),
        }
        .replace([':', '/'], "-")
    }
    fn tv_short_pathname(&self) -> String {
        match &self.request {
            ToolRequest::Version { version: v, .. } => v.to_string(),
            _ => self.tv_pathname(),
        }
        .replace([':', '/'], "-")
    }

    fn resolve_version(
        backend: &dyn Backend,
        request: ToolRequest,
        latest_versions: bool,
        v: &str,
    ) -> Result<ToolVersion> {
        let config = Config::get();
        let v = config.resolve_alias(backend, v)?;
        match v.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => {
                return Ok(Self::resolve_ref(
                    backend,
                    r.to_string(),
                    ref_type.to_string(),
                    request.options(),
                ));
            }
            Some(("path", p)) => {
                return Self::resolve_path(backend, PathBuf::from(p));
            }
            Some(("prefix", p)) => {
                return Self::resolve_prefix(backend, request, p);
            }
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(backend, request, latest_versions, sub, v);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(backend, request.clone(), v));

        if let Some(plugin) = backend.plugin() {
            if !plugin.is_installed() {
                return build(v);
            }
        }

        let existing = build(v.clone())?;
        if backend.is_version_installed(&existing, true) {
            // if the version is already installed, no need to fetch all the remote versions
            return Ok(existing);
        }

        if v == "latest" {
            if !latest_versions {
                if let Some(v) = backend.latest_installed_version(None)? {
                    return build(v);
                }
            }
            if let Some(v) = backend.latest_version(None)? {
                return build(v);
            }
        }
        if !latest_versions {
            let matches = backend.list_installed_versions_matching(&v)?;
            if matches.contains(&v) {
                return build(v);
            }
            if let Some(v) = matches.last() {
                return build(v.clone());
            }
        }
        let matches = backend.list_versions_matching(&v)?;
        if matches.contains(&v) {
            return build(v);
        }
        Self::resolve_prefix(backend, request, &v)
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    fn resolve_sub(
        tool: &dyn Backend,
        request: ToolRequest,
        latest_versions: bool,
        sub: &str,
        v: &str,
    ) -> Result<Self> {
        let v = match v {
            "latest" => tool.latest_version(None)?.unwrap(),
            _ => Config::get().resolve_alias(tool, v)?,
        };
        let v = tool_request::version_sub(&v, sub);
        Self::resolve_version(tool, request, latest_versions, &v)
    }

    fn resolve_prefix(tool: &dyn Backend, request: ToolRequest, prefix: &str) -> Result<Self> {
        let matches = tool.list_versions_matching(prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(Self::new(tool, request, v.to_string()))
    }

    fn resolve_ref(
        tool: &dyn Backend,
        ref_: String,
        ref_type: String,
        opts: ToolVersionOptions,
    ) -> Self {
        let request = ToolRequest::Ref {
            backend: tool.fa().clone(),
            ref_,
            ref_type,
            options: opts.clone(),
        };
        let version = request.version();
        Self::new(tool, request, version)
    }

    fn resolve_path(tool: &dyn Backend, path: PathBuf) -> Result<ToolVersion> {
        let path = fs::canonicalize(path)?;
        let request = ToolRequest::Path(tool.fa().clone(), path);
        let version = request.version();
        Ok(Self::new(tool, request, version))
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.backend.full, &self.version)
    }
}

impl PartialEq for ToolVersion {
    fn eq(&self, other: &Self) -> bool {
        self.backend.full == other.backend.full && self.version == other.version
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
        match self.backend.full.cmp(&other.backend.full) {
            Ordering::Equal => self.version.cmp(&other.version),
            o => o,
        }
    }
}

impl Hash for ToolVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.backend.full.hash(state);
        self.version.hash(state);
    }
}
