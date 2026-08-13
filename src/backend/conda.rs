use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{
    MISE_BINS_DIR, VersionInfo, filter_cached_prereleases, mark_prerelease,
    runtime_path_for_install_path,
};
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::Settings;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::{self, Lockfile, PlatformInfo};
use crate::toolset::{ToolVersion, ToolVersionOptions};
use crate::{backend::Backend, dirs, parallel};
use crate::{file, hash};
use async_trait::async_trait;
use eyre::{Result, WrapErr};
use itertools::Itertools;
use rattler::install::{InstallDriver, InstallOptions, PythonInfo, link_package};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, ParseStrictness,
    Platform as CondaPlatform, RepoDataRecord, prefix::Prefix, prefix_record::PathsEntry,
};
use rattler_repodata_gateway::{Gateway, RepoData};
use rattler_solve::{
    ChannelPriority, SolveStrategy, SolverImpl, SolverTask, resolvo::Solver as ResolvoSolver,
};
use rattler_virtual_packages::{VirtualPackageOverrides, VirtualPackages};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use versions::Versioning;

/// Conda package info stored in the shared conda-packages section of lockfiles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CondaPackageInfo {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[derive(Debug)]
pub struct CondaBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct CondaOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> CondaOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn channel_name(&self) -> String {
        self.values
            .str("channel")
            .map(str::to_string)
            .unwrap_or_else(|| Settings::get().conda.channel.clone())
    }

    fn channel(&self) -> Result<Channel> {
        let name = self.channel_name();
        let root_dir = std::env::current_dir().unwrap_or_else(|_| dirs::HOME.to_path_buf());
        let config = ChannelConfig::default_with_root_dir(root_dir);
        Channel::from_str(&name, &config)
            .map_err(|e| eyre::eyre!("invalid conda channel '{}': {}", name, e))
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("channel".to_string(), self.channel_name())])
    }
}

