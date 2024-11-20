use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::backend::ABackend;
use crate::cli::args::BackendArg;
use crate::config::Config;
#[cfg(windows)]
use crate::file;
use crate::hash::hash_to_str;
use crate::toolset::{tool_request, ToolRequest, ToolVersionOptions};
use console::style;
use eyre::Result;
#[cfg(windows)]
use path_absolutize::Absolutize;

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolRequest,
    pub version: String,
    pub checksum: Option<String>,
}

impl ToolVersion {
    pub fn new(request: ToolRequest, version: String) -> Self {
        ToolVersion {
            request,
            version,
            checksum: None,
        }
    }

    pub fn resolve(request: ToolRequest, opts: &ResolveOptions) -> Result<Self> {
        if opts.use_locked_version {
            if let Some(lt) = request.lockfile_resolve()? {
                let mut tv = Self::new(request.clone(), lt.version);
                tv.checksum = lt.checksum;
                return Ok(tv);
            }
        }
        let backend = request.ba().backend()?;
        if let Some(plugin) = backend.plugin() {
            if !plugin.is_installed() {
                let tv = Self::new(request.clone(), request.version());
                return Ok(tv);
            }
        }
        let tv = match request.clone() {
            ToolRequest::Version { version: v, .. } => Self::resolve_version(request, &v, opts)?,
            ToolRequest::Prefix { prefix, .. } => Self::resolve_prefix(request, &prefix, opts)?,
            ToolRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(request, &sub, &orig_version, opts)?,
            _ => {
                let version = request.version();
                Self::new(request, version)
            }
        };
        Ok(tv)
    }

    pub fn ba(&self) -> &BackendArg {
        self.request.ba()
    }

    pub fn backend(&self) -> Result<ABackend> {
        self.ba().backend()
    }

    pub fn install_path(&self) -> PathBuf {
        let pathname = match &self.request {
            ToolRequest::Path { path: p, .. } => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(),
        };
        let path = self.ba().installs_path.join(pathname);

        // handle non-symlinks on windows
        // TODO: make this a utility function in xx
        #[cfg(windows)]
        if path.is_file() {
            if let Ok(p) = file::read_to_string(&path).map(PathBuf::from) {
                let path = self.ba().installs_path.join(p);
                if path.exists() {
                    return path
                        .absolutize()
                        .expect("failed to absolutize path")
                        .to_path_buf();
                }
            }
        }
        path
    }
    pub fn cache_path(&self) -> PathBuf {
        self.ba().cache_path.join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        self.request.ba().downloads_path.join(self.tv_pathname())
    }
    pub fn latest_version(&self) -> Result<String> {
        let opts = ResolveOptions {
            latest_versions: true,
            use_locked_version: false,
        };
        let tv = self.request.resolve(&opts)?;
        // map cargo backend specific prefixes to ref
        let version = match tv.request.version().split_once(':') {
            Some((_ref_type @ ("tag" | "branch" | "rev"), r)) => {
                format!("ref:{r}")
            }
            _ => tv.version,
        };
        Ok(version)
    }
    pub fn style(&self) -> String {
        format!(
            "{}{}",
            style(&self.ba().short).blue().for_stderr(),
            style(&format!("@{}", &self.version)).for_stderr()
        )
    }
    pub fn tv_pathname(&self) -> String {
        match &self.request {
            ToolRequest::Version { .. } => self.version.to_string(),
            ToolRequest::Prefix { .. } => self.version.to_string(),
            ToolRequest::Sub { .. } => self.version.to_string(),
            ToolRequest::Ref { ref_: r, .. } => format!("ref-{}", r),
            ToolRequest::Path { path: p, .. } => format!("path-{}", hash_to_str(p)),
            ToolRequest::System { .. } => "system".to_string(),
        }
        .replace([':', '/'], "-")
    }
    fn resolve_version(
        request: ToolRequest,
        v: &str,
        opts: &ResolveOptions,
    ) -> Result<ToolVersion> {
        let config = Config::get();
        let backend = request.backend()?;
        let v = config.resolve_alias(&backend, v)?;
        match v.split_once(':') {
            Some((ref_type @ ("ref" | "tag" | "branch" | "rev"), r)) => {
                return Ok(Self::resolve_ref(
                    r.to_string(),
                    ref_type.to_string(),
                    request.options(),
                    &request,
                ));
            }
            Some(("path", p)) => {
                return Self::resolve_path(PathBuf::from(p), &request);
            }
            Some(("prefix", p)) => {
                return Self::resolve_prefix(request, p, opts);
            }
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(request, sub, v, opts);
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(request.clone(), v));

        if let Some(plugin) = backend.plugin() {
            if !plugin.is_installed() {
                return build(v);
            }
        }

        if v == "latest" {
            if !opts.latest_versions {
                if let Some(v) = backend.latest_installed_version(None)? {
                    return build(v);
                }
            }
            if let Some(v) = backend.latest_version(None)? {
                return build(v);
            }
        }
        if !opts.latest_versions {
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
        Self::resolve_prefix(request, &v, opts)
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    fn resolve_sub(
        request: ToolRequest,
        sub: &str,
        v: &str,
        opts: &ResolveOptions,
    ) -> Result<Self> {
        let backend = request.backend()?;
        let v = match v {
            "latest" => backend.latest_version(None)?.unwrap(),
            _ => Config::get().resolve_alias(&backend, v)?,
        };
        let v = tool_request::version_sub(&v, sub);
        Self::resolve_version(request, &v, opts)
    }

    fn resolve_prefix(request: ToolRequest, prefix: &str, opts: &ResolveOptions) -> Result<Self> {
        let backend = request.backend()?;
        if !opts.latest_versions {
            if let Some(v) = backend.list_installed_versions_matching(prefix)?.last() {
                return Ok(Self::new(request, v.to_string()));
            }
        }
        let matches = backend.list_versions_matching(prefix)?;
        let v = match matches.last() {
            Some(v) => v,
            None => prefix,
            // None => Err(VersionNotFound(plugin.name.clone(), prefix.to_string()))?,
        };
        Ok(Self::new(request, v.to_string()))
    }

    fn resolve_ref(
        ref_: String,
        ref_type: String,
        opts: ToolVersionOptions,
        tr: &ToolRequest,
    ) -> Self {
        let request = ToolRequest::Ref {
            backend: tr.ba().clone(),
            ref_,
            ref_type,
            os: None,
            options: opts.clone(),
            source: tr.source().clone(),
        };
        let version = request.version();
        Self::new(request, version)
    }

    fn resolve_path(path: PathBuf, tr: &ToolRequest) -> Result<ToolVersion> {
        let path = fs::canonicalize(path)?;
        let request = ToolRequest::Path {
            backend: tr.ba().clone(),
            path,
            source: tr.source().clone(),
            os: None,
            options: tr.options().clone(),
        };
        let version = request.version();
        Ok(Self::new(request, version))
    }
}

impl Display for ToolVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}@{}", &self.ba().full(), &self.version)
    }
}

impl PartialEq for ToolVersion {
    fn eq(&self, other: &Self) -> bool {
        self.ba() == other.ba() && self.version == other.version
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
        match self.request.ba().cmp(other.ba()) {
            Ordering::Equal => self.version.cmp(&other.version),
            o => o,
        }
    }
}

impl Hash for ToolVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ba().hash(state);
        self.version.hash(state);
    }
}

#[derive(Debug, Clone)]
pub struct ResolveOptions {
    pub latest_versions: bool,
    pub use_locked_version: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            latest_versions: false,
            use_locked_version: true,
        }
    }
}
