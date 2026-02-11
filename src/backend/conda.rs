use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::Config;
use crate::config::Settings;
use crate::file::{self, TarOptions};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::lockfile::{self, Lockfile, PlatformInfo};
use crate::toolset::ToolSource;
use crate::toolset::ToolVersion;
use crate::{backend::Backend, dirs, hash, http::HTTP, parallel};
use async_trait::async_trait;
use eyre::{Result, bail};
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
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

/// Map OS/arch pair to conda subdir format
fn platform_to_conda_subdir(os: &str, arch: &str) -> &'static str {
    match (os, arch) {
        ("linux", "x64") => "linux-64",
        ("linux", "arm64") => "linux-aarch64",
        ("macos", "x64") => "osx-64",
        ("macos", "arm64") => "osx-arm64",
        ("windows", "x64") => "win-64",
        _ => "noarch",
    }
}

impl CondaBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    /// Get the conda channel from settings or tool options
    fn channel(&self) -> String {
        self.ba
            .opts()
            .get("channel")
            .cloned()
            .unwrap_or_else(|| Settings::get().conda.channel.clone())
    }

    /// Map mise OS/ARCH to conda subdir
    fn conda_subdir() -> &'static str {
        platform_to_conda_subdir(OS.as_str(), ARCH.as_str())
    }

    /// Map PlatformTarget to conda subdir for lockfile resolution
    fn conda_subdir_for_platform(target: &PlatformTarget) -> &'static str {
        platform_to_conda_subdir(target.os_name(), target.arch_name())
    }

    /// Build a proper download URL from the API response
    fn build_download_url(download_url: &str) -> String {
        if download_url.starts_with("//") {
            format!("https:{}", download_url)
        } else if download_url.starts_with('/') {
            format!("https://conda.anaconda.org{}", download_url)
        } else {
            download_url.to_string()
        }
    }

    /// Fetch package files from the anaconda.org API for a given package
    async fn fetch_package_files_for(&self, package_name: &str) -> Result<Vec<CondaPackageFile>> {
        let channel = self.channel();
        let url = format!(
            "https://api.anaconda.org/package/{}/{}/files",
            channel, package_name
        );
        let files: Vec<CondaPackageFile> = HTTP_FETCH.json(&url).await?;
        Ok(files)
    }

    /// Fetch package files from the anaconda.org API for this tool
    async fn fetch_package_files(&self) -> Result<Vec<CondaPackageFile>> {
        self.fetch_package_files_for(&self.tool_name()).await
    }

    /// Find the best package file for a given version and platform
    /// Prefers platform-specific packages over noarch, and .conda format over .tar.bz2
    fn find_package_file<'a>(
        files: &'a [CondaPackageFile],
        version: Option<&str>,
        subdir: &str,
    ) -> Option<&'a CondaPackageFile> {
        // Try platform-specific packages first, then fall back to noarch
        Self::find_package_file_for_subdir(files, version, subdir)
            .or_else(|| Self::find_package_file_for_subdir(files, version, "noarch"))
    }

    /// Find the best package file for a given version and specific subdir
    /// Prefers .conda format over .tar.bz2 (newer, faster)
    fn find_package_file_for_subdir<'a>(
        files: &'a [CondaPackageFile],
        version: Option<&str>,
        subdir: &str,
    ) -> Option<&'a CondaPackageFile> {
        // Filter by exact platform match
        let platform_files: Vec<_> = files.iter().filter(|f| f.attrs.subdir == subdir).collect();

        if platform_files.is_empty() {
            return None;
        }

        // Find files matching the version spec
        let matching: Vec<_> = if let Some(ver) = version {
            platform_files
                .iter()
                .filter(|f| Self::version_matches(&f.version, ver))
                .copied()
                .collect()
        } else {
            // No version spec - get latest
            let latest = platform_files
                .iter()
                .max_by_key(|f| Versioning::new(&f.version))?;
            platform_files
                .iter()
                .filter(|f| f.version == latest.version)
                .copied()
                .collect()
        };

        if matching.is_empty() {
            return None;
        }

        // Prefer .conda format over .tar.bz2
        // Among matches, pick the latest version
        let best_version = matching
            .iter()
            .max_by_key(|f| Versioning::new(&f.version))?;

        matching
            .iter()
            .filter(|f| f.version == best_version.version)
            .find(|f| f.basename.ends_with(".conda"))
            .or_else(|| {
                matching
                    .iter()
                    .filter(|f| f.version == best_version.version)
                    .find(|f| f.basename.ends_with(".tar.bz2"))
            })
            .copied()
    }

    /// Check if a version matches a conda version spec
    /// Supports: exact match, prefix match, wildcard (*), and comparison operators
    fn version_matches(version: &str, spec: &str) -> bool {
        // Exact match
        if version == spec {
            return true;
        }

        // Wildcard pattern like "6.9.*" -> matches "6.9.anything"
        if let Some(prefix) = spec.strip_suffix(".*")
            && version.starts_with(prefix)
            && version
                .chars()
                .nth(prefix.len())
                .map(|c| c == '.')
                .unwrap_or(false)
        {
            return true;
        }

        // Single wildcard like "6.*" matches "6.anything"
        if let Some(prefix) = spec.strip_suffix('*')
            && version.starts_with(prefix)
        {
            return true;
        }

        // Prefix match (e.g., "1.7" matches "1.7.1")
        if version.starts_with(spec)
            && version
                .chars()
                .nth(spec.len())
                .map(|c| c == '.')
                .unwrap_or(false)
        {
            return true;
        }

        // Handle compound specs like ">=1.0,<2.0" by splitting on comma
        if spec.contains(',') {
            return spec
                .split(',')
                .all(|part| Self::version_matches(version, part.trim()));
        }

        // Comparison operators (>=, <=, >, <, ==, !=)
        Self::check_version_constraint(version, spec)
    }

    /// Check a single version constraint like ">=1.0" or "<2.0"
    fn check_version_constraint(version: &str, constraint: &str) -> bool {
        let v = match Versioning::new(version) {
            Some(v) => v,
            None => return false,
        };

        if let Some(spec_ver) = constraint.strip_prefix(">=") {
            if let Some(s) = Versioning::new(spec_ver) {
                return v >= s;
            }
        } else if let Some(spec_ver) = constraint.strip_prefix("<=") {
            if let Some(s) = Versioning::new(spec_ver) {
                return v <= s;
            }
        } else if let Some(spec_ver) = constraint.strip_prefix("==") {
            if let Some(s) = Versioning::new(spec_ver) {
                return v == s;
            }
        } else if let Some(spec_ver) = constraint.strip_prefix("!=") {
            if let Some(s) = Versioning::new(spec_ver) {
                return v != s;
            }
        } else if let Some(spec_ver) = constraint.strip_prefix('>') {
            if let Some(s) = Versioning::new(spec_ver) {
                return v > s;
            }
        } else if let Some(spec_ver) = constraint.strip_prefix('<')
            && let Some(s) = Versioning::new(spec_ver)
        {
            return v < s;
        }

        false
    }

    /// Extract a conda package (.conda or .tar.bz2) to the install path
    fn extract_conda_package(
        &self,
        ctx: &InstallContext,
        tarball_path: &std::path::Path,
        install_path: &std::path::Path,
    ) -> Result<()> {
        let filename = tarball_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if filename.ends_with(".conda") {
            // .conda format: ZIP containing pkg-*.tar.zst
            self.extract_conda_format(ctx, tarball_path, install_path)?;
        } else if filename.ends_with(".tar.bz2") {
            // Legacy format: plain tar.bz2
            ctx.pr.set_message(format!("extract {filename}"));
            let tar_opts = TarOptions {
                format: file::TarFormat::TarBz2,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            };
            file::untar(tarball_path, install_path, &tar_opts)?;
        } else {
            bail!("unsupported conda package format: {}", filename);
        }

        Ok(())
    }

    /// Extract .conda format (ZIP with inner tar.zst)
    fn extract_conda_format(
        &self,
        ctx: &InstallContext,
        conda_path: &std::path::Path,
        install_path: &std::path::Path,
    ) -> Result<()> {
        let filename = conda_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("package.conda");
        ctx.pr.set_message(format!("extract {filename}"));

        // Create a unique temp directory for extraction to avoid race conditions
        // when multiple processes extract different packages simultaneously
        let parent_dir = conda_path.parent().unwrap();
        let temp_dir = tempfile::tempdir_in(parent_dir)?;

        // Unzip the .conda file
        file::unzip(conda_path, temp_dir.path(), &Default::default())?;

        // Find and extract pkg-*.tar.zst
        let pkg_tar = std::fs::read_dir(temp_dir.path())?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("pkg-") && n.ends_with(".tar.zst"))
            });

        if let Some(pkg_tar_path) = pkg_tar {
            let tar_opts = TarOptions {
                format: file::TarFormat::TarZst,
                pr: Some(ctx.pr.as_ref()),
                ..Default::default()
            };
            file::untar(&pkg_tar_path, install_path, &tar_opts)?;
        } else {
            bail!("could not find pkg-*.tar.zst in .conda archive");
        }

        // temp_dir is automatically cleaned up when dropped
        Ok(())
    }

    /// Verify SHA256 checksum if available
    fn verify_checksum(tarball_path: &Path, expected_sha256: Option<&str>) -> Result<()> {
        if let Some(expected) = expected_sha256 {
            hash::ensure_checksum(tarball_path, expected, None, "sha256")?;
        }
        Ok(())
    }

    /// Get the shared conda package data directory
    /// All conda packages (main + deps) are stored here for sharing across tools
    fn conda_data_dir() -> PathBuf {
        dirs::DATA.join("conda-packages")
    }

    /// Get path for a specific package file in the data directory
    /// Uses basename which includes version+build: "clang-21.1.7-default_h489deba_0.conda"
    fn package_path(basename: &str) -> PathBuf {
        let filename = Path::new(basename)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(basename);
        Self::conda_data_dir().join(filename)
    }

    /// Recursively resolve dependencies for a package
    async fn resolve_dependencies(
        &self,
        pkg_file: &CondaPackageFile,
        subdir: &str,
        resolved: &mut HashMap<String, ResolvedPackage>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        // Extract version pins from parent package's build string.
        // e.g., vim's build string "py310pl5321h..." tells us it needs Python 3.10
        let build_pins = extract_build_pins(&pkg_file.basename);

        for dep in &pkg_file.attrs.depends {
            let Some((name, version_spec)) = parse_dependency(dep) else {
                continue;
            };

            // Skip if already resolved or being visited (circular dep protection)
            if resolved.contains_key(&name) || visited.contains(&name) {
                continue;
            }
            visited.insert(name.clone());

            // Fetch dependency package files
            let dep_files = match self.fetch_package_files_for(&name).await {
                Ok(files) => files,
                Err(e) => {
                    bail!("failed to fetch dependency '{}': {}", name, e);
                }
            };

            // If parent's build string pins this dependency version, try that first.
            // e.g., vim built with py310 should get Python 3.10.*, not 3.15
            let pinned_spec = build_pins.get(&name).map(|v| format!("{}.*", v));

            let matched = if let Some(ref pinned) = pinned_spec {
                Self::find_package_file(&dep_files, Some(pinned.as_str()), subdir).or_else(|| {
                    // Fall back to original spec if pinned version not available
                    debug!(
                        "pinned {} {} not found for {}, falling back to {:?}",
                        name, pinned, subdir, version_spec
                    );
                    Self::find_package_file(&dep_files, version_spec.as_deref(), subdir)
                })
            } else {
                Self::find_package_file(&dep_files, version_spec.as_deref(), subdir)
            };

            let Some(matched) = matched else {
                // Skip dependencies not available for this platform
                // This is common - many conda packages have platform-specific deps
                debug!(
                    "skipping dependency '{}' (spec: {:?}) - not available for platform {}",
                    name, version_spec, subdir
                );
                continue;
            };

            resolved.insert(name.clone(), matched.to_resolved_package(&name));

            // Recurse into this dependency's dependencies
            Box::pin(self.resolve_dependencies(matched, subdir, resolved, visited)).await?;
        }
        Ok(())
    }

    /// Download a conda package to shared data directory (standalone for parallel::parallel)
    async fn download_package(pkg: ResolvedPackage) -> Result<PathBuf> {
        use eyre::WrapErr;

        let data_dir = Self::conda_data_dir();
        file::create_dir_all(&data_dir)
            .wrap_err_with(|| format!("failed to create conda data dir for {}", pkg.name))?;

        let tarball_path = Self::package_path(&pkg.basename);

        // Check if file already exists with valid checksum
        if tarball_path.exists() {
            if Self::verify_checksum(&tarball_path, pkg.sha256.as_deref()).is_ok() {
                return Ok(tarball_path);
            }
            // Corrupted file - delete it
            let _ = std::fs::remove_file(&tarball_path);
        }

        // Download to a temp file first, then rename after verification
        // This ensures the final path never contains a corrupted file
        let temp_path = tarball_path.with_extension(format!(
            "{}.tmp.{}",
            tarball_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or(""),
            std::process::id()
        ));

        // Clean up any stale temp file from previous runs
        let _ = std::fs::remove_file(&temp_path);

        HTTP.download_file(&pkg.download_url, &temp_path, None)
            .await
            .wrap_err_with(|| format!("failed to download {}", pkg.download_url))?;

        // Verify checksum of downloaded file
        let file_size = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
        Self::verify_checksum(&temp_path, pkg.sha256.as_deref()).wrap_err_with(|| {
            format!(
                "checksum verification failed for {} (file size: {} bytes)",
                pkg.name, file_size
            )
        })?;

        // Rename temp file to final path (atomic on most filesystems)
        std::fs::rename(&temp_path, &tarball_path)
            .wrap_err_with(|| format!("failed to rename temp file for {}", pkg.name))?;

        Ok(tarball_path)
    }

    /// Read the lockfile for the tool's source config
    fn read_lockfile_for_tool(&self, tv: &ToolVersion) -> Result<Lockfile> {
        match tv.request.source() {
            ToolSource::MiseToml(path) => {
                let (lockfile_path, _) = lockfile::lockfile_path_for_config(path);
                Lockfile::read(&lockfile_path)
            }
            _ => Ok(Lockfile::default()),
        }
    }

    /// Resolve conda packages for the lockfile's shared conda-packages section.
    /// Returns a map of basename -> CondaPackageInfo for the given platform.
    pub async fn resolve_conda_packages(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, CondaPackageInfo>> {
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir_for_platform(target);

        let Some(pkg_file) = Self::find_package_file(&files, Some(&tv.version), subdir) else {
            return Ok(BTreeMap::new());
        };

        // Resolve dependencies for this platform
        let mut resolved = HashMap::new();
        let mut visited = HashSet::new();
        visited.insert(self.tool_name());
        self.resolve_dependencies(pkg_file, subdir, &mut resolved, &mut visited)
            .await?;

        // Convert to CondaPackageInfo map keyed by basename
        let mut result = BTreeMap::new();
        for pkg in resolved.values() {
            let basename = strip_conda_extension(&pkg.basename).to_string();
            result.insert(
                basename,
                CondaPackageInfo {
                    url: pkg.download_url.clone(),
                    checksum: pkg.sha256.as_ref().map(|s| format!("sha256:{}", s)),
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
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir();

        // Filter by current platform and group by version to get the latest upload time per version
        let mut version_times: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();

        for f in files
            .iter()
            .filter(|f| f.attrs.subdir == subdir || f.attrs.subdir == "noarch")
        {
            version_times
                .entry(f.version.clone())
                .and_modify(|existing| {
                    // Keep the latest upload time for each version
                    if let Some(new_time) = &f.upload_time
                        && (existing.is_none() || existing.as_ref().is_some_and(|e| new_time > e))
                    {
                        *existing = Some(new_time.clone());
                    }
                })
                .or_insert_with(|| f.upload_time.clone());
        }

        // Convert to VersionInfo and sort by version
        let versions: Vec<VersionInfo> = version_times
            .into_iter()
            .map(|(version, created_at)| VersionInfo {
                version,
                created_at,
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
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir();
        let platform_key = self.get_platform_key();

        // Find the package file for this version (prefers platform-specific over noarch)
        let pkg_file = Self::find_package_file(&files, Some(&tv.version), subdir);

        let pkg_file = match pkg_file {
            Some(f) => f,
            None => bail!(
                "conda package {}@{} not found for platform {}",
                self.tool_name(),
                tv.version,
                subdir
            ),
        };

        // Build main package info
        let main_pkg = pkg_file.to_resolved_package(&self.tool_name());

        // Check if we have locked dependencies
        let locked_deps = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.conda_deps.as_ref());

        // Resolve dependencies - either from lockfile or dynamically
        let (resolved, dep_basenames) = if let Some(basenames) = locked_deps {
            // Use locked dependencies - look them up in the lockfile
            ctx.pr.set_message("using locked dependencies".to_string());
            let lockfile = self.read_lockfile_for_tool(&tv)?;

            let mut resolved = HashMap::new();
            for basename in basenames {
                if let Some(pkg_info) = lockfile.get_conda_package(&platform_key, basename) {
                    let full_basename = extract_basename_from_url(&pkg_info.url);
                    resolved.insert(
                        basename.clone(),
                        ResolvedPackage {
                            name: basename.clone(), // Use basename as the key
                            download_url: pkg_info.url.clone(),
                            sha256: pkg_info.checksum.as_ref().map(|c: &String| {
                                c.strip_prefix("sha256:").unwrap_or(c).to_string()
                            }),
                            basename: full_basename,
                        },
                    );
                } else {
                    warn!(
                        "conda package {} not found in lockfile for platform {}",
                        basename, platform_key
                    );
                }
            }
            (resolved, basenames.clone())
        } else {
            // Resolve dynamically (current behavior)
            ctx.pr.set_message("resolving dependencies".to_string());
            let mut resolved = HashMap::new();
            let mut visited = HashSet::new();
            // Add main package to visited to prevent circular resolution back to it
            visited.insert(self.tool_name());
            self.resolve_dependencies(pkg_file, subdir, &mut resolved, &mut visited)
                .await?;

            // Convert to basename keys for lockfile
            let dep_basenames: Vec<String> = resolved
                .values()
                .map(|p| strip_conda_extension(&p.basename).to_string())
                .collect();

            // Re-key resolved by basename for consistency
            let resolved_by_basename: HashMap<String, ResolvedPackage> = resolved
                .into_values()
                .map(|p| (strip_conda_extension(&p.basename).to_string(), p))
                .collect();

            (resolved_by_basename, dep_basenames)
        };

        // Build list of all packages to download (deps + main)
        let mut all_packages: Vec<ResolvedPackage> = resolved.values().cloned().collect();
        all_packages.push(main_pkg.clone());

        // Download all packages in parallel
        ctx.pr
            .set_message(format!("downloading {} packages", all_packages.len()));
        let downloaded_paths =
            parallel::parallel(all_packages.clone(), Self::download_package).await?;

        // Create map of package basename -> downloaded path
        let path_map: HashMap<String, PathBuf> = all_packages
            .iter()
            .zip(downloaded_paths.iter())
            .map(|(pkg, path)| {
                (
                    strip_conda_extension(&pkg.basename).to_string(),
                    path.clone(),
                )
            })
            .collect();

        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;

        // Extract dependencies first (sequential to avoid conflicts)
        for basename in &dep_basenames {
            if let Some(tarball_path) = path_map.get(basename) {
                ctx.pr.set_message(format!("extract {basename}"));
                self.extract_conda_package(ctx, tarball_path, &install_path)?;
            }
        }

        // Extract main package last (so its files take precedence)
        let main_basename = strip_conda_extension(&main_pkg.basename);
        if let Some(main_tarball) = path_map.get(main_basename) {
            ctx.pr.set_message(format!("extract {}", self.tool_name()));
            self.extract_conda_package(ctx, main_tarball, &install_path)?;
        }

        // Fix hardcoded library paths in binaries and shared libraries
        // This patches conda build paths to point to the actual install directory
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        platform::fix_library_paths(ctx, &install_path)?;

        // Fix hardcoded conda build prefixes in text files (shell scripts, etc.)
        // Conda packages use a placeholder prefix that must be replaced at install time
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        conda_common::fix_text_prefixes(&install_path);

        // Store lockfile info
        let platform_info = tv.lock_platforms.entry(platform_key.clone()).or_default();
        platform_info.url = Some(main_pkg.download_url.clone());
        if let Some(sha256) = &main_pkg.sha256 {
            platform_info.checksum = Some(format!("sha256:{}", sha256));
        }
        platform_info.conda_deps = Some(dep_basenames.clone());

        // Store resolved packages in tv.conda_packages for lockfile update
        for (basename, pkg) in &resolved {
            tv.conda_packages.insert(
                (platform_key.clone(), basename.clone()),
                CondaPackageInfo {
                    url: pkg.download_url.clone(),
                    checksum: pkg.sha256.as_ref().map(|s| format!("sha256:{}", s)),
                },
            );
        }

        // Make binaries executable (use same path logic as list_bin_paths)
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

        Ok(tv)
    }

    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let files = self.fetch_package_files().await?;
        let subdir = Self::conda_subdir_for_platform(target);

        // Find the package file for this version and platform (prefers platform-specific over noarch)
        let pkg_file = Self::find_package_file(&files, Some(&tv.version), subdir);

        match pkg_file {
            Some(pkg_file) => {
                let download_url = Self::build_download_url(&pkg_file.download_url);

                // Resolve dependencies for this platform
                let mut resolved = HashMap::new();
                let mut visited = HashSet::new();
                visited.insert(self.tool_name());
                self.resolve_dependencies(pkg_file, subdir, &mut resolved, &mut visited)
                    .await?;

                // Get dependency basenames
                let conda_deps: Vec<String> = resolved
                    .values()
                    .map(|p| strip_conda_extension(&p.basename).to_string())
                    .collect();

                Ok(PlatformInfo {
                    url: Some(download_url),
                    checksum: pkg_file.sha256.as_ref().map(|s| format!("sha256:{}", s)),
                    size: None,
                    url_api: None,
                    conda_deps: if conda_deps.is_empty() {
                        None
                    } else {
                        Some(conda_deps)
                    },
                })
            }
            None => {
                // No package available for this platform
                Ok(PlatformInfo {
                    url: None,
                    checksum: None,
                    size: None,
                    url_api: None,
                    conda_deps: None,
                })
            }
        }
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let install_path = tv.install_path();
        if cfg!(windows) {
            // Windows conda packages put binaries in Library/bin
            Ok(vec![install_path.join("Library").join("bin")])
        } else {
            // Unix conda packages put binaries in bin
            Ok(vec![install_path.join("bin")])
        }
    }
}

/// Represents a conda package file from the anaconda.org API
#[derive(Debug, Deserialize)]
struct CondaPackageFile {
    version: String,
    basename: String,
    download_url: String,
    sha256: Option<String>,
    upload_time: Option<String>,
    #[serde(default)]
    attrs: CondaPackageAttrs,
}

impl CondaPackageFile {
    /// Convert to a ResolvedPackage with the given name
    fn to_resolved_package(&self, name: &str) -> ResolvedPackage {
        ResolvedPackage {
            name: name.to_string(),
            download_url: CondaBackend::build_download_url(&self.download_url),
            // Filter out empty strings - API sometimes returns "" instead of null
            sha256: self.sha256.as_ref().filter(|s| !s.is_empty()).cloned(),
            basename: self.basename.clone(),
        }
    }
}

/// Package attributes including platform info
#[derive(Debug, Default, Deserialize)]
struct CondaPackageAttrs {
    #[serde(default)]
    subdir: String,
    #[serde(default)]
    depends: Vec<String>,
}

/// Resolved package ready for download
#[derive(Debug, Clone)]
struct ResolvedPackage {
    name: String,
    download_url: String,
    sha256: Option<String>,
    basename: String,
}

/// Packages to skip during dependency resolution:
/// - Virtual packages (__osx, __glibc, etc.) represent system requirements
/// - Build-only constraints (python_abi) don't provide runtime files
/// - System-provided libraries (gcc, vc runtime) should be installed separately
///
/// Note: python/perl/ruby are NOT skipped because some tools (e.g. vim) dynamically
/// link against libpython/libperl and need the shared libraries at runtime.
const SKIP_PACKAGES: &[&str] = &[
    "python_abi",
    // Linux system libraries (provided by distro)
    "libgcc-ng",
    "libstdcxx-ng",
    // Windows Visual C++ runtime (requires Visual Studio or VC++ redistributable)
    "ucrt",
    "vc",
    "vc14_runtime",
    "vs2015_runtime",
];

/// Parse a conda dependency specification
/// Returns (package_name, optional_version_spec) or None if should be skipped
fn parse_dependency(dep: &str) -> Option<(String, Option<String>)> {
    // Skip virtual packages (start with __)
    if dep.starts_with("__") {
        return None;
    }

    // Parse "package_name [version_spec] [build_spec]"
    let parts: Vec<&str> = dep.split_whitespace().collect();
    let name = parts.first()?.to_string();

    // Skip runtime dependencies that are typically not needed for standalone tools
    if SKIP_PACKAGES.contains(&name.as_str()) {
        return None;
    }

    // Get version spec if present (ignore build spec)
    let version = parts.get(1).map(|s| s.to_string());
    Some((name, version))
}

/// Extract dependency version pins from a conda package's build string.
///
/// Conda build strings encode key dependency versions at the start:
/// - `py310` → Python 3.10 (first digit = major, rest = minor)
/// - `pl5321` → Perl 5.32 (first digit = major, next 2 digits = minor)
///
/// Returns a map of (dep_name → pinned_version_prefix), e.g. {"python": "3.10"}
fn extract_build_pins(basename: &str) -> HashMap<String, String> {
    let name = strip_conda_extension(basename);
    // Build string is the last segment: "pkg-version-buildstring"
    let build_string = name.rsplit('-').next().unwrap_or("");

    let mut pins = HashMap::new();
    let mut remaining = build_string;

    loop {
        if let Some(rest) = remaining.strip_prefix("py") {
            // Python: first digit = major, rest = minor → py310 = 3.10
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 2 {
                pins.insert(
                    "python".to_string(),
                    format!("{}.{}", &digits[..1], &digits[1..]),
                );
                remaining = &rest[digits.len()..];
                continue;
            }
        } else if let Some(rest) = remaining.strip_prefix("pl") {
            // Perl: first digit = major, next 2 = minor → pl5321 = 5.32
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 3 {
                pins.insert(
                    "perl".to_string(),
                    format!("{}.{}", &digits[..1], &digits[1..3]),
                );
                remaining = &rest[digits.len()..];
                continue;
            }
        }
        break;
    }

    pins
}

/// Strip conda extension from basename
/// "ncurses-6.4-h7ea286d_0.conda" -> "ncurses-6.4-h7ea286d_0"
fn strip_conda_extension(basename: &str) -> &str {
    basename
        .strip_suffix(".conda")
        .or_else(|| basename.strip_suffix(".tar.bz2"))
        .unwrap_or(basename)
}

/// Extract basename from URL
/// "https://conda.anaconda.org/.../ncurses-6.4-h7ea286d_0.conda" -> "ncurses-6.4-h7ea286d_0.conda"
fn extract_basename_from_url(url: &str) -> String {
    url.rsplit('/').next().unwrap_or(url).to_string()
}
