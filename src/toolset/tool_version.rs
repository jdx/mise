use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::{cmp::Ordering, sync::LazyLock};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::ABackend;
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
#[cfg(windows)]
use crate::file;
use crate::hash::hash_to_str;
use crate::lockfile::{CondaPackageInfo, LockfileTool, PlatformInfo};
use crate::toolset::{ToolRequest, ToolVersionOptions, tool_request};
use console::style;
use dashmap::DashMap;
use eyre::Result;
use jiff::Timestamp;
#[cfg(windows)]
use path_absolutize::Absolutize;

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolRequest,
    pub version: String,
    pub lock_platforms: BTreeMap<String, PlatformInfo>,
    pub install_path: Option<PathBuf>,
    /// Conda packages resolved during installation: (platform, basename) -> CondaPackageInfo
    pub conda_packages: BTreeMap<(String, String), CondaPackageInfo>,
}

impl ToolVersion {
    pub fn new(request: ToolRequest, version: String) -> Self {
        ToolVersion {
            request,
            version,
            lock_platforms: Default::default(),
            install_path: None,
            conda_packages: Default::default(),
        }
    }

    pub async fn resolve(
        config: &Arc<Config>,
        request: ToolRequest,
        opts: &ResolveOptions,
    ) -> Result<Self> {
        trace!("resolving {} {}", &request, opts);
        if opts.use_locked_version
            && !has_linked_version(request.ba())
            && let Some(lt) = request.lockfile_resolve(config)?
        {
            return Ok(Self::from_lockfile(request.clone(), lt));
        }
        let backend = request.ba().backend()?;
        if let Some(plugin) = backend.plugin()
            && !plugin.is_installed()
        {
            let tv = Self::new(request.clone(), request.version());
            return Ok(tv);
        }
        let tv = match request.clone() {
            ToolRequest::Version { version: v, .. } => {
                Self::resolve_version(config, request, &v, opts).await?
            }
            ToolRequest::Prefix { prefix, .. } => {
                Self::resolve_prefix(config, request, &prefix, opts).await?
            }
            ToolRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(config, request, &sub, &orig_version, opts).await?,
            _ => {
                let version = request.version();
                Self::new(request, version)
            }
        };
        trace!("resolved: {tv}");
        Ok(tv)
    }

    fn from_lockfile(request: ToolRequest, lt: LockfileTool) -> Self {
        let mut tv = Self::new(request, lt.version);
        tv.lock_platforms = lt.platforms;
        tv
    }

    pub fn ba(&self) -> &BackendArg {
        self.request.ba()
    }

    pub fn backend(&self) -> Result<ABackend> {
        self.ba().backend()
    }

    pub fn short(&self) -> &str {
        &self.ba().short
    }

    pub fn install_path(&self) -> PathBuf {
        if let Some(p) = &self.install_path {
            return p.clone();
        }
        static CACHE: LazyLock<DashMap<ToolVersion, PathBuf>> = LazyLock::new(DashMap::new);
        if let Some(p) = CACHE.get(self) {
            return p.clone();
        }
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
        CACHE.insert(self.clone(), path.clone());
        path
    }
    pub fn cache_path(&self) -> PathBuf {
        self.ba().cache_path.join(self.tv_pathname())
    }
    pub fn download_path(&self) -> PathBuf {
        self.request.ba().downloads_path.join(self.tv_pathname())
    }
    pub async fn latest_version(&self, config: &Arc<Config>) -> Result<String> {
        self.latest_version_with_opts(config, &ResolveOptions::default())
            .await
    }