impl CondaBackend {
    fn next_temp_id() -> u64 {
        static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    fn temp_download_path(dest: &std::path::Path) -> PathBuf {
        dest.with_extension(format!(
            "tmp.{}.{}",
            std::process::id(),
            Self::next_temp_id()
        ))
    }

    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
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

    fn detect_virtual_packages(platform: CondaPlatform) -> Result<Vec<GenericVirtualPackage>> {
        VirtualPackages::detect_for_platform(
            platform,
            &VirtualPackageOverrides::from_env(),
            Some(&dirs::CACHE.join("conda")),
        )
        .map(|vp| vp.into_generic_virtual_packages().collect())
        .map_err(|e| eyre::eyre!("failed to detect conda virtual packages: {e}"))
    }

    /// Deduplicate records that share the same archive identifier
    /// (name-version-build plus archive type) — the same key rattler-solve uses
    /// to detect duplicates, so URL-only dedup isn't sufficient when conda-forge
    /// serves the same archive under multiple URLs (e.g. distinct CDN paths).
    fn dedup_records_by_identifier<'a, I>(records: I) -> Vec<RepoDataRecord>
    where
        I: IntoIterator<Item = &'a RepoDataRecord>,
    {
        let mut seen = HashSet::new();
        records
            .into_iter()
            .filter(|r| seen.insert(&r.identifier))
            .cloned()
            .collect()
    }

    /// Flatten gateway RepoData into owned records for the solver, deduplicating
    /// by archive identifier. See [`Self::dedup_records_by_identifier`].
    fn flatten_repodata(repodata: &[RepoData]) -> Vec<RepoDataRecord> {
        Self::dedup_records_by_identifier(repodata.iter().flat_map(|rd| rd.iter()))
    }

    /// Fetch repodata and solve the conda environment for the given specs and platform.
    async fn solve_packages(
        &self,
        specs: Vec<MatchSpec>,
        platform: CondaPlatform,
        opts: CondaOptions<'_>,
    ) -> Result<Vec<RepoDataRecord>> {
        let channel = opts.channel()?;
        let gateway = Self::create_gateway();

        let repodata: Vec<RepoData> = gateway
            .query([channel], [platform, CondaPlatform::NoArch], specs.clone())
            .recursive(true)
            .await
            .map_err(|e| eyre::eyre!("failed to fetch repodata: {}", e))?
            .to_vec();

        let flat_records = Self::flatten_repodata(&repodata);
        let virtual_packages = Self::detect_virtual_packages(platform)?;

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
            strategy: SolveStrategy::Highest,
            dependency_overrides: vec![],
            excluded_candidates: Default::default(),
            cancellation_token: None,
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
            .and_then(|mut s| s.next_back())
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

    /// Verify a file's sha256 against an expected "sha256:<hex>" checksum.
    /// Returns Ok(true) if matches, Ok(false) if mismatches, or Ok(true)
    /// if no expected checksum is provided (skip verification).
    fn verify_checksum(path: &std::path::Path, expected: Option<&str>) -> Result<bool> {
        let Some(expected) = expected else {
            return Ok(true);
        };
        let Some(expected_hex) = expected.strip_prefix("sha256:") else {
            return Ok(true);
        };
        let actual_hex = hash::file_hash_sha256(path, None)?;
        Ok(actual_hex == expected_hex)
    }

    /// Download a file to dest with optional checksum verification.
    /// Uses atomic writes: downloads to a temp file, verifies, then renames.
    /// If dest already exists and checksum matches, skips download.
    async fn download_to(url: &str, dest: &std::path::Path, checksum: Option<&str>) -> Result<()> {
        if dest.exists() && Self::verify_checksum(dest, checksum)? {
            return Ok(());
        }

        file::create_dir_all(Self::conda_data_dir())?;
        let temp = Self::temp_download_path(dest);
        HTTP.download_file(url, &temp, None).await?;

        if !Self::verify_checksum(&temp, checksum)? {
            let _ = file::remove_all(&temp);
            let display_checksum = checksum.unwrap_or("unknown");
            return Err(eyre::eyre!(
                "checksum mismatch for {}: expected {}",
                url,
                display_checksum,
            ));
        }

        if let Err(err) = file::rename(&temp, dest) {
            let _ = file::remove_all(&temp);

            // Another concurrent installer may have won the race and written `dest`.
            // If `dest` now exists and verifies, treat this as success.
            if dest.exists() && Self::verify_checksum(dest, checksum)? {
                return Ok(());
            }

            return Err(err).wrap_err_with(|| {
                format!(
                    "failed to finalize conda archive download for {}",
                    dest.display()
                )
            });
        }
        Ok(())
    }

    /// Download a single package archive to the shared conda data dir.
    async fn download_record(record: RepoDataRecord) -> Result<PathBuf> {
        let url_str = record.url.to_string();
        let filename = Self::url_filename(&record.url);
        let dest = Self::conda_data_dir().join(&filename);
        let checksum = Self::format_sha256(&record);

        Self::download_to(&url_str, &dest, checksum.as_deref()).await?;
        Ok(dest)
    }

    /// Download a package by URL with optional checksum (for locked installs).
    async fn download_url_with_checksum(
        (url_str, checksum): (String, Option<String>),
    ) -> Result<PathBuf> {
        let filename = url_str.rsplit('/').next().unwrap_or("package").to_string();
        let dest = Self::conda_data_dir().join(&filename);

        Self::download_to(&url_str, &dest, checksum.as_deref()).await?;
        Ok(dest)
    }

    /// Extract a downloaded conda package archive into dest using rattler.
    async fn extract_package(archive: &std::path::Path, dest: &std::path::Path) -> Result<()> {
        rattler_package_streaming::tokio::fs::extract(archive, dest)
            .await
            .map_err(|e| eyre::eyre!("failed to extract {}: {}", archive.display(), e))?;
        Ok(())
    }

    /// Extract a package to a temp dir and link it into the prefix using rattler.
    ///
    /// This handles text and binary prefix replacement (replacing conda build
    /// placeholders with the actual install path), file permissions, and macOS
    /// code signing — all via rattler's link_package.
    async fn install_package(
        archive: &std::path::Path,
        prefix: &Prefix,
        driver: &InstallDriver,
        python_info: Option<PythonInfo>,
    ) -> Result<Vec<PathsEntry>> {
        let temp_dir = tempfile::tempdir()?;
        Self::extract_package(archive, temp_dir.path()).await?;
        let install_options = InstallOptions {
            python_info,
            ..InstallOptions::default()
        };
        let paths = link_package(temp_dir.path(), prefix, driver, install_options)
            .await
            .map_err(|e| eyre::eyre!("failed to link {}: {}", archive.display(), e))?;
        Ok(paths)
    }

    /// Extract PythonInfo from the solved records if a python package is present.
    /// This is needed to correctly install noarch python packages.
    fn python_info_from_records(
        records: &[RepoDataRecord],
        platform: CondaPlatform,
    ) -> Option<PythonInfo> {
        records
            .iter()
            .find(|r| r.package_record.name.as_normalized() == "python")
            .and_then(|r| {
                PythonInfo::from_version(
                    r.package_record.version.version(),
                    r.package_record.python_site_packages_path.as_deref(),
                    platform,
                )
                .ok()
            })
    }

    /// Extract PythonInfo from conda package basenames (for locked installs).
    /// Parses basenames using `ArchiveIdentifier` (`<name>-<version>-<build>` format).
    fn python_info_from_basenames(
        basenames: &[String],
        platform: CondaPlatform,
    ) -> Option<PythonInfo> {
        use rattler_conda_types::Version;
        use rattler_conda_types::package::ArchiveIdentifier;
        use std::str::FromStr;
        basenames.iter().find_map(|b| {
            let id = ArchiveIdentifier::from_str(b).ok()?;
            if id.name != "python" {
                return None;
            }
            let version = Version::from_str(&id.version).ok()?;
            PythonInfo::from_version(&version, None, platform).ok()
        })
    }

    fn read_lockfile_for_tool(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<Lockfile> {
        lockfile::read_lockfile_for_tool_source(&ctx.config, tv.request.source())
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
        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        let records = self
            .solve_packages(vec![match_spec], CondaPlatform::current(), opts)
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

        // Extract python info from solved records for noarch python packages
        let python_info = Self::python_info_from_records(&all_records, CondaPlatform::current());

        // Download all in parallel
        ctx.pr
            .set_message(format!("downloading {} packages", all_records.len()));
        let downloaded = parallel::parallel(all_records.clone(), Self::download_record).await?;

        // Create conda prefix and install driver
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;
        let prefix = Prefix::create(&install_path)
            .map_err(|e| eyre::eyre!("failed to create conda prefix: {}", e))?;
        let driver = InstallDriver::default();

        let mut main_paths = Vec::new();
        for (record, archive) in all_records.iter().zip(downloaded.iter()) {
            let name = record.package_record.name.as_normalized();
            let is_main = name == tool_name_norm;
            ctx.pr.set_message(format!("installing {name}"));
            let paths =
                Self::install_package(archive, &prefix, &driver, python_info.clone()).await?;
            if is_main {
                main_paths = paths;
            }
        }

        Self::make_bins_executable(&install_path)?;
        self.create_bin_launcher_dir(tv, &main_paths)?;

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
        platform_info.conda_deps = Some(dep_basenames.clone());

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
        tv: &mut ToolVersion,
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
        let main_checksum = platform_info.checksum.clone();

        let dep_basenames = platform_info.conda_deps.clone().unwrap_or_default();
        let lockfile = self.read_lockfile_for_tool(ctx, tv)?;

        // Extract python info from basenames for noarch python packages
        let python_info =
            Self::python_info_from_basenames(&dep_basenames, CondaPlatform::current());

        // Collect dep (url, checksum) pairs from lockfile (deps first, main last)
        let mut downloads: Vec<(String, Option<String>)> = vec![];
        for basename in &dep_basenames {
            if let Some(pkg_info) = lockfile.get_conda_package(platform_key, basename) {
                downloads.push((pkg_info.url.clone(), pkg_info.checksum.clone()));
            } else {
                return Err(eyre::eyre!(
                    "conda package {} not found in lockfile for {}",
                    basename,
                    platform_key
                ));
            }
        }
        downloads.push((main_url, main_checksum));

        ctx.pr
            .set_message(format!("downloading {} packages", downloads.len()));
        let downloaded = parallel::parallel(downloads, Self::download_url_with_checksum).await?;

        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        file::create_dir_all(&install_path)?;
        let prefix = Prefix::create(&install_path)
            .map_err(|e| eyre::eyre!("failed to create conda prefix: {}", e))?;
        let driver = InstallDriver::default();

        let mut main_paths = Vec::new();
        for archive in &downloaded {
            let filename = archive.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            ctx.pr.set_message(format!("installing {filename}"));
            // main package is always last, so main_paths ends up with its entries
            main_paths =
                Self::install_package(archive, &prefix, &driver, python_info.clone()).await?;
        }

        Self::make_bins_executable(&install_path)?;
        self.create_bin_launcher_dir(tv, &main_paths)?;

        // Repopulate tv.conda_packages from lockfile so downstream lockfile update preserves entries
        for basename in &dep_basenames {
            if let Some(pkg_info) = lockfile.get_conda_package(platform_key, basename) {
                tv.conda_packages.insert(
                    (platform_key.to_string(), basename.clone()),
                    pkg_info.clone(),
                );
            }
        }

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

    /// Creates a `.mise-bins` directory with launchers only for binaries from the main package.
    /// Uses the PathsEntry list returned by rattler's link_package to identify which files
    /// belong to the main package (excluding transitive dependency binaries). The launchers
    /// activate the package prefix only for the command they start, so dependencies are
    /// available to that command without exposing their binaries on the user's PATH.
    fn create_bin_launcher_dir(&self, tv: &ToolVersion, main_paths: &[PathsEntry]) -> Result<()> {
        let symlink_dir = tv.install_path().join(MISE_BINS_DIR);
        file::create_dir_all(&symlink_dir)?;

        let install_path = tv.install_path();
        let bin_dirs: &[&std::path::Path] = if cfg!(windows) {
            &[
                std::path::Path::new("Library/bin"),
                std::path::Path::new("Scripts"),
                std::path::Path::new("bin"),
            ]
        } else {
            &[std::path::Path::new("bin")]
        };

        for entry in main_paths {
            if !bin_dirs
                .iter()
                .any(|dir| entry.relative_path.starts_with(dir))
            {
                continue;
            }
            let Some(bin_name) = entry.relative_path.file_name() else {
                continue;
            };
            let src = install_path.join(&entry.relative_path);
            let dst = Self::bin_launcher_path(&symlink_dir, bin_name, cfg!(windows));
            if src.exists() && !dst.exists() {
                Self::create_bin_launcher(&install_path, &src, &dst)?;
            }
        }

        // On Windows the entries above are copies, and a copied executable resolves its
        // imports from its own directory before anything on PATH. The DLLs it needs sit
        // next to the original and mostly belong to dependency packages, so the
        // main-package filter leaves them behind and the copy dies with
        // STATUS_DLL_NOT_FOUND (`conda:postgresql` needs 91 of them). Bring every DLL
        // along: they are libraries, not commands, so this does not put dependency
        // executables on PATH, which is what this directory exists to prevent.
        //
        // unix does not need it — the entries there are symlinks, and `$ORIGIN` in an
        // ELF RPATH resolves against the *target's* directory, so `../lib` still lands
        // inside the install.
        if cfg!(windows) {
            for dir in bin_dirs {
                let bin_dir = install_path.join(dir);
                // Only some of these exist in any given package. Anything other than a
                // missing directory has to surface: swallowing it would drop a DLL and
                // still report the install as a success, which lands the user right back
                // on the STATUS_DLL_NOT_FOUND this is meant to prevent.
                let entries = match std::fs::read_dir(&bin_dir) {
                    Ok(entries) => entries,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        return Err(err).wrap_err_with(|| {
                            format!("failed to read {}", file::display_path(&bin_dir))
                        });
                    }
                };
                for entry in entries {
                    let entry = entry.wrap_err_with(|| {
                        format!(
                            "failed to read an entry in {}",
                            file::display_path(&bin_dir)
                        )
                    })?;
                    let src = entry.path();
                    if !src
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
                    {
                        continue;
                    }
                    // Asked of the `DirEntry` rather than the path: `Path::is_file`
                    // reports a metadata failure as "not a file", which would skip a DLL
                    // that antivirus or a permission problem merely made unreadable for a
                    // moment. Anything not a directory is fair game, including a symlink.
                    let file_type = entry.file_type().wrap_err_with(|| {
                        format!(
                            "failed to stat {} in {}",
                            entry.file_name().to_string_lossy(),
                            file::display_path(&bin_dir)
                        )
                    })?;
                    if file_type.is_dir() {
                        continue;
                    }
                    let Some(name) = src.file_name() else {
                        continue;
                    };
                    let dst = symlink_dir.join(name);
                    if dst.exists() {
                        // First writer wins, and the order is deliberate: the main
                        // package's own DLLs are placed above, then `Library/bin` before
                        // `Scripts` and `bin`. A name reaching here twice means two bin
                        // directories disagree, which is worth seeing under --verbose.
                        trace!(
                            "conda: {} already present in {}, keeping the earlier one",
                            name.to_string_lossy(),
                            file::display_path(&symlink_dir)
                        );
                        continue;
                    }
                    file::hard_link_or_copy(&src, &dst)?;
                }
            }
        }
        Ok(())
    }

    fn bin_launcher_path(
        launcher_dir: &Path,
        bin_name: &std::ffi::OsStr,
        windows: bool,
    ) -> PathBuf {
        let launcher = launcher_dir.join(bin_name);
        if windows
            && launcher
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            launcher.with_extension("cmd")
        } else {
            launcher
        }
    }

    fn create_bin_launcher(prefix: &Path, target: &Path, launcher: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            file::write(launcher, Self::render_unix_launcher(prefix, target))?;
            file::make_executable(launcher)?;
        }

        #[cfg(windows)]
        {
            let needs_activation_launcher = target
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    ext.eq_ignore_ascii_case("exe")
                        || ext.eq_ignore_ascii_case("cmd")
                        || ext.eq_ignore_ascii_case("bat")
                });
            if needs_activation_launcher {
                file::write(launcher, Self::render_windows_launcher(prefix, target))?;
            } else {
                // Preserve the existing fallback for uncommon executable formats. Native
                // .exe files remain at their original path and are invoked by a .cmd launcher
                // so their adjacent dependency DLLs and prefix activation are both available.
                file::make_symlink_or_copy(target, launcher)?;
            }
        }

