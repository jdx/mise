use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::{MISE_BINS_DIR, VersionInfo, runtime_path_for_install_path};
use crate::config::Config;
use crate::config::Settings;
use crate::http::{HTTP, apply_url_replacements};
use crate::install_context::InstallContext;
use crate::lockfile::{Lockfile, PlatformInfo};
use crate::toolset::{ToolVersion, ToolVersionOptions};
use crate::{backend::Backend, dirs, parallel};
use crate::{file, hash};
use async_trait::async_trait;
use eyre::{Result, WrapErr};
use http::Extensions;
use itertools::Itertools;
use rattler::install::{InstallDriver, InstallOptions, PythonInfo, link_package};
use rattler_conda_types::package::{
    IndexJson, LinkJson, NoArchLinks, PackageFile, PathType, PathsJson, PythonEntryPoints,
};
use rattler_conda_types::{
    Channel, ChannelConfig, GenericVirtualPackage, MatchSpec, PackageRecord, ParseStrictness,
    RepoDataRecord, Subdir as CondaPlatform, prefix::Prefix, prefix_record::PathsEntry,
};
use rattler_repodata_gateway::{Gateway, RepoData};
use rattler_solve::{
    ChannelPriority, SolveStrategy, SolverImpl, SolverTask, resolvo::Solver as ResolvoSolver,
};
use rattler_virtual_packages::{VirtualPackageOverrides, VirtualPackages};
use reqwest_middleware::{ClientBuilder as MiddlewareClientBuilder, Middleware, Next};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use versions::Versioning;

/// Where rattler links the files of a package whose path an earlier package already owns,
/// as `__clobbers__/<package name>/<path>` (its `CLOBBERS_DIR_NAME`, which is not exported).
const CLOBBERS_DIR: &str = "__clobbers__";

/// Conda package info stored in the shared conda-packages section of lockfiles
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CondaPackageInfo {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

#[derive(Debug)]
pub(crate) struct CondaBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct CondaOptions<'a> {
    values: BackendOptions<'a>,
}

#[derive(Debug)]
struct UrlReplacementMiddleware;

/// A `url_replacements` rewrite that [`UrlReplacementMiddleware`] refused to send.
///
/// `reqwest_middleware` wants a `std::error::Error`, which `eyre::Report` is not.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct UrlReplacementRefused(String);

/// Apply `url_replacements` to a repodata/package request, applying the same credential
/// rules [`crate::http::Client`] applies to a rewritten URL: refuse a transport downgrade
/// that would expose credentials, then drop credentials scoped to the original host.
///
/// Rattler drives its own client, so these requests never reach `crate::http::Client` and
/// would otherwise bypass both rules.
fn rewrite_request_url(
    request: &mut reqwest::Request,
    rewriter: impl FnOnce(&mut url::Url),
) -> Result<()> {
    let original = request.url().clone();
    rewriter(request.url_mut());
    // `headers_mut` and `url_mut` cannot be borrowed at once, so scope the URL through a copy.
    let mut url = request.url().clone();
    crate::http::clear_cross_host_credentials(request.headers_mut(), &original, &mut url);
    crate::http::ensure_secure_replacement_credentials(&original, &url, request.headers())?;
    *request.url_mut() = url;
    Ok(())
}

