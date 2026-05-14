use std::fmt::{Display, Formatter};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::{cmp::Ordering, sync::LazyLock};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::{ABackend, VersionInfo};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::env;
#[cfg(windows)]
use crate::file;
use crate::hash::hash_to_str;
use crate::install_before::resolve_before_date;
use crate::lockfile::{CondaPackageInfo, LockfileTool, PlatformInfo};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolRequest, ToolSource, ToolVersionOptions, tool_request};
use console::style;
use dashmap::DashMap;
use eyre::{Result, bail};
use jiff::Timestamp;
#[cfg(windows)]
use path_absolutize::Absolutize;

static INSTALL_PATH_CACHE: LazyLock<DashMap<ToolVersion, PathBuf>> = LazyLock::new(DashMap::new);

/// Clear the install_path cache. Called when install state is reset
/// to avoid stale paths (e.g. shared dir paths after a new install).
pub fn reset_install_path_cache() {
    INSTALL_PATH_CACHE.clear();
}

/// represents a single version of a tool for a particular plugin
#[derive(Debug, Clone)]
pub struct ToolVersion {
    pub request: ToolRequest,
    pub version: String,
    /// Effective install-before cutoff used to resolve this version.
    pub before_date: Option<Timestamp>,
    locked: bool,
    pub lock_platforms: BTreeMap<String, PlatformInfo>,
    pub install_path: Option<PathBuf>,
    /// Conda packages resolved during installation: (platform, basename) -> CondaPackageInfo
    pub conda_packages: BTreeMap<(String, String), CondaPackageInfo>,
}

impl ToolVersion {
    fn no_versions_found(backend: &ABackend, before_date: Option<Timestamp>) -> eyre::Report {
        let msg = if before_date.is_some() {
            format!(
                "no versions found for {} matching date filter",
                backend.id()
            )
        } else {
            format!("no versions found for {}", backend.id())
        };
        eyre::eyre!(msg)
    }

