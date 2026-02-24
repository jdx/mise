use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::file;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::{self, Lockfile, PlatformInfo};
use crate::toolset::ToolSource;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, dirs, parallel};
use async_trait::async_trait;
use eyre::Result;
use itertools::Itertools;
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseStrictness,
    Platform as CondaPlatform, RepoDataRecord,
};
use rattler_repodata_gateway::{Gateway, RepoData};
use rattler_solve::{
    ChannelPriority, SolveStrategy, SolverImpl, SolverTask, resolvo::Solver as ResolvoSolver,
};
use rattler_virtual_packages::{VirtualPackage, VirtualPackageOverrides};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use versions::Versioning;

// Shared utilities for platform-specific library path fixing
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[path = "conda_common.rs"]
mod conda_common;

// Platform-specific library path fixing modules
#[cfg(target_os = "linux")]
#[path = "conda_linux.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "conda_macos.rs"]
mod platform;

/// Conda package info stored in the shared conda-packages section of lockfiles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CondaPackageInfo {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// Conda backend requires experimental mode to be enabled
pub const EXPERIMENTAL: bool = true;

#[derive(Debug)]
pub struct CondaBackend {
    ba: Arc<BackendArg>,
}

impl CondaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn channel_name(&self) -> String {
        self.ba
            .opts()
            .get("channel")
            .cloned()
            .unwrap_or_else(|| Settings::get().conda.channel.clone())
    }

    fn channel(&self) -> Result<Channel> {
        let name = self.channel_name();
        let config = ChannelConfig::default_with_root_dir(std::path::PathBuf::from("/"));
        Channel::from_str(&name, &config)
            .map_err(|e| eyre::eyre!("invalid conda channel '{}': {}", name, e))
    }

    fn create_gateway() -> Gateway {
        Gateway::builder()
            .with_cache_dir(dirs::CACHE.join("conda"))
            .finish()
    }

    /// Map a mise PlatformTarget to a rattler conda Platform
    fn target_to_conda_platform(target: &PlatformTarget) -> CondaPlatform {
        match (target.os_name(), target.arch_name()) {
            ("linux", "x64") => CondaPlatform::Linux64,
            ("linux", "arm64") => CondaPlatform::LinuxAarch64,
            ("macos", "x64") => CondaPlatform::Osx64,
            ("macos", "arm64") => CondaPlatform::OsxArm64,
            ("windows", "x64") => CondaPlatform::Win64,
            _ => CondaPlatform::NoArch,
        }
    }

    fn detect_virtual_packages() -> Vec<GenericVirtualPackage> {
        VirtualPackage::detect(&VirtualPackageOverrides::default())
            .unwrap_or_default()
            .into_iter()
            .map(GenericVirtualPackage::from)
            .collect()
    }

    /// Flatten gateway RepoData into owned records for the solver
    fn flatten_repodata(repodata: &[RepoData]) -> Vec<RepoDataRecord> {
        repodata.iter().flat_map(|rd| rd.iter().cloned()).collect()
    }

    /// Fetch repodata and solve the conda environment for the given specs and platform.
    async fn solve_packages(
        &self,
        specs: Vec<MatchSpec>,
        platform: CondaPlatform,
    ) -> Result<Vec<RepoDataRecord>> {
        let channel = self.channel()?;
        let gateway = Self::create_gateway();

        let repodata: Vec<RepoData> = gateway
            .query([channel], [platform, CondaPlatform::NoArch], specs.clone())
            .recursive(true)
            .await
            .map_err(|e| eyre::eyre!("failed to fetch repodata: {}", e))?;

        let flat_records = Self::flatten_repodata(&repodata);
        let virtual_packages = Self::detect_virtual_packages();

        let task = SolverTask {
            available_packages: [flat_records.as_slice()],
            specs,
            virtual_packages,
            locked_packages: vec![],
            pinned_packages: vec![],
            constraints: vec![],
            timeout: None,
            channel_priority: ChannelPriority::Strict,
            exclude_newer: None,
            min_age: None,
            strategy: SolveStrategy::Highest,
        };

        let mut solver = ResolvoSolver;
        let result = solver
            .solve(task)
            .map_err(|e| eyre::eyre!("conda solve failed: {}", e))?;

        Ok(result.records)
    }

    /// Shared data dir for all conda package archives (shared across tools)
    fn conda_data_dir() -> PathBuf {
        dirs::DATA.join("conda-packages")
    }

    /// Get the filename portion of a package URL
    fn url_filename(url: &url::Url) -> String {
        url.path_segments()
            .and_then(|s| s.last())
            .unwrap_or("package")
            .to_string()
    }

    /// Strip .conda or .tar.bz2 extension to get the basename key
    fn record_basename(record: &RepoDataRecord) -> String {
        let filename = Self::url_filename(&record.url);
        filename
            .strip_suffix(".conda")
            .or_else(|| filename.strip_suffix(".tar.bz2"))
            .unwrap_or(&filename)
            .to_string()
    }

    /// Format sha256 as "sha256:<hex>" if present
    fn format_sha256(record: &RepoDataRecord) -> Option<String> {
        record
            .package_record
            .sha256
            .as_ref()
            .map(|h| format!("sha256:{}", hex::encode(h)))
    }

    /// Download a single package archive to the shared conda data dir.
    async fn download_record(record: RepoDataRecord) -> Result<PathBuf> {
        let url_str = record.url.to_string();
        let filename = Self::url_filename(&record.url);
        let dest = Self::conda_data_dir().join(&filename);

        if dest.exists() {
            return Ok(dest);
        }

        file::create_dir_all(&Self::conda_data_dir())?;
        HTTP.download_file(&url_str, &dest, None).await?;
        Ok(dest)
    }

    /// Download a package by raw URL string (for locked installs).
    async fn download_url(url_str: String) -> Result<PathBuf> {
        let filename = url_str.rsplit('/').next().unwrap_or("package").to_string();
        let dest = Self::conda_data_dir().join(&filename);

        if dest.exists() {
            return Ok(dest);
        }

        file::create_dir_all(&Self::conda_data_dir())?;
        HTTP.download_file(&url_str, &dest, None).await?;
        Ok(dest)
    }

    /// Extract a downloaded conda package archive into dest using rattler.
    async fn extract_package(archive: &std::path::Path, dest: &std::path::Path) -> Result<()> {
        rattler_package_streaming::tokio::fs::extract(archive, dest)
            .await
            .map_err(|e| eyre::eyre!("failed to extract {}: {}", archive.display(), e))?;
        Ok(())
    }

    fn read_lockfile_for_tool(&self, tv: &ToolVersion) -> Result<Lockfile> {
        match tv.request.source() {
            ToolSource::MiseToml(path) => {
                let (lockfile_path, _) = lockfile::lockfile_path_for_config(path);
                Lockfile::read(&lockfile_path)
            }
            _ => Ok(Lockfile::default()),
        }
    }

    /// Install from a fresh solve (no lockfile deps).
    async fn install_fresh(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        platform_key: &str,
    ) -> Result<()> {
        let tool_name = self.tool_name();
        let spec_str = format!("{}=={}", tool_name, tv.version);
        let match_spec = MatchSpec::from_str(&spec_str, ParseStrictness::Lenient)
            .map_err(|e| eyre::eyre!("invalid conda spec '{}': {}", spec_str, e))?;

        ctx.pr.set_message("fetching repodata".to_string());
        let records = self
            .solve_packages(vec![match_spec], CondaPlatform::current())
            .await?;

        // Separate main package from deps
        let tool_name_norm = tool_name.to_lowercase();
        let (main_vec, dep_records): (Vec<_>, Vec<_>) = records
            .into_iter()
            .partition(|r| r.package_record.name.as_normalized() == tool_name_norm);

        let main_record = main_vec
            .into_iter()
            .next()
            .ok_or_else(|| eyre::eyre!("main package {} not found in solve result", tool_name))?;

        // Build ordered list: deps first, main last
        let mut all_records = dep_records;
        all_records.push(main_record.clone());

        // Download all in parallel
        ctx.pr
            .set_message(format!("downloading {} packages", all_records.len()));
        let downloaded = parallel::parallel(all_records.clone(), Self::download_record).await?;

        // Extract into install dir
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;

        for (record, archive) in all_records.iter().zip(downloaded.iter()) {
            let name = record.package_record.name.as_normalized();
            ctx.pr.set_message(format!("extracting {name}"));
            Self::extract_package(archive, &install_path).await?;
        }

        // Fix library paths
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        platform::fix_library_paths(ctx, &install_path)?;

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        conda_common::fix_text_prefixes(&install_path);

        Self::make_bins_executable(&install_path)?;

        // Store lockfile info
        let n_deps = all_records.len() - 1; // all except main
        let dep_basenames: Vec<String> = all_records[..n_deps]
            .iter()
            .map(Self::record_basename)
            .collect();

        let platform_info = tv
            .lock_platforms
            .entry(platform_key.to_string())
            .or_default();
        platform_info.url = Some(main_record.url.to_string());
        platform_info.checksum = Self::format_sha256(&main_record);
        platform_info.conda_deps = if dep_basenames.is_empty() {
            None
        } else {
            Some(dep_basenames.clone())
        };

        // Store dep package info in tv.conda_packages for lockfile update
        for record in &all_records[..n_deps] {
            let basename = Self::record_basename(record);
            tv.conda_packages.insert(
                (platform_key.to_string(), basename),
                CondaPackageInfo {
                    url: record.url.to_string(),
                    checksum: Self::format_sha256(record),
                },
            );
        }

        Ok(())
    }

    /// Install using URLs stored in the lockfile (deterministic/reproducible path).
    async fn install_from_locked(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        platform_key: &str,
    ) -> Result<()> {
        ctx.pr.set_message("using locked dependencies".to_string());

        let platform_info = tv
            .lock_platforms
            .get(platform_key)
            .ok_or_else(|| eyre::eyre!("no lock info for platform {}", platform_key))?;

        let main_url = platform_info
            .url
            .as_ref()
            .ok_or_else(|| eyre::eyre!("no URL in lockfile for {}", self.tool_name()))?
            .clone();

        let dep_basenames = platform_info.conda_deps.clone().unwrap_or_default();
        let lockfile = self.read_lockfile_for_tool(tv)?;

        // Collect dep URLs from lockfile (deps first, main last)
        let mut urls: Vec<String> = vec![];
        for basename in &dep_basenames {
            if let Some(pkg_info) = lockfile.get_conda_package(platform_key, basename) {
                urls.push(pkg_info.url.clone());
            } else {
                warn!(
                    "conda package {} not found in lockfile for {}",
                    basename, platform_key
                );
            }
        }
        urls.push(main_url);

        ctx.pr
            .set_message(format!("downloading {} packages", urls.len()));
        let downloaded = parallel::parallel(urls, Self::download_url).await?;

        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;

        for archive in &downloaded {
            let filename = archive.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            ctx.pr.set_message(format!("extracting {filename}"));
            Self::extract_package(archive, &install_path).await?;
        }

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        platform::fix_library_paths(ctx, &install_path)?;

        #[cfg(any(target_os = "macos", target_os = "linux"))]
        conda_common::fix_text_prefixes(&install_path);

        Self::make_bins_executable(&install_path)?;

        Ok(())
    }

    fn make_bins_executable(install_path: &std::path::Path) -> Result<()> {
        let bin_path = if cfg!(windows) {
            install_path.join("Library").join("bin")
        } else {
            install_path.join("bin")
        };
        if bin_path.exists() {
            for entry in std::fs::read_dir(&bin_path)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    file::make_executable(&path)?;
                }
            }
        }
        Ok(())
    }

    /// Resolve conda packages for lockfile's shared conda-packages section.
    /// Returns a map of basename -> CondaPackageInfo for deps of this tool on the given platform.
    pub async fn resolve_conda_packages(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, CondaPackageInfo>> {
        let platform = Self::target_to_conda_platform(target);
        let tool_name = self.tool_name();
        let spec_str = format!("{}=={}", tool_name, tv.version);
        let match_spec = MatchSpec::from_str(&spec_str, ParseStrictness::Lenient)
            .map_err(|e| eyre::eyre!("invalid conda spec '{}': {}", spec_str, e))?;

        let records = self.solve_packages(vec![match_spec], platform).await?;

        let tool_name_norm = tool_name.to_lowercase();
        let mut result = BTreeMap::new();
        for record in &records {
            if record.package_record.name.as_normalized() == tool_name_norm {
                continue;
            }
            let basename = Self::record_basename(record);
            result.insert(
                basename,
                CondaPackageInfo {
                    url: record.url.to_string(),
                    checksum: Self::format_sha256(record),
                },
            );
        }

        Ok(result)
    }
}