#[async_trait]
impl Middleware for UrlReplacementMiddleware {
    async fn handle(
        &self,
        mut request: reqwest::Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<reqwest::Response> {
        rewrite_request_url(&mut request, apply_url_replacements).map_err(|err| {
            reqwest_middleware::Error::middleware(UrlReplacementRefused(format!("{err:#}")))
        })?;
        next.run(request, extensions).await
    }
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

    pub(crate) fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn create_gateway() -> Result<Gateway> {
        let client = MiddlewareClientBuilder::new(HTTP.reqwest()?.clone())
            .with(UrlReplacementMiddleware)
            .build();
        Ok(Gateway::builder()
            .with_client(client)
            .with_cache_dir(dirs::CACHE.join("conda"))
            .finish())
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

    /// The conda platform of the running host
    fn current_conda_platform() -> Result<CondaPlatform> {
        CondaPlatform::current()
            .ok_or_else(|| eyre::eyre!("conda backend does not support this platform"))
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

    /// When the channel released `record`: when it first listed it, or else the
    /// build start the builder recorded. A record with neither, or with a
    /// non-positive one, has no release date.
    fn record_released_at(record: &PackageRecord) -> Option<jiff::Timestamp> {
        record
            .indexed_timestamp
            .or(record.timestamp)
            // rattler hands back a jiff 0.2 timestamp; both versions span the same range.
            .and_then(|t| jiff::Timestamp::from_nanosecond(t.jiff_timestamp().as_nanosecond()).ok())
            .filter(|t| t.as_second() > 0)
    }

    /// The records the solver may choose from under the `minimum_release_age`
    /// cutoff `before`: none released after it, whether a build of the tool or of
    /// a dependency. A record with no release date is kept, as it is when versions
    /// are listed.
    fn released_before(
        records: Vec<RepoDataRecord>,
        before: Option<jiff::Timestamp>,
    ) -> Vec<RepoDataRecord> {
        let Some(before) = before else {
            return records;
        };
        records
            .into_iter()
            .filter(|record| {
                Self::record_released_at(&record.package_record).is_none_or(|t| t <= before)
            })
            .collect()
    }

    /// Fetch repodata and solve the conda environment for the given specs and platform,
    /// choosing only from packages released by `before` (the `minimum_release_age` cutoff).
    async fn solve_packages(
        &self,
        specs: Vec<MatchSpec>,
        platform: CondaPlatform,
        opts: CondaOptions<'_>,
        before: Option<jiff::Timestamp>,
    ) -> Result<Vec<RepoDataRecord>> {
        let channel = opts.channel()?;
        let gateway = Self::create_gateway()?;

        let repodata: Vec<RepoData> = gateway
            .query([channel], [platform, CondaPlatform::NoArch], specs.clone())
            .recursive(true)
            .await
            .map_err(|e| eyre::eyre!("failed to fetch repodata: {}", e))?
            .to_vec();

        let flat_records = Self::released_before(Self::flatten_repodata(&repodata), before);
        let virtual_packages = Self::detect_virtual_packages(platform)?;

        let task = SolverTask {
            available_packages: [flat_records.as_slice()],
            specs,
            virtual_packages,
            locked_packages: vec![],
            pinned_packages: vec![],
            additional_packages: vec![],
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
        let result = solver.solve(task).map_err(|e| match before {
            Some(before) => eyre::eyre!(
                "conda solve failed with only packages released before {before} (minimum_release_age): {e}"
            ),
            None => eyre::eyre!("conda solve failed: {}", e),
        })?;

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

    /// The channel subdir a package URL points into: the URL without its file name.
    fn channel_subdir(url: &str) -> &str {
        url.rsplit_once('/').map_or(url, |(dir, _)| dir)
    }

    /// The channel a package URL points into: the URL without its subdir and file name.
    /// A tool solves against one channel, so its own package and all its dependencies,
    /// whether platform-specific or noarch, share this.
    fn channel_root(url: &str) -> &str {
        let subdir = Self::channel_subdir(url);
        subdir.rsplit_once('/').map_or(subdir, |(root, _)| root)
    }

    /// Hash of the channel subdir a package URL points into.
    ///
    /// A conda file name (`<name>-<version>-<build>`) identifies a package only within
    /// one channel subdir, while tools from different channels share the lockfile's
    /// `conda-packages` section and the archive store. Both are qualified with this, so
    /// that one channel's package cannot replace the package another channel has under
    /// the same name. A channel's owner chooses its URL, so the tag is 128 bits of a
    /// sha256: too long to search for a URL whose tag equals another channel's.
    pub(crate) fn channel_tag(url: &str) -> String {
        hash::hash_sha256_to_str(Self::channel_subdir(url))[..32].to_string()
    }

    /// Directory of the shared archive store that holds the archive downloaded from `url`.
    fn archive_dir(url: &str) -> PathBuf {
        Self::conda_data_dir().join(Self::channel_tag(url))
    }

    /// Key of a package in the lockfile's shared `conda-packages` section: the file name
    /// without its .conda or .tar.bz2 extension, then `@` and the [`Self::channel_tag`].
    fn record_basename(record: &RepoDataRecord) -> String {
        let filename = Self::url_filename(&record.url);
        let basename = filename
            .strip_suffix(".conda")
            .or_else(|| filename.strip_suffix(".tar.bz2"))
            .unwrap_or(&filename);
        format!("{basename}@{}", Self::channel_tag(record.url.as_str()))
    }

    /// Checks that the `[conda-packages]` entry `key` may be installed as a dependency of
    /// the tool package at `main_url`: it comes from the tool's channel, and a key that
    /// names a channel names the one its URL points into. A key without a channel was
    /// written before keys named one, and is accepted only from the tool's channel.
    pub(crate) fn check_locked_dependency(
        key: &str,
        package: &CondaPackageInfo,
        main_url: &str,
    ) -> Result<()> {
        let redact = crate::install_layout::resolver::redact_credentials;
        let main_channel = Self::channel_root(main_url);
        if Self::channel_root(&package.url) != main_channel {
            eyre::bail!(
                "refusing the conda package {key} in mise.lock: its url {} is not in the channel {} of the tool, which all its dependencies come from. Run `mise lock` to resolve the tool again",
                redact(&package.url),
                redact(main_channel),
            );
        }
        match key.rsplit_once('@') {
            Some((_, tag)) => {
                if tag != Self::channel_tag(&package.url) {
                    eyre::bail!(
                        "refusing the conda package {key} in mise.lock: its key names another channel than its url {}. Run `mise lock` to resolve the tool again",
                        redact(&package.url)
                    );
                }
            }
            None => {
                remove_by!("2027.11.0", "conda-package-keys-without-channel");
            }
        }
        Ok(())
    }

    /// The checksum the channel declares for `record`, as "<algorithm>:<hex>":
    /// its sha256, or its md5 when the channel lists no sha256. Downloads and
    /// lockfile entries are verified against it, so a package with either is
    /// never installed unverified.
    fn format_checksum(record: &RepoDataRecord) -> Option<String> {
        let package = &record.package_record;
        package
            .sha256
            .as_ref()
            .map(|h| format!("sha256:{}", hex::encode(h)))
            .or_else(|| {
                package
                    .md5
                    .as_ref()
                    .map(|h| format!("md5:{}", hex::encode(h)))
            })
    }

    /// Verify a file against an expected "<algorithm>:<hex>" checksum.
    /// Returns Ok(true) if it matches, Ok(false) if it does not, and Ok(true)
    /// if no checksum is expected. A checksum without an algorithm, or with one
    /// that cannot be computed, is an error, never a match.
    fn verify_checksum(path: &std::path::Path, expected: Option<&str>) -> Result<bool> {
        let Some(expected) = expected else {
            return Ok(true);
        };
        let Some((algo, expected_hex)) = expected.split_once(':') else {
            eyre::bail!(
                "invalid checksum {expected:?} for {}: expected <algorithm>:<hex>",
                path.display()
            );
        };
        match hash::ensure_checksum(path, expected_hex, None, algo) {
            Ok(()) => Ok(true),
            Err(err) if err.is::<hash::ChecksumMismatch>() => Ok(false),
            Err(err) => Err(err),
        }
    }

    /// Download a file to dest with optional checksum verification.
    /// Uses atomic writes: downloads to a temp file, verifies, then renames.
    /// If dest already exists and checksum matches, skips download.
    async fn download_to(url: &str, dest: &std::path::Path, checksum: Option<&str>) -> Result<()> {
        if dest.exists() && Self::verify_checksum(dest, checksum)? {
            return Ok(());
        }

        if let Some(dir) = dest.parent() {
            file::create_dir_all(dir)?;
        }
        let temp = Self::temp_download_path(dest);
        HTTP.download_file_pinned(url, &temp, checksum, None)
            .await?;

        let verified = Self::verify_checksum(&temp, checksum);
        if !matches!(verified, Ok(true)) {
            let _ = file::remove_all(&temp);
            verified?;
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
        let dest = Self::archive_dir(&url_str).join(&filename);
        let checksum = Self::format_checksum(&record);

        Self::download_to(&url_str, &dest, checksum.as_deref()).await?;
        Ok(dest)
    }

    /// Download a package by URL with optional checksum (for locked installs).
    async fn download_url_with_checksum(
        (url_str, checksum): (String, Option<String>),
    ) -> Result<PathBuf> {
        let filename = url_str.rsplit('/').next().unwrap_or("package").to_string();
        let dest = Self::archive_dir(&url_str).join(&filename);

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

    /// Reads the index and the paths of an extracted package, which say what rattler
    /// links and where.
    ///
    /// rattler joins the paths onto the prefix as they are, and tells by their spelling
    /// alone whether an earlier package owns a path or a directory above it. If one does,
    /// the file is not linked through what is there, such as a symlink of that package,
    /// but to `__clobbers__/<package name>/<path>`. That holds only while every path is
    /// spelled one way and nothing else is in that directory, so a package is refused if
    /// it names a path otherwise, ships `__clobbers__`, or has a name that leads out of
    /// there or under which an earlier package has files there.
    fn read_package_paths(
        archive: &Path,
        package_dir: &Path,
        prefix: &Prefix,
        python_info: Option<&PythonInfo>,
    ) -> Result<(IndexJson, PathsJson)> {
        let index_json = IndexJson::from_package_directory(package_dir)
            .wrap_err_with(|| format!("failed to read the index of {}", archive.display()))?;
        let paths_json = PathsJson::from_package_directory_with_deprecated_fallback(package_dir)
            .wrap_err_with(|| format!("failed to read the paths of {}", archive.display()))?;
        for entry in &paths_json.paths {
            // where rattler links the entry (its `compute_paths`)
            let path = match python_info {
                Some(python_info) if index_json.noarch.is_python() => {
                    python_info.get_python_noarch_target_path(&entry.relative_path)
                }
                _ => Cow::Borrowed(entry.relative_path.as_path()),
            };
            // rattler keeps clobbered files in `__clobbers__`, which a case-insensitive
            // file system also opens under any other case
            let in_clobbers = path.components().next().is_some_and(|first| {
                first
                    .as_os_str()
                    .to_str()
                    .is_some_and(|first| first.eq_ignore_ascii_case(CLOBBERS_DIR))
            });
            if !path.to_str().is_some_and(file::is_safe_relative_path) || in_clobbers {
                eyre::bail!(
                    "refusing to install conda package {}: it has the unsafe path {}, which is not a plain path inside the environment",
                    archive.display(),
                    path.display()
                );
            }
        }
        let name = index_json.name.as_normalized();
        let clobbers = prefix.path().join(CLOBBERS_DIR).join(name);
        if !file::is_plain_file_name(name) || clobbers.symlink_metadata().is_ok() {
            eyre::bail!(
                "refusing to install conda package {}: it cannot be linked under the name {name:?}, which is not a plain file name or is already used by another package in the environment",
                archive.display()
            );
        }
        Ok((index_json, paths_json))
    }

    /// Extract a package to a temp dir and link it into the prefix using rattler.
    ///
    /// This handles text and binary prefix replacement (replacing conda build
    /// placeholders with the actual install path), file permissions, and macOS
    /// code signing — all via rattler's link_package.
    ///
    /// rattler links a package's symlinks whatever their target and opens every
    /// destination by name, so a link that leaves the prefix redirects what is written
    /// after it. A link that stays inside its own package can still leave the prefix
    /// through the links of the packages before it. The symlinks of a package are
    /// therefore linked first and the whole prefix is checked before any of its files
    /// are written, and no package is linked after a check fails.
    async fn install_package(
        archive: &Path,
        prefix: &Prefix,
        driver: &InstallDriver,
        python_info: Option<PythonInfo>,
    ) -> Result<Vec<PathsEntry>> {
        let temp_dir = tempfile::tempdir()?;
        let package_dir = temp_dir.path();
        Self::extract_package(archive, package_dir).await?;
        file::ensure_symlinks_stay_within(package_dir).wrap_err_with(|| {
            format!(
                "refusing to install conda package {}: it has a symlink that leads outside the package",
                archive.display()
            )
        })?;
        let (index_json, paths_json) =
            Self::read_package_paths(archive, package_dir, prefix, python_info.as_ref())?;
        let paths_version = paths_json.paths_version;
        let (links, files): (Vec<_>, Vec<_>) = paths_json
            .paths
            .into_iter()
            .partition(|entry| package_dir.join(&entry.relative_path).is_symlink());

        let mut paths = Vec::new();
        if !links.is_empty() {
            // A symlink is linked as one whatever paths.json calls it. As a file that is
            // to be copied, rattler would write what it points to, following a link that
            // another entry has just put at the same destination.
            let links = links
                .into_iter()
                .map(|mut entry| {
                    entry.path_type = PathType::SoftLink;
                    entry.no_link = false;
                    entry.prefix_placeholder = None;
                    entry
                })
                .collect();
            let install_options = InstallOptions {
                python_info: python_info.clone(),
                index_json: Some(index_json.clone()),
                paths_json: Some(PathsJson {
                    paths: links,
                    paths_version,
                }),
                // the entry points of a noarch python package are written with its files
                link_json: Some(Some(LinkJson {
                    noarch: NoArchLinks::Python(PythonEntryPoints {
                        entry_points: vec![],
                    }),
                    package_metadata_version: 1,
                })),
                ..InstallOptions::default()
            };
            paths = link_package(package_dir, prefix, driver, install_options)
                .await
                .map_err(|e| eyre::eyre!("failed to link {}: {}", archive.display(), e))?;
            file::ensure_symlinks_stay_within(prefix.path()).wrap_err_with(|| {
                format!(
                    "refusing to install conda package {}: together with the packages linked before it, it has a symlink that leads outside the environment",
                    archive.display()
                )
            })?;
        }
        let install_options = InstallOptions {
            python_info,
            index_json: Some(index_json),
            paths_json: Some(PathsJson {
                paths: files,
                paths_version,
            }),
            ..InstallOptions::default()
        };
        let files = link_package(package_dir, prefix, driver, install_options)
            .await
            .map_err(|e| eyre::eyre!("failed to link {}: {}", archive.display(), e))?;
        paths.extend(files);
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
        tv.request.read_lockfile(&ctx.config)
    }

    /// The `<name>==<version>` spec the solver is asked for.
    fn exact_match_spec(&self, version: &str) -> Result<MatchSpec> {
        exact_match_spec(&self.tool_name(), version)
    }

    /// Install from a fresh solve (no lockfile deps).
    async fn install_fresh(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        platform_key: &str,
    ) -> Result<()> {
        let tool_name = self.tool_name();
        let match_spec = self.exact_match_spec(&tv.version)?;

        ctx.pr.set_message("fetching repodata".to_string());
        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        // The tool's cutoff, not `ctx.before_date`, which an exact pin clears: an exact
        // version names no build, so its builds are held back like any other package,
        // and the install solves the same packages as `mise lock`.
        let records = self
            .solve_packages(
                vec![match_spec],
                Self::current_conda_platform()?,
                opts,
                tv.before_date,
            )
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
        let python_info =
            Self::python_info_from_records(&all_records, Self::current_conda_platform()?);

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
        platform_info.checksum = Self::format_checksum(&main_record);
        platform_info.conda_deps = Some(dep_basenames.clone());

        // Store dep package info in tv.conda_packages for lockfile update
        for record in &all_records[..n_deps] {
            let basename = Self::record_basename(record);
            tv.conda_packages.insert(
                (platform_key.to_string(), basename),
                CondaPackageInfo {
                    url: record.url.to_string(),
                    checksum: Self::format_checksum(record),
                },
            );
        }

        Ok(())
    }

    /// Why `--locked` refuses the dependency packages `deps` that the lockfile entry of
    /// `tool` on `platform_key`, whose own package is at `main_url`, names in
    /// `[conda-packages]`: one is missing, comes from another channel, has a checksum
    /// that cannot be verified, or, when `require_checksum`, has none.
    fn locked_dependencies_problem(
        lockfile: &Lockfile,
        tool: &str,
        platform_key: &str,
        main_url: &str,
        deps: &[String],
        require_checksum: bool,
    ) -> Option<eyre::Report> {
        for key in deps {
            let Some(package) = lockfile.get_conda_package(platform_key, key) else {
                return Some(eyre::eyre!(
                    "conda package {key} of {tool} is not in the [conda-packages] section of mise.lock for platform {platform_key} (--locked mode)\n\
                    hint: Run `mise lock` to record it"
                ));
            };
            if let Err(err) = Self::check_locked_dependency(key, package, main_url) {
                return Some(err);
            }
            match package.checksum.as_deref() {
                Some(checksum) if !hash::is_well_formed_checksum(checksum) => {
                    return Some(eyre::eyre!(
                        "Invalid lockfile checksum {checksum:?} for conda package {key} of {tool} on platform {platform_key} (--locked mode)\n\
                        hint: Run `mise lock` to record checksums"
                    ));
                }
                None if require_checksum => {
                    return Some(eyre::eyre!(
                        "No lockfile checksum found for conda package {key} of {tool} on platform {platform_key} ({}) (--locked mode)\n\
                        hint: Run `mise lock` to record checksums, or set locked_require_checksum = false",
                        crate::install_layout::resolver::redact_credentials(&package.url)
                    ));
                }
                _ => {}
            }
        }
        None
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
            Self::python_info_from_basenames(&dep_basenames, Self::current_conda_platform()?);

        // Collect dep (url, checksum) pairs from lockfile (deps first, main last)
        let mut downloads: Vec<(String, Option<String>)> = vec![];
        for basename in &dep_basenames {
            if let Some(pkg_info) = lockfile.get_conda_package(platform_key, basename) {
                Self::check_locked_dependency(basename, pkg_info, &main_url)?;
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

    /// Creates a `.mise-bins` directory with entries only for binaries from the main package.
    /// Uses the PathsEntry list returned by rattler's link_package to identify which files
    /// belong to the main package (excluding transitive dependency binaries). Commands that
    /// need the package prefix get a launcher that activates it only for the command they
    /// start, so dependencies are available to that command without exposing their binaries
    /// on the user's PATH; the rest are plain symlinks. See `package_needs_activation`.
    fn create_bin_launcher_dir(&self, tv: &ToolVersion, main_paths: &[PathsEntry]) -> Result<()> {
        // `MISE_DATA_DIR` is only tilde-expanded, never absolutized, so `install_path()` can
        // be relative. Neither entry kind survives that: a relative symlink target resolves
        // against the link's own directory, and a relative path inside a launcher resolves
        // against whatever cwd the caller happens to have. Pin both to an absolute prefix.
        let install_path = std::path::absolute(tv.install_path()).wrap_err_with(|| {
            format!(
                "failed to resolve {}",
                file::display_path(tv.install_path())
            )
        })?;
        let symlink_dir = install_path.join(MISE_BINS_DIR);
        file::create_dir_all(&symlink_dir)?;
        let bin_dirs: &[&std::path::Path] = if cfg!(windows) {
            &[
                std::path::Path::new("Library/bin"),
                std::path::Path::new("Scripts"),
                std::path::Path::new("bin"),
            ]
        } else {
            &[std::path::Path::new("bin")]
        };

        let main_bin_names: BTreeSet<std::ffi::OsString> = main_paths
            .iter()
            .filter(|entry| {
                bin_dirs
                    .iter()
                    .any(|dir| entry.relative_path.starts_with(dir))
            })
            .filter_map(|entry| entry.relative_path.file_name().map(|name| name.to_owned()))
            .collect();

        // Windows always gets a launcher. There it does more than activate the prefix: the
        // entries in this directory are copies rather than symlinks, and the `.cmd` is what
        // keeps a copied `.exe` able to find the dependency DLLs brought along below.
        let needs_launcher =
            cfg!(windows) || Self::package_needs_activation(&install_path, &main_bin_names)?;

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
                Self::place_bin_entry(
                    &install_path,
                    &src,
                    &dst,
                    needs_launcher,
                    file::make_symlink_or_copy,
                )?;
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

    /// Places one of the package's commands in `.mise-bins`.
    ///
    /// A command that needs the prefix activated gets a launcher; the rest are symlinked.
    /// A symlinked command is invoked as `<prefix>/.mise-bins/<name>` rather than
    /// `<prefix>/bin/<name>`, so its `argv[0]` differs from the launcher's. That is safe
    /// only because the two directories sit at the same depth: `dirname(argv[0])/../share`
    /// reaches `<prefix>/share` either way, and a sibling lookup finds the package's other
    /// commands, which this directory holds in full whenever it is symlinked. See
    /// `mise_bins_dir_sits_beside_bin`.
    ///
    /// `link` is the symlink step, taken as an argument so the fallback below can be tested.
    fn place_bin_entry(
        prefix: &Path,
        src: &Path,
        dst: &Path,
        needs_launcher: bool,
        link: impl FnOnce(&Path, &Path) -> Result<()>,
    ) -> Result<()> {
        if needs_launcher {
            return Self::create_bin_launcher(prefix, src, dst);
        }
        if let Err(err) = link(src, dst) {
            // A filesystem that refuses symlinks would otherwise fail an install that used
            // to work, since every command was a written file before. Fall back to the
            // launcher: for this package its activation is unnecessary rather than wrong,
            // which is a better trade than no install at all.
            debug!(
                "conda: symlinking {} failed, falling back to a launcher: {err:#}",
                file::display_path(dst)
            );
            return Self::create_bin_launcher(prefix, src, dst);
        }
        Ok(())
    }

    /// Whether the commands in this package have to run through an activation launcher
    /// rather than a plain symlink in `.mise-bins`.
    ///
    /// The launcher activates the prefix for one command: it exports `CONDA_PREFIX`, sources
    /// `etc/conda/activate.d`, and puts the prefix's `bin` ahead of the user's `PATH` so the
    /// command can reach executables from its dependencies. Both of those carry a cost. The
    /// `PATH` is exported, so it survives the `exec` into the command and is inherited by
    /// everything the command spawns — a pager, an editor, a git hook — where the prefix's
    /// binaries shadow the user's. And the shell that does it is another process on every
    /// invocation.
    ///
    /// A package that ships no activation scripts, no dependency executables and no script
    /// entry points has nothing for any of that to do, so its commands are symlinked instead.
    /// That is the shape of a single-binary tool such as `conda:ripgrep` or `conda:onefetch`,
    /// where the prefix's `bin` holds only the tool itself: a symlink still resolves the
    /// package's own libraries, because `$ORIGIN` in an ELF RPATH is relative to the target.
    fn package_needs_activation(
        prefix: &Path,
        main_bin_names: &BTreeSet<std::ffi::OsString>,
    ) -> Result<bool> {
        // An activate.d script can export anything, PATH included, so its mere presence
        // means the command has to be started through the launcher.
        let activate_d = prefix.join("etc").join("conda").join("activate.d");
        match std::fs::read_dir(&activate_d) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.wrap_err_with(|| {
                        format!(
                            "failed to read an entry in {}",
                            file::display_path(&activate_d)
                        )
                    })?;
                    if entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("sh"))
                    {
                        return Ok(true);
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(err).wrap_err_with(|| {
                    format!("failed to read {}", file::display_path(&activate_d))
                });
            }
        }

        let bin_dir = prefix.join("bin");
        let entries = match std::fs::read_dir(&bin_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("failed to read {}", file::display_path(&bin_dir)));
            }
        };
        for entry in entries {
            let entry = entry.wrap_err_with(|| {
                format!(
                    "failed to read an entry in {}",
                    file::display_path(&bin_dir)
                )
            })?;
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
            if !main_bin_names.contains(&entry.file_name()) {
                // A dependency's executable. The command may shell out to it, and it is
                // reachable only while the launcher puts this directory on PATH.
                return Ok(true);
            }
            if Self::is_script(&entry.path())? {
                // A script entry point can expand `${CONDA_PREFIX}` (the bug `.mise-bins`
                // was introduced for) or take its interpreter off PATH.
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether `path` begins with a `#!` line.
    fn is_script(path: &Path) -> Result<bool> {
        use std::io::Read;

        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            // A dangling symlink is not a command anyone can run. Leave the decision to
            // the other checks rather than failing an otherwise fine install over it.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err)
                    .wrap_err_with(|| format!("failed to open {}", file::display_path(path)));
            }
        };
        let mut magic = [0u8; 2];
        match file.read_exact(&mut magic) {
            Ok(()) => Ok(&magic == b"#!"),
            // Too short to carry a shebang.
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
            Err(err) => {
                Err(err).wrap_err_with(|| format!("failed to read {}", file::display_path(path)))
            }
        }
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
    pub(crate) async fn resolve_conda_packages(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, CondaPackageInfo>> {
        let platform = Self::target_to_conda_platform(target);
        let tool_name = self.tool_name();
        let match_spec = self.exact_match_spec(&tv.version)?;

        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        let records = self
            .solve_packages(vec![match_spec], platform, opts, tv.before_date)
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
                    checksum: Self::format_checksum(record),
                },
            );
        }

        Ok(result)
    }
}

/// Builds one `VersionInfo` per distinct version from repodata records.
///
/// A version has many builds and platform records, each with its own upload
/// `timestamp`. The earliest one is the date the version first became available,
/// which is the release date `minimum_release_age` should judge. A record's
/// `indexed_timestamp` (when the channel first listed it) is preferred over
/// `timestamp` (the build start, which the builder supplies). Records with
/// neither, or with a non-positive one, don't date the version.
fn collect_version_infos<'a>(
    records: impl IntoIterator<Item = &'a PackageRecord>,
) -> Vec<VersionInfo> {
    let mut by_version: BTreeMap<String, Option<jiff::Timestamp>> = BTreeMap::new();
    for record in records {
        let ts = CondaBackend::record_released_at(record);
        let earliest = by_version.entry(record.version.to_string()).or_insert(None);
        *earliest = match (*earliest, ts) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    by_version
        .into_iter()
        .map(|(version, ts)| VersionInfo {
            version,
            created_at: ts.map(|t| t.to_string()),
            ..Default::default()
        })
        .sorted_by_cached_key(|v| Versioning::new(&v.version))
        .collect()
}

#[async_trait]
impl Backend for CondaBackend {
    fn option_spec(&self) -> Option<crate::backend::OptionSpec> {
        Some(crate::backend::OptionSpec::new(&["tool_conda_options"]))
    }

    fn get_type(&self) -> BackendType {
        BackendType::Conda
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn locked_dependencies_error(
        &self,
        config: &Config,
        tv: &ToolVersion,
        platform_key: &str,
        locked_platform: Option<&PlatformInfo>,
    ) -> Option<eyre::Report> {
        let locked_platform = locked_platform?;
        let deps = locked_platform.conda_deps.as_deref()?;
        let main_url = locked_platform.url.as_deref()?;
        if deps.is_empty() {
            return None;
        }
        let lockfile = match tv.request.read_lockfile(config) {
            Ok(lockfile) => lockfile,
            Err(err) => return Some(err),
        };
        let settings = Settings::get();
        let require_checksum = settings
            .locked_require_checksum
            .unwrap_or(settings.generate_lockfiles());
        Self::locked_dependencies_problem(
            &lockfile,
            &tv.style(),
            platform_key,
            main_url,
            deps,
            require_checksum,
        )
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
        let current_platform = Self::current_conda_platform()?;
        let tool_name = self.tool_name();

        let gateway = Self::create_gateway()?;
        let match_spec = MatchSpec::from(conda_package_name(&tool_name)?);

        let repodata: Vec<RepoData> = gateway
            .query(
                [channel],
                [current_platform, CondaPlatform::NoArch],
                [match_spec],
            )
            .await
            .map_err(|e| eyre::eyre!("failed to list versions for '{}': {}", tool_name, e))?
            .to_vec();

        let versions = collect_version_infos(
            repodata
                .iter()
                .flat_map(|data| data.into_iter().map(|record| &record.package_record)),
        );

        Ok(versions)
    }

    /// Versions are not marked as pre-releases by the channel, so the shared
    /// listing marks the ones whose version looks like one.
    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    /// The versions available depend on the channel (the `channel` option or
    /// `conda.channel`) and on this machine's platform, which the shared
    /// versions host's one list cannot describe, so conda lists from its
    /// channel and caches per channel and platform.
    async fn remote_version_cache_context(&self, config: &Arc<Config>) -> Result<Option<String>> {
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let channel = CondaOptions::new(&raw_opts).channel_name();
        Ok(Some(format!(
            "{channel}@{}",
            Self::current_conda_platform()?
        )))
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
        let match_spec = self.exact_match_spec(&tv.version)?;

        let raw_opts = tv.request.options();
        let opts = CondaOptions::new(&raw_opts);
        let records = self
            .solve_packages(vec![match_spec], platform, opts, tv.before_date)
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
            checksum: Self::format_checksum(&main),
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

/// The conda package name of a tool.
fn conda_package_name(tool_name: &str) -> Result<rattler_conda_types::PackageName> {
    use std::str::FromStr;
    rattler_conda_types::PackageName::from_str(tool_name)
        .map_err(|e| eyre::eyre!("invalid conda package name '{tool_name}': {e}"))
}

/// Build the `<name>==<version>` match spec from its parts.
///
/// Formatting the version into a match spec string and parsing that would
/// honour the match spec grammar inside the version (`[url=...]`, `channel::`,
/// a build string), and the version can come from a plain tool list that loads
/// without trust. Only the version spec grammar applies to the version here,
/// and only to name one version or a `.*` prefix of one: a group such as
/// `3.46.0|>=0` would widen the request to other versions.
fn exact_match_spec(tool_name: &str, version: &str) -> Result<MatchSpec> {
    use rattler_conda_types::{EqualityOperator, StrictRangeOperator, VersionSpec};
    let name = conda_package_name(tool_name)?;
    let invalid =
        |reason: String| eyre::eyre!("invalid conda version '{version}' for {tool_name}: {reason}");
    let spec = VersionSpec::from_str(&format!("=={version}"), ParseStrictness::Lenient)
        .map_err(|e| invalid(e.to_string()))?;
    if !matches!(
        spec,
        VersionSpec::Exact(EqualityOperator::Equals, _)
            | VersionSpec::StrictRange(StrictRangeOperator::StartsWith, _)
    ) {
        return Err(invalid("not a single version".to_string()));
    }
    Ok(MatchSpec {
        name: rattler_conda_types::PackageNameMatcher::Exact(name),
        version: Some(spec),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::{CondaBackend, CondaOptions, MISE_BINS_DIR, rewrite_request_url};
    #[cfg(unix)]
    use crate::file;
    use crate::toolset::ToolVersionOptions;
    use rattler_conda_types::package::{ArchiveIdentifier, CondaArchiveType, DistArchiveType};
    use rattler_conda_types::{
        PackageName, PackageRecord, RepoDataRecord, Version, package::DistArchiveIdentifier,
    };
    use std::collections::BTreeMap;
    #[cfg(unix)]
    use std::collections::BTreeSet;
    use std::path::Path;
    #[cfg(unix)]
    use std::process::Command;
    use std::str::FromStr;
    use url::Url;

    /// The version can come from a plain tool list that loads without trust,
    /// so the match spec grammar must not apply to it.
    #[test]
    fn exact_match_spec_reads_the_version_as_a_version_only() {
        let spec = super::exact_match_spec("sqlite", "3.46.0").unwrap();
        assert_eq!(spec.to_string(), "sqlite ==3.46.0");
        assert!(spec.url.is_none());
        assert!(super::exact_match_spec("sqlite", "3.12.*").is_ok());
        for version in [
            "ref:sqlite[url=http://127.0.0.1:20319/sqlite-3.46.0-h0.tar.bz2]",
            "3.46.0[url=/proc/self/cwd/sqlite-3.46.0-h0.tar.bz2]",
            "3.46.0[url=http://example.invalid/x.tar.bz2]",
            "3.46.0 h0",
            "conda-forge::3.46.0",
            "3.46.0|>=0",
            "3.46.0,>=0",
            "*",
        ] {
            assert!(
                super::exact_match_spec("sqlite", version).is_err(),
                "{version}"
            );
        }
        assert!(super::exact_match_spec("sqlite[url=http://example.invalid/x]", "1").is_err());
    }

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
                rattler_conda_types::package::BuildString::new_unchecked(build),
            ),
            identifier: DistArchiveIdentifier::new(archive_id, DistArchiveType::Conda(conda_type)),
            url: Url::parse(url).unwrap(),
            channel: None,
        }
    }

    #[test]
    fn version_infos_use_earliest_timestamp() {
        use rattler_conda_types::utils::TimestampMs;
        let ms =
            |s: &str| TimestampMs::from_timestamp_millis(s.parse::<jiff_0_2::Timestamp>().unwrap());
        let mut a = make_record("vim", "9.1.0", "a_0", "https://x.invalid/a.conda").package_record;
        a.timestamp = Some(ms("2024-03-02T00:00:00Z"));
        let mut b = make_record("vim", "9.1.0", "b_1", "https://x.invalid/b.conda").package_record;
        b.timestamp = Some(ms("2024-03-01T00:00:00Z"));
        let c = make_record("vim", "9.1.0", "c_2", "https://x.invalid/c.conda").package_record;
        let undated =
            make_record("vim", "9.0.0", "a_0", "https://x.invalid/d.conda").package_record;
        let infos = super::collect_version_infos([&a, &b, &c, &undated]);
        assert_eq!(infos.len(), 2);
        assert_eq!(infos[0].version, "9.0.0");
        assert_eq!(infos[0].created_at, None);
        assert_eq!(infos[1].version, "9.1.0");
        assert_eq!(infos[1].created_at.as_deref(), Some("2024-03-01T00:00:00Z"));
    }

    #[test]
    fn version_infos_prefer_indexed_timestamp() {
        use rattler_conda_types::utils::TimestampMs;
        let ms = |s: &str| TimestampMs::from_timestamp_millis(s.parse().unwrap());
        let mut a = make_record("vim", "9.1.0", "a_0", "https://x.invalid/a.conda").package_record;
        a.timestamp = Some(ms("2020-01-01T00:00:00Z"));
        a.indexed_timestamp = Some(ms("2024-03-01T00:00:00Z"));
        let infos = super::collect_version_infos([&a]);
        assert_eq!(infos[0].created_at.as_deref(), Some("2024-03-01T00:00:00Z"));
    }

    #[test]
    fn minimum_release_age_hides_too_new_conda_version() {
        use rattler_conda_types::utils::TimestampMs;
        let mut new =
            make_record("vim", "9.2.0", "a_0", "https://x.invalid/a.conda").package_record;
        new.timestamp = Some(TimestampMs::from_timestamp_millis(
            "2024-06-10T00:00:00Z".parse().unwrap(),
        ));
        let mut old =
            make_record("vim", "9.1.0", "a_0", "https://x.invalid/b.conda").package_record;
        old.timestamp = Some(TimestampMs::from_timestamp_millis(
            "2024-01-01T00:00:00Z".parse().unwrap(),
        ));
        let infos = super::collect_version_infos([&new, &old]);
        let cutoff: jiff::Timestamp = "2024-06-01T00:00:00Z".parse().unwrap();
        let kept = crate::backend::VersionInfo::filter_by_date(infos, cutoff);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].version, "9.1.0");
    }

    #[test]
    fn minimum_release_age_holds_back_too_new_builds_of_a_version() {
        use rattler_conda_types::utils::TimestampMs;
        let at = |s: &str| Some(TimestampMs::from_timestamp_millis(s.parse().unwrap()));
        let channel = "https://conda.anaconda.org/conda-forge/linux-64";
        let mut old_build = make_record("vim", "9.1.0", "a_0", &format!("{channel}/a.conda"));
        old_build.package_record.timestamp = at("2024-01-01T00:00:00Z");
        let mut new_build = make_record("vim", "9.1.0", "b_1", &format!("{channel}/b.conda"));
        new_build.package_record.timestamp = at("2024-06-10T00:00:00Z");
        let mut new_dep = make_record("zlib", "1.3.1", "h_2", &format!("{channel}/z.conda"));
        new_dep.package_record.timestamp = at("2020-01-01T00:00:00Z");
        new_dep.package_record.indexed_timestamp = at("2024-06-10T00:00:00Z");
        let undated = make_record("ncurses", "6.5", "h_0", &format!("{channel}/n.conda"));
        let records = vec![old_build, new_build, new_dep, undated];

        let cutoff: jiff::Timestamp = "2024-06-01T00:00:00Z".parse().unwrap();
        let builds = |kept: Vec<RepoDataRecord>| {
            kept.iter()
                .map(|r| r.package_record.build.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            builds(CondaBackend::released_before(records.clone(), Some(cutoff))),
            ["a_0", "h_0"]
        );
        assert_eq!(
            builds(CondaBackend::released_before(records, None)).len(),
            4
        );
    }

    #[test]
    fn request_url_rewrite_preserves_request_data() {
        let original = Url::parse("https://upstream.invalid/channel/noarch/repodata.json").unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::POST, original);
        request.headers_mut().insert(
            "x-conda-test",
            reqwest::header::HeaderValue::from_static("preserved"),
        );
        *request.body_mut() = Some("request-body".into());

        rewrite_request_url(&mut request, |url| {
            *url = Url::parse("https://mirror.invalid/conda/noarch/repodata.json").unwrap();
        })
        .unwrap();

        assert_eq!(
            request.url().as_str(),
            "https://mirror.invalid/conda/noarch/repodata.json"
        );
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.headers()["x-conda-test"], "preserved");
        assert_eq!(
            request.body().and_then(reqwest::Body::as_bytes),
            Some(&b"request-body"[..])
        );
    }

    #[test]
    fn request_url_rewrite_can_be_a_noop() {
        let original = Url::parse("https://upstream.invalid/channel/noarch/repodata.json").unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::GET, original.clone());

        rewrite_request_url(&mut request, |_| {}).unwrap();

        assert_eq!(request.url(), &original);
    }

    #[test]
    fn request_url_rewrite_rejects_credential_downgrade() {
        let original = Url::parse("https://upstream.invalid/channel/noarch/repodata.json").unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::GET, original);
        request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer channel-token"),
        );

        let err = rewrite_request_url(&mut request, |url| {
            *url = Url::parse("http://upstream.invalid/conda/noarch/repodata.json").unwrap();
        })
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("refusing to send credentials over an HTTPS-to-HTTP URL replacement")
        );
    }

    #[test]
    fn request_url_rewrite_allows_unauthenticated_downgrade() {
        let original = Url::parse("https://upstream.invalid/channel/noarch/repodata.json").unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::GET, original);

        rewrite_request_url(&mut request, |url| {
            *url = Url::parse("http://mirror.invalid/conda/noarch/repodata.json").unwrap();
        })
        .unwrap();

        assert_eq!(
            request.url().as_str(),
            "http://mirror.invalid/conda/noarch/repodata.json"
        );
    }

    #[test]
    fn request_url_rewrite_scopes_credentials_to_the_original_host() {
        let original =
            Url::parse("https://user:password@upstream.invalid/channel/noarch/repodata.json")
                .unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::GET, original);
        request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer channel-token"),
        );
        request.headers_mut().insert(
            "x-conda-test",
            reqwest::header::HeaderValue::from_static("preserved"),
        );