        Ok(())
    }

    #[cfg(any(unix, test))]
    fn render_unix_launcher(prefix: &Path, target: &Path) -> String {
        let prefix = shell_quote(&prefix.to_string_lossy());
        let target = shell_quote(&target.to_string_lossy());
        format!(
            "#!/bin/sh\n\
             export CONDA_PREFIX={prefix}\n\
             export CONDA_DEFAULT_ENV={prefix}\n\
             export CONDA_SHLVL=1\n\
             export PATH=\"$CONDA_PREFIX/bin${{PATH:+:$PATH}}\"\n\
             for _mise_conda_script in \"$CONDA_PREFIX\"/etc/conda/activate.d/*.sh; do\n\
             \tif [ -f \"$_mise_conda_script\" ]; then\n\
             \t\t. \"$_mise_conda_script\" || exit $?\n\
             \tfi\n\
             done\n\
             unset _mise_conda_script\n\
             exec {target} \"$@\"\n"
        )
    }

    #[cfg(any(windows, test))]
    fn render_windows_launcher(prefix: &Path, target: &Path) -> String {
        let prefix = cmd_escape_value(&prefix.to_string_lossy());
        let target = cmd_escape_value(&target.to_string_lossy());
        format!(
            "@echo off\r\n\
             setlocal\r\n\
             set \"CONDA_PREFIX={prefix}\"\r\n\
             set \"CONDA_DEFAULT_ENV={prefix}\"\r\n\
             set \"CONDA_SHLVL=1\"\r\n\
             set \"PATH={prefix};{prefix}\\Library\\mingw-w64\\bin;{prefix}\\Library\\usr\\bin;{prefix}\\Library\\bin;{prefix}\\Scripts;{prefix}\\bin;%PATH%\"\r\n\
             for %%F in (\"{prefix}\\etc\\conda\\activate.d\\*.bat\") do if exist \"%%~fF\" call \"%%~fF\"\r\n\
             for %%F in (\"{prefix}\\etc\\conda\\activate.d\\*.cmd\") do if exist \"%%~fF\" call \"%%~fF\"\r\n\
             call \"{target}\" %*\r\n\
             exit /b %ERRORLEVEL%\r\n"
        )
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

        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        let records = self
            .solve_packages(vec![match_spec], platform, opts)
            .await?;

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

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["channel"]
    }

    fn resolve_lockfile_options(
        &self,
        request: &crate::toolset::ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(CondaOptions::new(&raw_opts).lockfile_options())
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = CondaOptions::new(&raw_opts);
        let channel = opts.channel()?;
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
            .map_err(|e| eyre::eyre!("failed to list versions for '{}': {}", tool_name, e))?
            .to_vec();

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
    /// channel option affects which versions are available. The override is
    /// on `_with_refresh` so it applies to both cached and refresh-enabled
    /// resolution paths; conda always queries the channel directly so the
    /// `_refresh` flag is irrelevant.
    async fn list_remote_versions_with_info_with_refresh(
        &self,
        config: &Arc<Config>,
        _refresh: bool,
    ) -> Result<Vec<VersionInfo>> {
        let opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let want_prereleases = self.include_prereleases(&opts);
        let versions = self
            ._list_remote_versions(config)
            .await?
            .into_iter()
            .map(mark_prerelease)
            .collect();
        Ok(filter_cached_prereleases(versions, want_prereleases))
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let platform_key = self.get_platform_key();
        let has_locked = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|p| p.url.as_ref())
            .is_some();

        if has_locked {
            self.install_from_locked(ctx, &mut tv, &platform_key)
                .await?;
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

        let match_spec = MatchSpec::from_str(&spec_str, ParseStrictness::Lenient)
            .map_err(|e| eyre::eyre!("invalid conda spec '{spec_str}': {e}"))?;

        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        let records = self
            .solve_packages(vec![match_spec], platform, opts)
            .await
            .wrap_err_with(|| format!("failed to solve {tool_name} for {}", target.to_key()))?;

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

        let main = main_record.ok_or_else(|| {
            eyre::eyre!(
                "conda solve for {tool_name} on {} did not return the requested package",
                target.to_key()
            )
        })?;
        Ok(PlatformInfo {
            url: Some(main.url.to_string()),
            checksum: Self::format_sha256(&main),
            size: None,
            url_api: None,
            conda_deps: Some(dep_basenames),
            ..Default::default()
        })
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let install_path = tv.install_path();
        let mise_bins = install_path.join(MISE_BINS_DIR);
        if mise_bins.exists() {
            return Ok(vec![runtime_path_for_install_path(tv, mise_bins)]);
        }

        // Fallback for tools installed before this change
        let bin_paths = if cfg!(windows) {
            // Conda packages on Windows can put binaries in either location
            // depending on the build variant (MSVC vs MSYS2/MinGW)
            vec![
                install_path.join("Library").join("bin"),
                install_path.join("bin"),
            ]
        } else {
            vec![install_path.join("bin")]
        };

        Ok(bin_paths
            .into_iter()
            .map(|path| runtime_path_for_install_path(tv, path))
            .collect())
    }
}