#[async_trait]
impl Backend for CondaBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Conda
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let channel = self.channel()?;
        let current_platform = CondaPlatform::current();
        let tool_name = self.tool_name();

        let gateway = Self::create_gateway();
        let match_spec = MatchSpec::from_str(&tool_name, ParseStrictness::Lenient)
            .map_err(|e| eyre::eyre!("invalid match spec for '{}': {}", tool_name, e))?;

        let repodata: Vec<RepoData> = gateway
            .query(
                [channel],
                [current_platform, CondaPlatform::NoArch],
                [match_spec],
            )
            .await
            .map_err(|e| eyre::eyre!("failed to list versions for '{}': {}", tool_name, e))?;

        // Collect unique versions across all repodata results
        let mut version_set: std::collections::HashSet<String> = std::collections::HashSet::new();
        for data in &repodata {
            for record in data {
                version_set.insert(record.package_record.version.to_string());
            }
        }

        let versions = version_set
            .into_iter()
            .map(|version| VersionInfo {
                version,
                ..Default::default()
            })
            .sorted_by_cached_key(|v| Versioning::new(&v.version))
            .collect();

        Ok(versions)
    }

    /// Override to bypass the shared remote_versions cache since conda's
    /// channel option affects which versions are available.
    async fn list_remote_versions_with_info(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<VersionInfo>> {
        self._list_remote_versions(config).await
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        Settings::get().ensure_experimental("conda backend")?;

        let platform_key = self.get_platform_key();
        let has_locked = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.conda_deps.as_ref())
            .is_some();

        if has_locked {
            self.install_from_locked(ctx, &tv, &platform_key).await?;
        } else {
            self.install_fresh(ctx, &mut tv, &platform_key).await?;
        }

        Ok(tv)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let platform = Self::target_to_conda_platform(target);
        let tool_name = self.tool_name();
        let spec_str = format!("{}=={}", tool_name, tv.version);

        let match_spec = match MatchSpec::from_str(&spec_str, ParseStrictness::Lenient) {
            Ok(s) => s,
            Err(e) => {
                debug!("invalid conda spec '{}': {}", spec_str, e);
                return Ok(PlatformInfo::default());
            }
        };

        let records = match self.solve_packages(vec![match_spec], platform).await {
            Ok(r) => r,
            Err(e) => {
                debug!(
                    "failed to resolve {} for {}: {}",
                    tool_name,
                    target.to_key(),
                    e
                );
                return Ok(PlatformInfo::default());
            }
        };

        let tool_name_norm = tool_name.to_lowercase();
        let mut main_record = None;
        let mut dep_basenames: Vec<String> = vec![];

        for record in &records {
            if record.package_record.name.as_normalized() == tool_name_norm {
                main_record = Some(record.clone());
            } else {
                dep_basenames.push(Self::record_basename(record));
            }
        }

        match main_record {
            Some(main) => Ok(PlatformInfo {
                url: Some(main.url.to_string()),
                checksum: Self::format_sha256(&main),
                size: None,
                url_api: None,
                conda_deps: if dep_basenames.is_empty() {
                    None
                } else {
                    Some(dep_basenames)
                },
            }),
            None => Ok(PlatformInfo::default()),
        }
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let install_path = tv.install_path();
        if cfg!(windows) {
            Ok(vec![install_path.join("Library").join("bin")])
        } else {
            Ok(vec![install_path.join("bin")])
        }
    }
}