        rewrite_request_url(&mut request, |url| {
            url.set_host(Some("mirror.invalid")).unwrap();
            url.set_scheme("http").unwrap();
        })
        .unwrap();

        assert!(
            !request
                .headers()
                .contains_key(reqwest::header::AUTHORIZATION)
        );
        assert_eq!(request.headers()["x-conda-test"], "preserved");
        assert_eq!(request.url().scheme(), "http");
        assert_eq!(request.url().username(), "");
        assert_eq!(request.url().password(), None);
    }

    #[test]
    fn request_url_rewrite_keeps_credentials_on_the_same_host() {
        let original =
            Url::parse("https://user:password@upstream.invalid/channel/noarch/repodata.json")
                .unwrap();
        let mut request = reqwest::Request::new(reqwest::Method::GET, original);
        request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_static("Bearer channel-token"),
        );

        rewrite_request_url(&mut request, |url| {
            url.set_path("/mirror/noarch/repodata.json");
        })
        .unwrap();

        assert_eq!(
            request.headers()[reqwest::header::AUTHORIZATION],
            "Bearer channel-token"
        );
        assert_eq!(request.url().username(), "user");
        assert_eq!(request.url().password(), Some("password"));
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

    /// Installing onto a filesystem that refuses symlinks must not fail a package that
    /// would otherwise be symlinked — before this, every command was a written file.
    #[cfg(unix)]
    #[test]
    fn a_refused_symlink_falls_back_to_a_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("prefix");
        let target = prefix.join("bin/tool");
        let dst = prefix.join(MISE_BINS_DIR).join("tool");
        file::create_dir_all(target.parent().unwrap()).unwrap();
        file::create_dir_all(dst.parent().unwrap()).unwrap();
        file::write(&target, "\x7fELF-ish binary").unwrap();

        CondaBackend::place_bin_entry(&prefix, &target, &dst, false, |_, _| {
            Err(eyre::eyre!("read-only or symlink-hostile filesystem"))
        })
        .unwrap();

        assert!(!dst.is_symlink());
        let launcher = std::fs::read_to_string(&dst).unwrap();
        assert!(launcher.contains("export CONDA_PREFIX"));
        assert!(launcher.contains("exec '"));
    }

    /// A tool that resolves its resources relative to `argv[0]` without following the
    /// symlink — `dirname(argv[0])/../share` is the common shape — keeps working through
    /// `.mise-bins` only while that directory is as deep in the prefix as `bin` is. Moving
    /// it deeper would send those lookups outside the package, and nothing about a compiled
    /// binary would reveal the breakage at install time.
    #[test]
    fn mise_bins_dir_sits_beside_bin() {
        assert_eq!(
            Path::new(MISE_BINS_DIR).components().count(),
            Path::new("bin").components().count()
        );
    }

    /// Builds a prefix with `bin/tool` plus whatever extra files the case needs, and
    /// reports whether its commands would be launchers or plain symlinks.
    #[cfg(unix)]
    fn needs_activation_for(
        temp: &Path,
        extra_bins: &[&str],
        tool_contents: &str,
        activate_scripts: &[&str],
    ) -> bool {
        let prefix = temp.join("prefix");
        let bin_dir = prefix.join("bin");
        file::create_dir_all(&bin_dir).unwrap();
        file::write(bin_dir.join("tool"), tool_contents).unwrap();
        for name in extra_bins {
            file::write(bin_dir.join(name), "").unwrap();
        }
        if !activate_scripts.is_empty() {
            let activate_dir = prefix.join("etc/conda/activate.d");
            file::create_dir_all(&activate_dir).unwrap();
            for name in activate_scripts {
                file::write(activate_dir.join(name), "").unwrap();
            }
        }
        let main_bin_names = BTreeSet::from([std::ffi::OsString::from("tool")]);
        CondaBackend::package_needs_activation(&prefix, &main_bin_names).unwrap()
    }

    /// A single-binary package (`conda:ripgrep`, `conda:onefetch`) has nothing to activate:
    /// no activate.d, no dependency executables, no script entry point.
    #[cfg(unix)]
    #[test]
    fn self_contained_package_skips_the_launcher() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!needs_activation_for(
            temp.path(),
            &[],
            "\x7fELF-ish binary",
            &[]
        ));
    }

    /// `conda:curl` ships 54 dependency executables in the prefix's bin. The command may
    /// shell out to one, and only the launcher's PATH makes them reachable.
    #[cfg(unix)]
    #[test]
    fn dependency_executables_require_the_launcher() {
        let temp = tempfile::tempdir().unwrap();
        assert!(needs_activation_for(
            temp.path(),
            &["openssl"],
            "\x7fELF-ish binary",
            &[]
        ));
    }

    /// The `conda:jdtls` case from https://github.com/jdx/mise/discussions/8576: the entry
    /// point is a shell script that expands `${CONDA_PREFIX}`.
    #[cfg(unix)]
    #[test]
    fn script_entry_point_requires_the_launcher() {
        let temp = tempfile::tempdir().unwrap();
        assert!(needs_activation_for(
            temp.path(),
            &[],
            "#!/bin/sh\nexec ${CONDA_PREFIX}/libexec/tool \"$@\"\n",
            &[]
        ));
    }

    /// An activate.d script can export anything, including PATH, so its presence alone
    /// means the command has to start through the launcher.
    #[cfg(unix)]
    #[test]
    fn activate_scripts_require_the_launcher() {
        let temp = tempfile::tempdir().unwrap();
        assert!(needs_activation_for(
            temp.path(),
            &[],
            "\x7fELF-ish binary",
            &["libxml2-split_activate.sh"]
        ));
    }

    /// Only `.sh` scripts are sourced by the unix launcher, so a stray `.bat` beside them
    /// is not a reason to build one.
    #[cfg(unix)]
    #[test]
    fn non_shell_activate_scripts_do_not_require_the_launcher() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!needs_activation_for(
            temp.path(),
            &[],
            "\x7fELF-ish binary",
            &["windows_only.bat"]
        ));
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

    /// Writes a `.tar.bz2` conda package with the given files and symlinks (path, target).
    /// A file whose name is not a plain relative path is only listed in paths.json.
    #[cfg(unix)]
    fn write_conda_package(
        dir: &Path,
        name: &str,
        files: &[&str],
        links: &[(&str, &str)],
    ) -> std::path::PathBuf {
        use rattler_conda_types::compression_level::CompressionLevel;

        let root = dir.join("package");
        file::remove_all(&root).unwrap();
        let mut members = vec![];
        let mut write = |path: &str, content: String| {
            let member = root.join(path);
            file::create_dir_all(member.parent().unwrap()).unwrap();
            file::write(&member, content).unwrap();
            members.push(member);
        };
        let entries: Vec<_> = links
            .iter()
            .map(|(path, _)| (*path, "softlink"))
            .chain(files.iter().map(|path| (*path, "hardlink")))
            .map(|(path, kind)| serde_json::json!({"_path": path, "path_type": kind}))
            .collect();
        write(
            "info/index.json",
            serde_json::json!({"name": name, "version": "1.0", "build": "0", "build_number": 0})
                .to_string(),
        );
        write(
            "info/paths.json",
            serde_json::json!({"paths": entries, "paths_version": 1}).to_string(),
        );
        for path in files
            .iter()
            .filter(|path| file::is_safe_relative_path(path))
        {
            write(path, format!("{path} of {name}"));
        }
        for (path, target) in links {
            let member = root.join(path);
            file::create_dir_all(member.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink(target, &member).unwrap();
            members.push(member);
        }

        let archive = dir.join(format!("{name}-1.0-0.tar.bz2"));
        rattler_package_streaming::write::write_tar_bz2_package(
            std::fs::File::create(&archive).unwrap(),
            &root,
            &members,
            CompressionLevel::Lowest,
            None,
            None,
        )
        .unwrap();
        archive
    }

    /// Links the packages into a fresh prefix in the given order, as an install does.
    #[cfg(unix)]
    async fn link_packages(dir: &Path, archives: &[&Path]) -> eyre::Result<()> {
        use rattler::install::InstallDriver;
        use rattler_conda_types::prefix::Prefix;

        let prefix = dir.join("prefix");
        file::remove_all(&prefix)?;
        let prefix = Prefix::create(prefix)?;
        let driver = InstallDriver::default();
        for archive in archives {
            CondaBackend::install_package(archive, &prefix, &driver, None).await?;
        }
        Ok(())
    }

    /// Tools from different channels share the lockfile's `conda-packages` section, so a
    /// file name that two channels have must not be one key: the entry written last
    /// would be what both tools install.
    #[test]
    fn record_basename_tells_channels_apart() {
        let key = |dir: &str| {
            let url = format!("{dir}/zlib-1.3.1-hb9d3cd8_2.conda");
            CondaBackend::record_basename(&make_record("zlib", "1.3.1", "hb9d3cd8_2", &url))
        };
        let team = key("https://conda.example.com/team/linux-64");
        assert_eq!(team, key("https://conda.example.com/team/linux-64"));
        assert_ne!(team, key("https://conda.example.com/other/linux-64"));
        assert_ne!(team, key("https://conda.example.com/team/noarch"));
        assert_ne!(team, key("https://conda.anaconda.org/conda-forge/linux-64"));
        let (basename, tag) = team.split_once('@').unwrap();
        assert_eq!(basename, "zlib-1.3.1-hb9d3cd8_2");
        assert_eq!(tag.len(), 32);
    }

    /// An archive in the store is reused when it matches its checksum, and a package
    /// may have none, so two channels must not write the same path.
    #[test]
    fn archive_dir_tells_channels_apart() {
        let dir = CondaBackend::archive_dir;
        let store = CondaBackend::conda_data_dir();
        let team = dir("https://conda.example.com/team/linux-64/zlib-1.3.1-h_2.conda");
        assert_eq!(team.parent(), Some(store.as_path()));
        assert_eq!(
            team,
            dir("https://conda.example.com/team/linux-64/xz-5.2.6-h_0.conda")
        );
        assert_ne!(
            team,
            dir("https://conda.example.com/other/linux-64/zlib-1.3.1-h_2.conda")
        );
    }

    /// A locked install reads the python version out of the `conda_deps` key.
    #[test]
    fn python_info_is_read_from_a_key_that_names_its_channel() {
        let url = "https://conda.example.com/team/linux-64/python-3.12.3-hab00c5b_0_cpython.conda";
        let record = make_record("python", "3.12.3", "hab00c5b_0_cpython", url);
        let key = CondaBackend::record_basename(&record);
        assert!(key.contains('@'));
        let platform = rattler_conda_types::Subdir::Linux64;
        let info = CondaBackend::python_info_from_basenames(&[key], platform).unwrap();
        assert_eq!(info.short_version, (3, 12));
    }

    /// A locked dependency comes from the tool's own channel, whatever its key, and a
    /// key that names a channel names the one its url points into.
    #[test]
    fn locked_dependencies_come_from_the_tool_channel() {
        let main = "https://conda.anaconda.org/conda-forge/linux-64/tool-1.0-h_0.conda";
        let package = |url: &str| super::CondaPackageInfo {
            url: url.to_string(),
            checksum: None,
        };
        let check = |key: &str, url: &str| {
            CondaBackend::check_locked_dependency(key, &package(url), main)
                .map_err(|e| e.to_string())
        };
        let forge = "https://conda.anaconda.org/conda-forge/linux-64/zlib-1.3.1-h_2.conda";
        let forge_noarch = "https://conda.anaconda.org/conda-forge/noarch/tzdata-2024a-h_0.conda";
        let evil = "https://conda.example.com/evil/linux-64/zlib-1.3.1-h_2.conda";
        let key =
            |url: &str| CondaBackend::record_basename(&make_record("zlib", "1.3.1", "h_2", url));

        check(&key(forge), forge).unwrap();
        check(&key(forge_noarch), forge_noarch).unwrap();
        check("zlib-1.3.1-h_2", forge).unwrap();
        let err = check("zlib-1.3.1-h_2", evil).unwrap_err();
        assert!(err.contains("not in the channel"), "{err}");
        let err = check(&key(evil), evil).unwrap_err();
        assert!(err.contains("not in the channel"), "{err}");
        let err = check(&key(forge_noarch), forge).unwrap_err();
        assert!(err.contains("names another channel"), "{err}");

        // the messages leave out credentials in either url
        let private = "https://user:token@conda.example.com/team/linux-64/tool-1.0-h_0.conda";
        let private_dep = "https://user:token@conda.example.com/team/noarch/zlib-1.3.1-h_2.conda";
        let err =
            CondaBackend::check_locked_dependency("zlib-1.3.1-h_2", &package(private_dep), main)
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("not in the channel") && !err.contains("token"),
            "{err}"
        );
        let err = CondaBackend::check_locked_dependency("zlib-1.3.1-h_2", &package(forge), private)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("not in the channel") && !err.contains("token"),
            "{err}"
        );
        let err =
            CondaBackend::check_locked_dependency(&key(forge), &package(private_dep), private)
                .unwrap_err()
                .to_string();
        assert!(
            err.contains("names another channel") && !err.contains("token"),
            "{err}"
        );
    }

    #[test]
    fn format_checksum_falls_back_to_md5() {
        let url = "https://conda.anaconda.org/conda-forge/linux-64/zlib-1.3.1-h_2.conda";
        let mut record = make_record("zlib", "1.3.1", "h_2", url);
        assert_eq!(CondaBackend::format_checksum(&record), None);

        record.package_record.md5 = Some(Default::default());
        assert_eq!(
            CondaBackend::format_checksum(&record).as_deref(),
            Some("md5:00000000000000000000000000000000")
        );

        record.package_record.sha256 = Some(Default::default());
        let checksum = CondaBackend::format_checksum(&record).unwrap();
        assert_eq!(checksum, format!("sha256:{}", "0".repeat(64)));
    }

    #[test]
    fn verify_checksum_compares_every_algorithm_and_rejects_unknown_ones() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pkg.conda");
        std::fs::write(&path, b"abc").unwrap();
        let sha256 = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let md5 = "md5:900150983cd24fb0d6963f7d28e17f72";
        let verify = |checksum: &str| CondaBackend::verify_checksum(&path, Some(checksum));
        assert!(verify(sha256).unwrap());
        assert!(verify(md5).unwrap());
        assert!(CondaBackend::verify_checksum(&path, None).unwrap());

        std::fs::write(&path, b"replaced").unwrap();
        assert!(!verify(sha256).unwrap());
        assert!(!verify(md5).unwrap());
        assert!(verify("crc32:352441c2").is_err());
        assert!(verify("900150983cd24fb0d6963f7d28e17f72").is_err());
    }

    #[test]
    fn locked_mode_refuses_dependencies_it_cannot_verify() {
        let platform = "linux-x64";
        let main = "https://conda.anaconda.org/conda-forge/linux-64/tool-1.0-h_0.conda";
        let dep_url = "https://conda.anaconda.org/conda-forge/linux-64/zlib-1.3.1-h_2.conda";
        let key = CondaBackend::record_basename(&make_record("zlib", "1.3.1", "h_2", dep_url));
        let lockfile_with = |checksum: Option<&str>| {
            let mut lockfile = super::Lockfile::default();
            lockfile.set_conda_package(
                platform,
                &key,
                super::CondaPackageInfo {
                    url: dep_url.to_string(),
                    checksum: checksum.map(str::to_string),
                },
            );
            lockfile
        };
        let problem = |lockfile: &super::Lockfile, deps: &[String], require: bool| {
            CondaBackend::locked_dependencies_problem(
                lockfile,
                "conda:tool",
                platform,
                main,
                deps,
                require,
            )
            .map(|e| e.to_string())
        };
        let deps = vec![key.clone()];
        let sha256 = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

        assert_eq!(problem(&lockfile_with(Some(sha256)), &deps, true), None);
        assert_eq!(problem(&lockfile_with(None), &deps, false), None);
        let err = problem(&lockfile_with(None), &deps, true).unwrap();
        assert!(err.contains("No lockfile checksum found"), "{err}");
        for malformed in [
            "crc32:00",
            "sha256:",
            "sha256:00",
            "md5:zz",
            sha256.trim_end_matches('d'),
        ] {
            let err = problem(&lockfile_with(Some(malformed)), &deps, false).unwrap();
            assert!(
                err.contains("Invalid lockfile checksum"),
                "{malformed}: {err}"
            );
        }
        let err = problem(&super::Lockfile::default(), &deps, false).unwrap();
        assert!(
            err.contains("is not in the [conda-packages] section"),
            "{err}"
        );
    }

    /// rattler writes what a package names by name, through the links that are in the
    /// prefix by then, so no link may lead out of the prefix when a file is written.
    #[cfg(unix)]
    #[tokio::test]
    async fn install_package_keeps_packages_inside_the_prefix() {
        const README: &[&str] = &["share/README"];
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let prefix = dir.join("prefix");
        let error = |result: eyre::Result<()>| format!("{:#}", result.unwrap_err());

        // links that stay inside are linked, and so is a file that two packages have
        let links = [("lib/libfoo.so", "libfoo.so.1"), ("doc", "share")];
        let inside = write_conda_package(dir, "inside", README, &links);
        let other = write_conda_package(dir, "other", &["share/README", "bin/tool"], &[]);
        let (inside, other) = (inside.as_path(), other.as_path());
        link_packages(dir, &[inside, other]).await.unwrap();
        assert_eq!(
            file::read_to_string(prefix.join("doc/README")).unwrap(),
            "share/README of inside"
        );
        assert!(prefix.join("lib/libfoo.so").is_symlink());
        assert!(prefix.join("bin/tool").exists());
        assert!(prefix.join("__clobbers__/other/share/README").exists());

        for target in ["../..", "/"] {
            let outside = write_conda_package(dir, "outside", README, &[("lib/out", target)]);
            let err = error(link_packages(dir, &[outside.as_path()]).await);
            assert!(err.contains("points outside of"), "{err}");
            assert!(prefix.join("lib").symlink_metadata().is_err());
        }

        // Each of these stays inside on its own: `d/y` is not in the first package, and
        // in the second it is the prefix itself. The package that completes the way out
        // is refused before any of its files are written.
        let first = write_conda_package(dir, "first", &["bin/first"], &[("d/x", "y/../..")]);
        let second = write_conda_package(dir, "second", &["bin/second"], &[("d/y", "..")]);
        let (first, second) = (first.as_path(), second.as_path());
        link_packages(dir, &[first]).await.unwrap();
        link_packages(dir, &[second]).await.unwrap();
        for (order, unwritten) in [([first, second], "second"), ([second, first], "first")] {
            let err = error(link_packages(dir, &order).await);
            assert!(err.contains("points outside of"), "{err}");
            assert!(!prefix.join("bin").join(unwritten).exists());
        }

        // A file below a link of an earlier package is linked to
        // `__clobbers__/<name>/<path>` instead of through the link, so no package may
        // bring that directory, lead out of it with its name, or share the name.
        // A case-insensitive file system opens the directory under any case.
        for dir_name in ["__clobbers__", "__CLOBBERS__"] {
            let link = format!("{dir_name}/other/share");
            let links = [(link.as_str(), "../../lib")];
            let clobber = write_conda_package(dir, "clobber", README, &links);
            let err = error(link_packages(dir, &[clobber.as_path()]).await);
            assert!(err.contains("unsafe path"), "{dir_name}: {err}");
            assert!(prefix.join(dir_name).symlink_metadata().is_err());
        }

        // named `..`, the second package would have `d/s/link` and `t/victim` linked
        // through the links `d/s` and `t` of the first, onto the file next to the prefix
        file::write(dir.join("victim"), "untouched").unwrap();
        let links = [("d/s", ".."), ("t", "link")];
        let earlier = write_conda_package(dir, "earlier", README, &links);
        let parent = write_conda_package(dir, "..", &["t/victim"], &[("d/s/link", "..")]);
        let err = error(link_packages(dir, &[earlier.as_path(), parent.as_path()]).await);
        assert!(err.contains("\"..\""), "{err}");
        assert_eq!(
            file::read_to_string(dir.join("victim")).unwrap(),
            "untouched"
        );
        let err = error(link_packages(dir, &[inside, inside, inside]).await);
        assert!(err.contains("\"inside\""), "{err}");

        // the names in paths.json are joined onto the prefix as they are
        for path in ["doc/../../out", "../out", "/out", "./share/README"] {
            let named = write_conda_package(dir, "named", &[path], &[("doc", "share")]);
            let err = error(link_packages(dir, &[named.as_path()]).await);
            assert!(err.contains("unsafe path"), "{err}");
            assert!(prefix.join("doc").symlink_metadata().is_err());
        }
    }
}