#[cfg(any(unix, test))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(any(windows, test))]
fn cmd_escape_value(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('"', "^\"")
}

#[cfg(test)]
mod tests {
    use super::{CondaBackend, CondaOptions};
    #[cfg(unix)]
    use crate::file;
    use crate::toolset::ToolVersionOptions;
    use rattler_conda_types::package::{ArchiveIdentifier, CondaArchiveType, DistArchiveType};
    use rattler_conda_types::{
        PackageName, PackageRecord, RepoDataRecord, Version, package::DistArchiveIdentifier,
    };
    use std::collections::BTreeMap;
    use std::path::Path;
    #[cfg(unix)]
    use std::process::Command;
    use std::str::FromStr;
    use url::Url;

    fn make_record(name: &str, version: &str, build: &str, url: &str) -> RepoDataRecord {
        let archive_id = ArchiveIdentifier {
            name: name.to_string(),
            version: version.to_string(),
            build_string: build.to_string(),
        };
        let conda_type = if url.ends_with(".conda") {
            CondaArchiveType::Conda
        } else {
            CondaArchiveType::TarBz2
        };
        RepoDataRecord {
            package_record: PackageRecord::new(
                PackageName::from_str(name).unwrap(),
                Version::from_str(version).unwrap(),
                build.to_string(),
            ),
            identifier: DistArchiveIdentifier::new(archive_id, DistArchiveType::Conda(conda_type)),
            url: Url::parse(url).unwrap(),
            channel: None,
        }
    }