    pub async fn latest_version_with_opts(
        &self,
        config: &Arc<Config>,
        base_opts: &ResolveOptions,
    ) -> Result<String> {
        // Note: We always use latest_versions=true and use_locked_version=false for latest version lookup,
        // but we preserve before_date from base_opts to respect date-based filtering
        let opts = ResolveOptions {
            latest_versions: true,
            use_locked_version: false,
            before_date: base_opts.before_date,
        };
        let tv = self.request.resolve(config, &opts).await?;
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
            ToolRequest::Ref { ref_: r, .. } => format!("ref-{r}"),
            ToolRequest::Path { path: p, .. } => format!("path-{}", hash_to_str(p)),
            ToolRequest::System { .. } => {
                // Only show deprecation warning if not from .tool-versions file
                if !matches!(
                    self.request.source(),
                    crate::toolset::ToolSource::ToolVersions(_)
                ) {
                    deprecated!(
                        "system_tool_version",
                        "@system is deprecated, use MISE_DISABLE_TOOLS instead"
                    );
                }
                "system".to_string()
            }
        }
        .replace([':', '/'], "-")
    }
    async fn resolve_version(
        config: &Arc<Config>,
        request: ToolRequest,
        v: &str,
        opts: &ResolveOptions,
    ) -> Result<ToolVersion> {
        let backend = request.backend()?;
        let v = config.resolve_alias(&backend, v).await?;

        // Re-check the lockfile after alias resolution (e.g., "lts" → "24")
        // The initial lockfile check in resolve() uses the unresolved alias which
        // won't match lockfile entries like "24.13.0".starts_with("lts")
        if opts.use_locked_version
            && !has_linked_version(request.ba())
            && let Some(lt) = request.lockfile_resolve_with_prefix(config, &v)?
        {
            return Ok(Self::from_lockfile(request.clone(), lt));
        }

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
                return Self::resolve_prefix(config, request, p, opts).await;
            }
            Some((part, v)) if part.starts_with("sub-") => {
                let sub = part.split_once('-').unwrap().1;
                return Self::resolve_sub(config, request, sub, v, opts).await;
            }
            _ => (),
        }

        let build = |v| Ok(Self::new(request.clone(), v));

        if let Some(plugin) = backend.plugin()
            && !plugin.is_installed()
        {
            return build(v);
        }

        let settings = Settings::get();
        let is_offline = settings.offline();

        if v == "latest" {
            if !opts.latest_versions
                && let Some(v) = backend.latest_installed_version(None)?
            {
                return build(v);
            }
            if !is_offline
                && let Some(v) = backend
                    .latest_version_with_opts(config, None, opts.before_date)
                    .await?
            {
                return build(v);
            }
        }
        if !opts.latest_versions {
            let matches = backend.list_installed_versions_matching(&v);
            if matches.contains(&v) {
                return build(v);
            }
            if let Some(v) = matches.last() {
                return build(v.clone());
            }
        }
        // When OFFLINE, skip ALL remote version fetching regardless of version format
        if is_offline {
            return build(v);
        }
        // In prefer-offline mode (hook-env, activate, exec), skip remote version
        // fetching for fully-qualified versions (e.g. "2.3.2") that aren't installed.
        // Prefix versions like "2" still need remote resolution to find e.g. "2.1.0".
        if settings.prefer_offline() && v.matches('.').count() >= 2 {
            return build(v);
        }
        // First try with date filter (common case)
        let matches = backend
            .list_versions_matching_with_opts(config, &v, opts.before_date)
            .await?;
        if matches.contains(&v) {
            return build(v);
        }
        // If date filter is active and exact version not found, check without filter.
        // Explicit pinned versions like "22.5.0" should not be filtered by date.
        if opts.before_date.is_some() {
            let all_versions = backend.list_versions_matching(config, &v).await?;
            if all_versions.contains(&v) {
                // Exact match exists but was filtered by date - use it anyway
                return build(v);
            }
        }
        Self::resolve_prefix(config, request, &v, opts).await
    }

    /// resolve a version like `sub-1:12.0.0` which becomes `11.0.0`, `sub-0.1:12.1.0` becomes `12.0.0`
    async fn resolve_sub(
        config: &Arc<Config>,
        request: ToolRequest,
        sub: &str,
        v: &str,
        opts: &ResolveOptions,
    ) -> Result<Self> {
        let backend = request.backend()?;
        let v = match v {
            "latest" => backend
                .latest_version_with_opts(config, None, opts.before_date)
                .await?
                .ok_or_else(|| {
                    let msg = if opts.before_date.is_some() {
                        format!(
                            "no versions found for {} matching date filter",
                            backend.id()
                        )
                    } else {
                        format!("no versions found for {}", backend.id())
                    };
                    eyre::eyre!(msg)
                })?,
            _ => config.resolve_alias(&backend, v).await?,
        };
        let v = tool_request::version_sub(&v, sub);
        Box::pin(Self::resolve_version(config, request, &v, opts)).await
    }

    async fn resolve_prefix(
        config: &Arc<Config>,
        request: ToolRequest,
        prefix: &str,
        opts: &ResolveOptions,
    ) -> Result<Self> {
        let backend = request.backend()?;
        if !opts.latest_versions
            && let Some(v) = backend.list_installed_versions_matching(prefix).last()
        {
            return Ok(Self::new(request, v.to_string()));
        }
        let matches = backend
            .list_versions_matching_with_opts(config, prefix, opts.before_date)
            .await?;
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
        match self.request.ba().as_ref().cmp(other.ba()) {
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
    /// Only consider versions released before this timestamp
    pub before_date: Option<Timestamp>,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            latest_versions: false,
            use_locked_version: true,
            before_date: None,
        }
    }
}

/// Check if a tool has any user-linked versions (created by `mise link`).
/// A linked version is an installed version whose path is a symlink to an absolute path,
/// as opposed to runtime symlinks which point to relative paths (starting with "./").
fn has_linked_version(ba: &BackendArg) -> bool {
    let installs_dir = &ba.installs_path;
    let Ok(entries) = std::fs::read_dir(installs_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(Some(target)) = crate::file::resolve_symlink(&path) {
            // Runtime symlinks start with "./" (e.g., latest -> ./1.35.0)
            // User-linked symlinks point to absolute paths (e.g., brew -> /opt/homebrew/opt/hk)
            if target.is_absolute() {
                return true;
            }
        }
    }
    false
}

impl Display for ResolveOptions {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let mut opts = vec![];
        if self.latest_versions {
            opts.push("latest_versions".to_string());
        }
        if self.use_locked_version {
            opts.push("use_locked_version".to_string());
        }
        if let Some(ts) = &self.before_date {
            opts.push(format!("before_date={ts}"));
        }
        write!(f, "({})", opts.join(", "))
    }
}