    pub fn new(request: ToolRequest, version: String) -> Self {
        ToolVersion {
            request,
            version,
            before_date: None,
            locked: false,
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
        let minimum_release_age = request.options().minimum_release_age().map(str::to_string);
        let mut opts = opts.clone();
        opts.before_date = resolve_before_date(opts.before_date, minimum_release_age.as_deref())?;

        trace!("resolving {} {}", &request, opts);
        if opts.use_locked_version
            && !has_linked_version(request.ba())
            && let Some(lt) = request.lockfile_resolve(config)?
        {
            return Ok(Self::from_lockfile(request.clone(), lt).with_before_date(opts.before_date));
        }
        let backend = request.ba().backend()?;
        if let Some(plugin) = backend.plugin()
            && !plugin.is_installed()
        {
            let tv = Self::new(request.clone(), request.version());
            return Ok(tv.with_before_date(opts.before_date));
        }
        let tv = match request.clone() {
            ToolRequest::Version { version: v, .. } => {
                Self::resolve_version(config, request, &v, &opts).await?
            }
            ToolRequest::Prefix { prefix, .. } => {
                Self::resolve_prefix(config, request, &prefix, &opts).await?
            }
            ToolRequest::Sub {
                sub, orig_version, ..
            } => Self::resolve_sub(config, request, &sub, &orig_version, &opts).await?,
            _ => {
                let version = request.version();
                Self::new(request, version)
            }
        };
        let tv = tv.with_before_date(opts.before_date);
        trace!("resolved: {tv}");
        Ok(tv)
    }

    fn with_before_date(mut self, before_date: Option<Timestamp>) -> Self {
        self.before_date = before_date;
        self
    }

    fn from_lockfile(request: ToolRequest, lt: LockfileTool) -> Self {
        let mut tv = Self::new(request, lt.version);
        tv.locked = true;
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
        if let Some(p) = INSTALL_PATH_CACHE.get(self) {
            return p.clone();
        }
        let pathname = match &self.request {
            ToolRequest::Path { path: p, .. } => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(),
        };
        let path = self.ba().installs_path.join(&pathname);

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

        // Check shared install directories if the primary path doesn't exist
        let path = if matches!(&self.request, ToolRequest::Path { .. }) {
            path
        } else {
            env::find_in_shared_installs(path, &self.ba().tool_dir_name(), &pathname)
        };

        // Only cache the resolved path if it actually exists on disk. Otherwise
        // the answer may change once the tool installs into a different
        // location (e.g. a shared install dir created mid-run by `--system` /
        // `--shared`), and the stale cache entry would be returned to callers
        // like core go's `exec_env`, sending wrong values for GOROOT/GOPATH.
        if path.exists() {
            INSTALL_PATH_CACHE.insert(self.clone(), path.clone());
        }
        path
    }
    pub fn runtime_path(&self) -> PathBuf {
        if self.locked {
            return self.install_path();
        }
        let Some(pathname) = self.runtime_pathname() else {
            return self.install_path();
        };
        let path = self.ba().installs_path.join(&pathname);
        let path = env::find_in_shared_installs(path, &self.ba().tool_dir_name(), &pathname);
        if path.is_dir() && is_runtime_symlink(&path) {
            return path;
        }

        #[cfg(windows)]
        if path.is_file()
            && is_runtime_symlink(&path)
            && let Ok(Some(target)) = file::resolve_symlink(&path)
            && let Some(parent) = path.parent()
        {
            let target = parent.join(target);
            if target.is_dir() {
                return target
                    .absolutize()
                    .expect("failed to absolutize path")
                    .to_path_buf();
            }
        }

        self.install_path()
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
            offline: base_opts.offline,
            refresh_remote_versions: base_opts.refresh_remote_versions,
            inactive: base_opts.inactive,
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
    fn runtime_pathname(&self) -> Option<String> {
        let pathname = match &self.request {
            ToolRequest::Version { version, .. } if version != &self.version => version,
            ToolRequest::Prefix { prefix, .. } => prefix,
            _ => return None,
        };
        Some(pathname.replace([':', '/'], "-"))
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
        let settings = Settings::get();
        if settings.locked
            && opts.use_locked_version
            && settings.lockfile_enabled()
            && !has_linked_version(request.ba())
            && request.source().path().is_some()
        {
            bail!(
                "{}@{} is not in the lockfile\nhint: Run `mise install` without --locked to update the lockfile",
                request.ba().short,
                request.version()
            );
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
        let is_offline = settings.offline() || opts.offline;
        let prefer_offline = settings.prefer_offline();
        let should_filter_installed_versions =
            opts.before_date.is_some() && !is_offline && !prefer_offline;

        if v == "latest" {
            if !opts.latest_versions
                && !should_filter_installed_versions
                && let Some(v) = backend.latest_installed_version(None)?
            {
                return build(v);
            }
            if !is_offline
                && let Some(v) = backend
                    .latest_version_with_refresh(
                        config,
                        None,
                        opts.before_date,
                        opts.refresh_remote_versions,
                    )
                    .await?
            {
                return build(v);
            }
            if !is_offline {
                let versions = backend
                    .list_remote_versions_with_refresh(config, opts.refresh_remote_versions)
                    .await?;
                if versions.is_empty()
                    && let Some(v) = backend.unresolved_latest_version()
                {
                    return build(v);
                }
            }
            // Prune-style offline (opts.offline) wants a non-erroring no-op
            // when nothing is installed — the literal "latest" can't match
            // any installed pathname so it's safe. Global MISE_OFFLINE keeps
            // the original error to avoid surprising upgrade/outdated callers.
            if opts.offline {
                return build(v);
            }
            return Err(Self::no_versions_found(&backend, opts.before_date));
        }
        if !opts.latest_versions {
            let matches = backend.list_installed_versions_matching(&v);
            if matches.contains(&v) {
                return build(v);
            }
            if !should_filter_installed_versions && let Some(v) = matches.last() {
                return build(v.clone());
            }
        }
        if matches!(
            request.source(),
            ToolSource::IdiomaticVersionFile(path)
                if crate::config::config_file::idiomatic_version::package_json::is_package_json(path)
        ) && crate::semver::is_npm_semver_range_query(&v)
        {
            if !opts.latest_versions && !should_filter_installed_versions {
                let installed_versions = backend.list_installed_versions();
                if let Some(matches) =
                    crate::semver::npm_semver_range_filter(&installed_versions, &v)
                    && let Some(v) = matches.last()
                {
                    return build(v.clone());
                }
            }
            if !is_offline {
                let versions = match opts.before_date {
                    Some(before) => {
                        let versions_with_info = backend
                            .list_remote_versions_with_info_with_refresh(
                                config,
                                opts.refresh_remote_versions,
                            )
                            .await?;
                        VersionInfo::filter_by_date(versions_with_info, before)
                            .into_iter()
                            .map(|v| v.version)
                            .collect()
                    }
                    None => {
                        backend
                            .list_remote_versions_with_refresh(config, opts.refresh_remote_versions)
                            .await?
                    }
                };
                if let Some(matches) = crate::semver::npm_semver_range_filter(&versions, &v)
                    && let Some(v) = matches.last()
                {
                    return build(v.clone());
                }
            }
        }
        // When OFFLINE, skip ALL remote version fetching regardless of version format
        if is_offline {
            return build(v);
        }
        // In prefer-offline mode (hook-env, activate, exec), skip remote version
        // fetching for fully-qualified versions (e.g. "2.3.2") that aren't installed.
        // Prefix versions like "2" still need remote resolution to find e.g. "2.1.0".
        // "latest" also needs remote resolution but is handled in the block above.
        if settings.prefer_offline() && v.matches('.').count() >= 2 {
            return build(v);
        }
        // First try with date filter (common case)
        let matches = backend
            .list_versions_matching_with_opts(
                config,
                &v,
                opts.before_date,
                opts.refresh_remote_versions,
            )
            .await?;
        if matches.contains(&v) {
            return build(v);
        }
        if let Some(v) = matches.last() {
            return build(v.clone());
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
        build(v)
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
        if v == "latest" && opts.offline {
            // Can't resolve sub-N:latest offline (no remote latest, and
            // applying version_sub to latest_installed_version would shift
            // one step too low). Return the raw spec; callers that care
            // (`get_versions_needed_by_tracked_configs`) over-protect by
            // keeping all installed versions of this backend.
            let version = request.version();
            return Ok(Self::new(request, version));
        }
        let v = match v {
            "latest" => backend
                .latest_version_with_refresh(
                    config,
                    None,
                    opts.before_date,
                    opts.refresh_remote_versions,
                )
                .await?
                .ok_or_else(|| Self::no_versions_found(&backend, opts.before_date))?,
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
        let settings = Settings::get();
        let is_offline = settings.offline() || opts.offline;
        let should_filter_installed_versions =
            opts.before_date.is_some() && !is_offline && !settings.prefer_offline();
        if !opts.latest_versions
            && !should_filter_installed_versions
            && let Some(v) = backend.list_installed_versions_matching(prefix).last()
        {
            return Ok(Self::new(request, v.to_string()));
        }
        if opts.offline {
            return Ok(Self::new(request, prefix.to_string()));
        }
        let matches = backend
            .list_versions_matching_with_opts(
                config,
                prefix,
                opts.before_date,
                opts.refresh_remote_versions,
            )
            .await?;
        let v = matches
            .last()
            .ok_or_else(|| Self::no_versions_found(&backend, opts.before_date))?;
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
    /// Additive to `Settings::offline()` — either being true skips remote version listing.
    pub offline: bool,
    /// Ignore cached remote version lists while resolving this request.
    pub refresh_remote_versions: bool,
    /// Include installed-but-inactive versions that do not have a known config source
    /// (for example `ToolSource::Unknown`) when resolving tools for flows like
    /// outdated/upgrade checks.
    pub inactive: bool,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            latest_versions: false,
            use_locked_version: true,
            before_date: None,
            offline: false,
            refresh_remote_versions: false,
            inactive: false,
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
        if self.offline {
            opts.push("offline".to_string());
        }
        if self.refresh_remote_versions {
            opts.push("refresh_remote_versions".to_string());
        }
        write!(f, "({})", opts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendResolution;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn runtime_path_does_not_return_file_based_runtime_symlink() -> Result<()> {
        reset_install_path_cache();

        let temp_dir = tempfile::tempdir()?;
        let short = format!(
            "dummy-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut backend = BackendArg::new_raw(
            short.clone(),
            None,
            short,
            None,
            BackendResolution::new(false),
        );
        backend.installs_path = temp_dir.path().join("installs").join("dummy");

        let install_path = backend.installs_path.join("1.0.1");
        fs::create_dir_all(install_path.join("bin"))?;
        fs::write(backend.installs_path.join("1.0"), "./1.0.1")?;

        let request = ToolRequest::Version {
            backend: Arc::new(backend),
            version: "1.0".into(),
            options: ToolVersionOptions::default(),
            source: ToolSource::Argument,
        };
        let tv = ToolVersion::new(request, "1.0.1".into());

        let runtime_path = tv.runtime_path();
        assert_eq!(runtime_path, install_path);
        assert!(runtime_path.is_dir());

        Ok(())
    }

    /// Regression test for https://github.com/jdx/mise/discussions/9526
    ///
    /// `install_path()` must not cache a path that does not yet exist. If it
    /// did, a tool that first asked for its install path before the install
    /// completed would receive that cached path forever — even after the tool
    /// installed somewhere else (e.g. via `--system` into a shared install
    /// dir). Subsequent callers like core go's `exec_env` would then export
    /// the wrong GOROOT/GOPATH, breaking go-backend tools that depend on go.
    #[test]
    fn install_path_does_not_cache_nonexistent_paths() -> Result<()> {
        reset_install_path_cache();

        let temp_dir = tempfile::tempdir()?;
        let short = format!(
            "dummy-cache-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut backend = BackendArg::new_raw(
            short.clone(),
            None,
            short,
            None,
            BackendResolution::new(false),
        );
        backend.installs_path = temp_dir.path().join("installs").join("dummy-cache");

        let request = ToolRequest::Version {
            backend: Arc::new(backend),
            version: "1.0.0".into(),
            options: ToolVersionOptions::default(),
            source: ToolSource::Argument,
        };
        let tv = ToolVersion::new(request, "1.0.0".into());

        // First call: nothing exists yet. Should return the primary path but
        // must NOT populate the cache.
        let p1 = tv.install_path();
        assert!(!p1.exists());
        assert!(INSTALL_PATH_CACHE.get(&tv).is_none());

        // Now "install" the tool by creating the dir.
        fs::create_dir_all(&p1)?;

        // Second call should still return the same path; this time it exists
        // so the cache is populated.
        let p2 = tv.install_path();
        assert_eq!(p1, p2);
        assert!(p2.exists());
        assert!(INSTALL_PATH_CACHE.get(&tv).is_some());

        Ok(())
    }
}