    #[test]
    fn conda_options_reads_channel() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "channel".to_string(),
            toml::Value::String("conda-forge".to_string()),
        );

        assert_eq!(CondaOptions::new(&opts).channel_name(), "conda-forge");
    }

    #[test]
    fn conda_lockfile_options_include_channel() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "channel".to_string(),
            toml::Value::String("bioconda".to_string()),
        );

        assert_eq!(
            CondaOptions::new(&opts).lockfile_options(),
            BTreeMap::from([("channel".to_string(), "bioconda".to_string())])
        );
    }

    #[test]
    fn temp_download_path_is_unique_per_call() {
        let tmpdir = tempfile::tempdir().unwrap();
        let dest = tmpdir.path().join("libgcc-15.2.0-he0feb66_18.conda");

        let first = CondaBackend::temp_download_path(&dest);
        let second = CondaBackend::temp_download_path(&dest);

        assert_ne!(first, second);
        assert_eq!(first.parent(), dest.parent());
        assert_eq!(second.parent(), dest.parent());
    }

    #[test]
    fn unix_launcher_quotes_prefix_and_target() {
        let launcher = CondaBackend::render_unix_launcher(
            Path::new("/tmp/prefix with ' quote"),
            Path::new("/tmp/prefix with ' quote/bin/tool"),
        );

        assert!(launcher.contains("export CONDA_PREFIX='/tmp/prefix with '\\'' quote'"));
        assert!(launcher.contains("exec '/tmp/prefix with '\\'' quote/bin/tool' \"$@\""));
    }

    #[test]
    fn windows_launcher_escapes_cmd_metacharacters() {
        let launcher = CondaBackend::render_windows_launcher(
            Path::new(r"C:\prefix&tools%name%"),
            Path::new(r"C:\prefix&tools%name%\Scripts\tool.cmd"),
        );

        assert!(launcher.contains(r#"set "CONDA_PREFIX=C:\prefix^&tools%%name%%""#));
        assert!(launcher.contains(r#"call "C:\prefix^&tools%%name%%\Scripts\tool.cmd" %*"#));
    }

    #[test]
    fn windows_native_executable_uses_cmd_launcher_path() {
        let launcher = CondaBackend::bin_launcher_path(
            Path::new("/prefix/.mise-bins"),
            std::ffi::OsStr::new("tool.exe"),
            true,
        );

        assert_eq!(launcher, Path::new("/prefix/.mise-bins/tool.cmd"));
        assert_eq!(
            CondaBackend::bin_launcher_path(
                Path::new("/prefix/.mise-bins"),
                std::ffi::OsStr::new("tool.cmd"),
                true,
            ),
            Path::new("/prefix/.mise-bins/tool.cmd")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_launcher_activates_only_its_own_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix with ' quote");
        let bin_dir = prefix.join("bin");
        let activate_dir = prefix.join("etc/conda/activate.d");
        let target = bin_dir.join("tool");
        let dependency = bin_dir.join("dependency-command");
        let launcher = temp.path().join("launcher");
        file::create_dir_all(&bin_dir).unwrap();
        file::create_dir_all(&activate_dir).unwrap();
        file::write(
            &target,
            "#!/bin/sh\nprintf '%s\\n' \"$CONDA_PREFIX\" \"$CONDA_DEFAULT_ENV\" \"$CONDA_SHLVL\" \"$CONDA_TEST_ACTIVATED\" \"$(dependency-command)\" \"$1\"\nexit 23\n",
        )
        .unwrap();
        file::make_executable(&target).unwrap();
        file::write(&dependency, "#!/bin/sh\nprintf dependency-output\n").unwrap();
        file::make_executable(&dependency).unwrap();
        file::write(
            activate_dir.join("test.sh"),
            "export CONDA_TEST_ACTIVATED=activated\n",
        )
        .unwrap();
        CondaBackend::create_bin_launcher(&prefix, &target, &launcher).unwrap();

        let output = Command::new(&launcher)
            .arg("forwarded argument")
            .env("CONDA_PREFIX", "/wrong/prefix")
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(23));
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!(
                "{0}\n{0}\n1\nactivated\ndependency-output\nforwarded argument\n",
                prefix.display()
            )
        );
    }

    /// Regression test for https://github.com/jdx/mise/discussions/9829:
    /// conda-forge can serve the same archive under multiple URLs, so URL-based
    /// dedup leaves duplicates that the solver rejects. Dedup must use the
    /// archive identifier (name-version-build + archive type).
    #[test]
    fn dedup_records_collapses_same_identifier_different_urls() {
        let records = [
            make_record(
                "adwaita-icon-theme",
                "40.1.1",
                "ha770c72_1",
                "https://conda.anaconda.org/conda-forge/noarch/adwaita-icon-theme-40.1.1-ha770c72_1.tar.bz2",
            ),
            make_record(
                "adwaita-icon-theme",
                "40.1.1",
                "ha770c72_1",
                "https://mirror.example.com/conda-forge/noarch/adwaita-icon-theme-40.1.1-ha770c72_1.tar.bz2",
            ),
        ];

        let deduped = CondaBackend::dedup_records_by_identifier(records.iter());
        assert_eq!(deduped.len(), 1);
        assert_eq!(
            deduped[0].identifier.to_string(),
            "adwaita-icon-theme-40.1.1-ha770c72_1.tar.bz2"
        );
    }

    /// .conda and .tar.bz2 variants of the same name-version-build are distinct
    /// archives (the solver prefers .conda); dedup must preserve both so the
    /// solver's archive-type preference logic still applies.
    #[test]
    fn dedup_records_preserves_conda_and_tarbz2_variants() {
        let records = [
            make_record(
                "foo",
                "1.0",
                "h0_0",
                "https://example.com/foo-1.0-h0_0.tar.bz2",
            ),
            make_record(
                "foo",
                "1.0",
                "h0_0",
                "https://example.com/foo-1.0-h0_0.conda",
            ),
        ];

        let deduped = CondaBackend::dedup_records_by_identifier(records.iter());
        assert_eq!(deduped.len(), 2);
    }
}
