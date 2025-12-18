use crate::backend::SecurityFeature;
use crate::backend::VersionInfo;
use crate::backend::asset_matcher::{self, Asset, ChecksumFetcher};
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, lookup_platform_key, lookup_platform_key_for_target,
    template_string, try_with_v_prefix, verify_artifact,
};
use crate::cli::args::{BackendArg, ToolVersionType};
use crate::config::{Config, Settings};
use crate::file;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersionOptions;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{backend::Backend, github, gitlab};
use async_trait::async_trait;
use eyre::Result;
use regex::Regex;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;

#[derive(Debug)]
pub struct UnifiedGitBackend {
    ba: Arc<BackendArg>,
}

struct ReleaseAsset {
    name: String,
    url: String,
    url_api: String,
    digest: Option<String>,
}

const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";
const DEFAULT_GITLAB_API_BASE_URL: &str = "https://gitlab.com/api/v4";

/// Status returned from verification attempts
enum VerificationStatus {
    /// No attestations or provenance found (not an error, tool may not have them)
    NoAttestations,
    /// An error occurred during verification
    Error(String),
}

/// Returns install-time-only option keys for GitHub/GitLab backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "asset_pattern".into(),
        "url".into(),
        "version_prefix".into(),
    ]
}

#[async_trait]
impl Backend for UnifiedGitBackend {
    fn get_type(&self) -> BackendType {
        if self.is_gitlab() {
            BackendType::Gitlab
        } else {
            BackendType::Github
        }
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<SecurityFeature> {
        // Only report security features for GitHub (not GitLab yet)
        if self.is_gitlab() {
            return vec![];
        }

        let mut features = vec![];

        // Get the latest release to check for security assets
        let repo = self.ba.tool_name();
        let opts = self.ba.opts();
        let api_url = self.get_api_url(&opts);

        let releases = github::list_releases_from_url(api_url.as_str(), &repo)
            .await
            .unwrap_or_default();

        let latest_release = releases.first();

        // Check for checksum files in assets
        if let Some(release) = latest_release {
            let has_checksum = release.assets.iter().any(|a| {
                let name = a.name.to_lowercase();
                name.contains("sha256")
                    || name.contains("checksum")
                    || name.ends_with(".sha256")
                    || name.ends_with(".sha512")
            });
            if has_checksum {
                features.push(SecurityFeature::Checksum {
                    algorithm: Some("sha256".to_string()),
                });
            }
        }

        // Check for GitHub Attestations (assets with .sigstore.json or .sigstore extension)
        if let Some(release) = latest_release {
            let has_attestations = release.assets.iter().any(|a| {
                let name = a.name.to_lowercase();
                name.ends_with(".sigstore.json") || name.ends_with(".sigstore")
            });
            if has_attestations {
                features.push(SecurityFeature::GithubAttestations {
                    signer_workflow: None,
                });
            }
        }

        // Check for SLSA provenance (intoto.jsonl files)
        if let Some(release) = latest_release {
            let has_slsa = release.assets.iter().any(|a| {
                let name = a.name.to_lowercase();
                name.contains(".intoto.jsonl")
                    || name.contains("provenance")
                    || name.ends_with(".attestation")
            });
            if has_slsa {
                features.push(SecurityFeature::Slsa { level: None });
            }
        }

        features
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let repo = self.ba.tool_name();
        let id = self.ba.to_string();
        let opts = self.ba.opts();
        let api_url = self.get_api_url(&opts);
        let version_prefix = opts.get("version_prefix");

        // Derive web URL base from API URL for enterprise support
        let web_url_base = if self.is_gitlab() {
            if api_url == DEFAULT_GITLAB_API_BASE_URL {
                format!("https://gitlab.com/{}", repo)
            } else {
                // Enterprise GitLab - derive web URL from API URL
                let web_url = api_url.replace("/api/v4", "");
                format!("{}/{}", web_url, repo)
            }
        } else if api_url == DEFAULT_GITHUB_API_BASE_URL {
            format!("https://github.com/{}", repo)
        } else {
            // Enterprise GitHub - derive web URL from API URL
            let web_url = api_url.replace("/api/v3", "").replace("api.", "");
            format!("{}/{}", web_url, repo)
        };

        // Get releases with full metadata from GitHub or GitLab
        let raw_versions: Vec<VersionInfo> = if self.is_gitlab() {
            gitlab::list_releases_from_url(api_url.as_str(), &repo)
                .await?
                .into_iter()
                .filter(|r| version_prefix.is_none_or(|p| r.tag_name.starts_with(p)))
                .map(|r| VersionInfo {
                    version: self.strip_version_prefix(&r.tag_name),
                    created_at: r.released_at,
                    release_url: Some(format!("{}/-/releases/{}", web_url_base, r.tag_name)),
                })
                .collect()
        } else {
            github::list_releases_from_url(api_url.as_str(), &repo)
                .await?
                .into_iter()
                .filter(|r| version_prefix.is_none_or(|p| r.tag_name.starts_with(p)))
                .map(|r| VersionInfo {
                    version: self.strip_version_prefix(&r.tag_name),
                    created_at: Some(r.created_at),
                    release_url: Some(format!("{}/releases/tag/{}", web_url_base, r.tag_name)),
                })
                .collect()
        };

        // Apply common validation and reverse order
        let versions = raw_versions
            .into_iter()
            .filter(|v| match v.version.parse::<ToolVersionType>() {
                Ok(ToolVersionType::Version(_)) => true,
                _ => {
                    warn!("Invalid version: {id}@{}", v.version);
                    false
                }
            })
            .rev()
            .collect();

        Ok(versions)
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = self.get_api_url(&opts);

        // Check if URL already exists in lockfile platforms first
        let platform_key = self.get_platform_key();

        let asset = if let Some(existing_platform) = tv.lock_platforms.get(&platform_key)
            && existing_platform.url.is_some()
        {
            debug!(
                "Using existing URL from lockfile for platform {}: {}",
                platform_key,
                existing_platform.url.clone().unwrap_or_default()
            );
            ReleaseAsset {
                name: get_filename_from_url(existing_platform.url.as_deref().unwrap_or("")),
                url: existing_platform.url.clone().unwrap_or_default(),
                url_api: existing_platform.url_api.clone().unwrap_or_default(),
                digest: None, // Don't use old digest from lockfile, will be fetched fresh if needed
            }
        } else {
            // Find the asset URL for this specific version
            self.resolve_asset_url(&tv, &opts, &repo, &api_url).await?
        };

        // Download and install
        self.download_and_install(ctx, &mut tv, &asset, &opts)
            .await?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<std::path::PathBuf>> {
        if self.get_filter_bins(tv).is_some() {
            return Ok(vec![tv.install_path().join(".mise-bins")]);
        }

        self.discover_bin_paths(tv)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        // These options affect which artifact is downloaded
        for key in ["asset_pattern", "url", "version_prefix"] {
            if let Some(value) = opts.get(key) {
                result.insert(key.to_string(), value.clone());
            }
        }

        result
    }

    /// Resolve platform-specific lock information for cross-platform lockfile generation.
    /// This fetches release asset metadata including SHA256 digests from GitHub/GitLab API.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = self.get_api_url(&opts);

        // Resolve asset for the target platform
        let asset = self
            .resolve_asset_url_for_target(tv, &opts, &repo, &api_url, target)
            .await;

        match asset {
            Ok(asset) => Ok(PlatformInfo {
                url: Some(asset.url),
                url_api: Some(asset.url_api),
                checksum: asset.digest,
                size: None,
            }),
            Err(e) => {
                debug!(
                    "Failed to resolve asset for {} on {}: {}",
                    self.ba.full(),
                    target.to_key(),
                    e
                );
                Ok(PlatformInfo::default())
            }
        }
    }
}

impl UnifiedGitBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn is_gitlab(&self) -> bool {
        self.ba.backend_type() == BackendType::Gitlab
    }

    fn repo(&self) -> String {
        // Use tool_name() method to properly resolve aliases
        // This ensures that when an alias like "test-edit = github:microsoft/edit" is used,
        // the repository name is correctly extracted as "microsoft/edit"
        self.ba.tool_name()
    }

    // Helper to format asset names for error messages
    fn format_asset_list<'a, I>(assets: I) -> String
    where
        I: Iterator<Item = &'a String>,
    {
        assets.cloned().collect::<Vec<_>>().join(", ")
    }

    fn get_api_url(&self, opts: &ToolVersionOptions) -> String {
        opts.get("api_url")
            .map(|s| s.as_str())
            .unwrap_or(if self.is_gitlab() {
                DEFAULT_GITLAB_API_BASE_URL
            } else {
                DEFAULT_GITHUB_API_BASE_URL
            })
            .to_string()
    }

    /// Downloads and installs the asset
    async fn download_and_install(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        asset: &ReleaseAsset,
        opts: &ToolVersionOptions,
    ) -> Result<()> {
        let filename = asset.name.clone();
        let file_path = tv.download_path().join(&filename);

        // Count operations dynamically:
        // 1. Download (always)
        // 2. Verify checksum (if checksum option present)
        // 3. Extract/install (if file needs extraction)
        let mut op_count = 1; // download

        // Check if we'll verify checksum
        let has_checksum = lookup_platform_key(opts, "checksum")
            .or_else(|| opts.get("checksum").cloned())
            .is_some();
        if has_checksum {
            op_count += 1;
        }

        // Check if we'll extract (archives need extraction)
        let needs_extraction = filename.ends_with(".tar.gz")
            || filename.ends_with(".tar.xz")
            || filename.ends_with(".tar.bz2")
            || filename.ends_with(".tar.zst")
            || filename.ends_with(".tgz")
            || filename.ends_with(".txz")
            || filename.ends_with(".tbz2")
            || filename.ends_with(".zip");
        if needs_extraction {
            op_count += 1;
        }

        ctx.pr.start_operations(op_count);

        // Store the asset URL and digest (if available) in the tool version
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        platform_info.url = Some(asset.url.clone());
        platform_info.url_api = Some(asset.url_api.clone());
        if let Some(digest) = &asset.digest {
            debug!("using GitHub API digest for checksum verification");
            platform_info.checksum = Some(digest.clone());
        }

        let url = match asset.url_api.starts_with(DEFAULT_GITHUB_API_BASE_URL)
            || asset.url_api.starts_with(DEFAULT_GITLAB_API_BASE_URL)
        {
            // check if url is reachable, 404 might indicate a private repo or asset.
            // This is needed, because private repos and assets cannot be downloaded
            // via browser url, therefore a fallback to api_url is needed in such cases.
            true => match HTTP.head(asset.url.clone()).await {
                Ok(_) => asset.url.clone(),
                Err(_) => asset.url_api.clone(),
            },

            // Custom API URLs usually imply that a custom GitHub/GitLab instance is used.
            // Often times such instances do not allow browser URL downloads, e.g. due to
            // upstream company SSOs. Therefore, using the api_url for downloading is the safer approach.
            false => {
                debug!(
                    "Since the tool resides on a custom GitHub/GitLab API ({:?}), the asset download will be performed using the given API instead of browser URL download",
                    asset.url_api
                );
                asset.url_api.clone()
            }
        };

        let headers = if self.is_gitlab() {
            gitlab::get_headers(&url)
        } else {
            github::get_headers(&url)
        };

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file_with_headers(url, &file_path, &headers, Some(ctx.pr.as_ref()))
            .await?;

        // Verify and install
        verify_artifact(tv, &file_path, opts, Some(ctx.pr.as_ref()))?;
        self.verify_checksum(ctx, tv, &file_path)?;

        // Verify attestations or SLSA (check attestations first, fall back to SLSA)
        self.verify_attestations_or_slsa(ctx, tv, &file_path)
            .await?;

        install_artifact(tv, &file_path, opts, Some(ctx.pr.as_ref()))?;

        if let Some(bins) = self.get_filter_bins(tv) {
            self.create_symlink_bin_dir(tv, bins)?;
        }

        Ok(())
    }

    /// Discovers bin paths in the installation directory
    fn discover_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<std::path::PathBuf>> {
        let opts = tv.request.options();
        if let Some(bin_path_template) =
            lookup_platform_key(&opts, "bin_path").or_else(|| opts.get("bin_path").cloned())
        {
            let bin_path = template_string(&bin_path_template, tv);
            return Ok(vec![tv.install_path().join(&bin_path)]);
        }

        let bin_path = tv.install_path().join("bin");
        if bin_path.exists() {
            return Ok(vec![bin_path]);
        }

        // Check if the root directory contains an executable file
        // If so, use the root directory as a bin path
        if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && file::is_executable(&path) {
                    return Ok(vec![tv.install_path()]);
                }
            }
        }

        // Look for bin directory or executables in subdirectories (for extracted archives)
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir(tv.install_path()) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // Check for {subdir}/bin
                    let sub_bin_path = path.join("bin");
                    if sub_bin_path.exists() {
                        paths.push(sub_bin_path);
                    } else {
                        // Check for executables directly in subdir (e.g., tusd_darwin_arm64/tusd)
                        if let Ok(sub_entries) = std::fs::read_dir(&path) {
                            for sub_entry in sub_entries.flatten() {
                                let sub_path = sub_entry.path();
                                if sub_path.is_file() && file::is_executable(&sub_path) {
                                    paths.push(path.clone());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        if paths.is_empty() {
            Ok(vec![tv.install_path()])
        } else {
            Ok(paths)
        }
    }

    /// Resolves the asset URL using either explicit patterns or auto-detection.
    /// Delegates to resolve_asset_url_for_target with the current platform.
    async fn resolve_asset_url(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
    ) -> Result<ReleaseAsset> {
        let current_platform = PlatformTarget::from_current();
        self.resolve_asset_url_for_target(tv, opts, repo, api_url, &current_platform)
            .await
    }

    /// Resolves asset URL for a specific target platform (for cross-platform lockfile generation)
    async fn resolve_asset_url_for_target(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        // Check for direct platform-specific URLs first
        if let Some(direct_url) = lookup_platform_key_for_target(opts, "url", target) {
            return Ok(ReleaseAsset {
                name: get_filename_from_url(&direct_url),
                url: direct_url.clone(),
                url_api: direct_url.clone(),
                digest: None, // Direct URLs don't have API digest
            });
        }

        let version = &tv.version;
        let version_prefix = opts.get("version_prefix").map(|s| s.as_str());
        if self.is_gitlab() {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_gitlab_asset_url_for_target(
                    tv, opts, repo, api_url, &candidate, target,
                )
                .await
            })
            .await
        } else {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_github_asset_url_for_target(
                    tv, opts, repo, api_url, &candidate, target,
                )
                .await
            })
            .await
        }
    }

    /// Resolves GitHub asset URL for a specific target platform
    async fn resolve_github_asset_url_for_target(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        let release = github::get_release_for_url(api_url, repo, version).await?;
        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();

        // Build asset list with URLs for checksum fetching
        let assets_with_urls: Vec<Asset> = release
            .assets
            .iter()
            .map(|a| Asset::new(&a.name, &a.browser_download_url))
            .collect();

        // Try explicit pattern first
        if let Some(pattern) = lookup_platform_key_for_target(opts, "asset_pattern", target)
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern for the target platform
            let templated_pattern = template_string_for_target(&pattern, tv, target);

            let asset = release
                .assets
                .into_iter()
                .find(|a| self.matches_pattern(&a.name, &templated_pattern))
                .ok_or_else(|| {
                    eyre::eyre!(
                        "No matching asset found for pattern: {}\nAvailable assets: {}",
                        templated_pattern,
                        Self::format_asset_list(available_assets.iter())
                    )
                })?;

            // Try to get checksum from API digest or fetch from release assets
            let digest = if asset.digest.is_some() {
                asset.digest
            } else {
                self.try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
                    .await
            };

            return Ok(ReleaseAsset {
                name: asset.name,
                url: asset.browser_download_url,
                url_api: asset.url,
                digest,
            });
        }

        // Fall back to auto-detection for target platform
        let asset_name = asset_matcher::detect_asset_for_target(&available_assets, target)?;
        let asset = self
            .find_asset_case_insensitive(&release.assets, &asset_name, |a| &a.name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
                    Self::format_asset_list(available_assets.iter())
                )
            })?;

        // Try to get checksum from API digest or fetch from release assets
        let digest = if asset.digest.is_some() {
            asset.digest.clone()
        } else {
            self.try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
                .await
        };

        Ok(ReleaseAsset {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            url_api: asset.url.clone(),
            digest,
        })
    }

    /// Resolves GitLab asset URL for a specific target platform
    async fn resolve_gitlab_asset_url_for_target(
        &self,
        tv: &ToolVersion,
        opts: &ToolVersionOptions,
        repo: &str,
        api_url: &str,
        version: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        let release = gitlab::get_release_for_url(api_url, repo, version).await?;
        let available_assets: Vec<String> = release
            .assets
            .links
            .iter()
            .map(|a| a.name.clone())
            .collect();

        // Build asset list with URLs for checksum fetching
        let assets_with_urls: Vec<Asset> = release
            .assets
            .links
            .iter()
            .map(|a| Asset::new(&a.name, &a.direct_asset_url))
            .collect();

        // Try explicit pattern first
        if let Some(pattern) = lookup_platform_key_for_target(opts, "asset_pattern", target)
            .or_else(|| opts.get("asset_pattern").cloned())
        {
            // Template the pattern for the target platform
            let templated_pattern = template_string_for_target(&pattern, tv, target);

            let asset = release
                .assets
                .links
                .into_iter()
                .find(|a| self.matches_pattern(&a.name, &templated_pattern))
                .ok_or_else(|| {
                    eyre::eyre!(
                        "No matching asset found for pattern: {}\nAvailable assets: {}",
                        templated_pattern,
                        Self::format_asset_list(available_assets.iter())
                    )
                })?;

            // GitLab doesn't provide digests, so try fetching from release assets
            let digest = self
                .try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
                .await;

            return Ok(ReleaseAsset {
                name: asset.name,
                url: asset.direct_asset_url.clone(),
                url_api: asset.url,
                digest,
            });
        }

        // Fall back to auto-detection for target platform
        let asset_name = asset_matcher::detect_asset_for_target(&available_assets, target)?;
        let asset = self
            .find_asset_case_insensitive(&release.assets.links, &asset_name, |a| &a.name)
            .ok_or_else(|| {
                eyre::eyre!(
                    "Auto-detected asset not found: {}\nAvailable assets: {}",
                    asset_name,
                    Self::format_asset_list(available_assets.iter())
                )
            })?;

        // GitLab doesn't provide digests, so try fetching from release assets
        let digest = self
            .try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
            .await;

        Ok(ReleaseAsset {
            name: asset.name.clone(),
            url: asset.direct_asset_url.clone(),
            url_api: asset.url.clone(),
            digest,
        })
    }

    fn find_asset_case_insensitive<'a, T>(
        &self,
        assets: &'a [T],
        target_name: &str,
        get_name: impl Fn(&T) -> &str,
    ) -> Option<&'a T> {
        // First try exact match, then case-insensitive
        assets
            .iter()
            .find(|a| get_name(a) == target_name)
            .or_else(|| {
                let target_lower = target_name.to_lowercase();
                assets
                    .iter()
                    .find(|a| get_name(a).to_lowercase() == target_lower)
            })
    }

    fn matches_pattern(&self, asset_name: &str, pattern: &str) -> bool {
        // Simple pattern matching - convert glob-like pattern to regex
        let regex_pattern = pattern
            .replace(".", "\\.")
            .replace("*", ".*")
            .replace("?", ".");

        if let Ok(re) = Regex::new(&format!("^{regex_pattern}$")) {
            re.is_match(asset_name)
        } else {
            // Fallback to simple contains check
            asset_name.contains(pattern)
        }
    }

    fn strip_version_prefix(&self, tag_name: &str) -> String {
        let opts = self.ba.opts();

        // If a custom version_prefix is configured, strip it first
        if let Some(prefix) = opts.get("version_prefix")
            && let Some(stripped) = tag_name.strip_prefix(prefix)
        {
            return stripped.to_string();
        }

        // Fall back to stripping 'v' prefix
        if tag_name.starts_with('v') {
            tag_name.trim_start_matches('v').to_string()
        } else {
            tag_name.to_string()
        }
    }

    /// Tries to fetch a checksum for an asset from release checksum files.
    ///
    /// This method looks for checksum files (SHA256SUMS, *.sha256, etc.) in the release
    /// assets and attempts to extract the checksum for the target asset.
    ///
    /// Returns the checksum in "sha256:hash" format if found, None otherwise.
    async fn try_fetch_checksum_from_assets(
        &self,
        assets: &[Asset],
        asset_name: &str,
    ) -> Option<String> {
        let fetcher = ChecksumFetcher::new(assets);
        match fetcher.fetch_checksum_for(asset_name).await {
            Some(result) => {
                debug!(
                    "Found checksum for {} from {}: {}",
                    asset_name,
                    result.source_file,
                    result.to_string_formatted()
                );
                Some(result.to_string_formatted())
            }
            None => {
                trace!("No checksum file found for {}", asset_name);
                None
            }
        }
    }

    fn get_filter_bins(&self, tv: &ToolVersion) -> Option<Vec<String>> {
        let opts = tv.request.options();
        let filter_bins = lookup_platform_key(&opts, "filter_bins")
            .or_else(|| opts.get("filter_bins").cloned())?;

        Some(
            filter_bins
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        )
    }

    /// Creates a `.mise-bins` directory with symlinks only to the binaries specified in filter_bins.
    fn create_symlink_bin_dir(&self, tv: &ToolVersion, bins: Vec<String>) -> Result<()> {
        let symlink_dir = tv.install_path().join(".mise-bins");
        file::create_dir_all(&symlink_dir)?;

        // Find where the actual binaries are
        let install_path = tv.install_path();
        let bin_paths = self.discover_bin_paths(tv)?;

        // Collect all possible source directories (install root + discovered bin paths)
        let mut src_dirs = bin_paths;
        if !src_dirs.contains(&install_path) {
            src_dirs.push(install_path);
        }

        for bin_name in bins {
            // Find the binary in any of the source directories
            let mut found = false;
            for dir in &src_dirs {
                let src = dir.join(&bin_name);
                if src.exists() {
                    let dst = symlink_dir.join(&bin_name);
                    if !dst.exists() {
                        file::make_symlink_or_copy(&src, &dst)?;
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                warn!(
                    "Could not find binary '{}' in install directories. Available paths: {:?}",
                    bin_name, src_dirs
                );
            }
        }
        Ok(())
    }

    /// Verify artifact using GitHub attestations or SLSA provenance.
    /// Tries attestations first, falls back to SLSA if no attestations found.
    /// If verification is attempted and fails, it's a hard error.
    async fn verify_attestations_or_slsa(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
    ) -> Result<()> {
        let settings = Settings::get();

        // Only verify for GitHub repos (not GitLab)
        if self.is_gitlab() {
            return Ok(());
        }

        // Try GitHub attestations first (if enabled globally and for github backend)
        if settings.github_attestations && settings.github.github_attestations {
            match self
                .try_verify_github_attestations(ctx, tv, file_path)
                .await
            {
                Ok(true) => return Ok(()), // Verified successfully
                Ok(false) => {
                    // Attestations exist but verification failed - hard error
                    return Err(eyre::eyre!(
                        "GitHub attestations verification failed for {tv}"
                    ));
                }
                Err(VerificationStatus::NoAttestations) => {
                    // No attestations - fall through to try SLSA
                    debug!("No GitHub attestations found for {tv}, trying SLSA");
                }
                Err(VerificationStatus::Error(e)) => {
                    // Error during verification - hard error
                    return Err(eyre::eyre!(
                        "GitHub attestations verification error for {tv}: {e}"
                    ));
                }
            }
        }

        // Fall back to SLSA provenance (if enabled globally and for github backend)
        if settings.slsa && settings.github.slsa {
            match self.try_verify_slsa(ctx, tv, file_path).await {
                Ok(true) => return Ok(()), // Verified successfully
                Ok(false) => {
                    // Provenance exists but verification failed - hard error
                    return Err(eyre::eyre!("SLSA provenance verification failed for {tv}"));
                }
                Err(VerificationStatus::NoAttestations) => {
                    // No provenance found - this is fine
                    debug!("No SLSA provenance found for {tv}");
                }
                Err(VerificationStatus::Error(e)) => {
                    // Error during verification - hard error
                    return Err(eyre::eyre!("SLSA verification error for {tv}: {e}"));
                }
            }
        }

        Ok(())
    }

    /// Try to verify GitHub attestations. Returns:
    /// - Ok(true) if attestations exist and verified successfully
    /// - Ok(false) if attestations exist but verification failed
    /// - Err(NoAttestations) if no attestations found
    /// - Err(Error) if an error occurred during verification
    async fn try_verify_github_attestations(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
    ) -> std::result::Result<bool, VerificationStatus> {
        ctx.pr.set_message("verify GitHub attestations".to_string());

        // Parse owner/repo from the repo string
        let repo = self.repo();
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(VerificationStatus::Error(format!(
                "Invalid repo format: {repo}"
            )));
        }
        let (owner, repo_name) = (parts[0], parts[1]);

        match sigstore_verification::verify_github_attestation(
            file_path, owner, repo_name, None, // No token - use public API
            None, // We don't know the expected workflow
        )
        .await
        {
            Ok(verified) => {
                if verified {
                    debug!("GitHub attestations verified successfully for {tv}");
                }
                Ok(verified)
            }
            Err(sigstore_verification::AttestationError::NoAttestations) => {
                Err(VerificationStatus::NoAttestations)
            }
            Err(e) => Err(VerificationStatus::Error(e.to_string())),
        }
    }

    /// Try to verify SLSA provenance. Returns:
    /// - Ok(true) if provenance exists and verified successfully
    /// - Ok(false) if provenance exists but verification failed
    /// - Err(NoAttestations) if no provenance found
    /// - Err(Error) if an error occurred during verification
    async fn try_verify_slsa(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
    ) -> std::result::Result<bool, VerificationStatus> {
        ctx.pr.set_message("verify SLSA provenance".to_string());

        // Get the release to find provenance assets
        let repo = self.repo();
        let opts = tv.request.options();
        let api_url = self.get_api_url(&opts);
        let version = &tv.version;

        // Try to get the release (with version prefix support)
        let version_prefix = opts.get("version_prefix").map(|s| s.as_str());
        let release = match try_with_v_prefix(version, version_prefix, |candidate| {
            let api_url = api_url.clone();
            let repo = repo.clone();
            async move { github::get_release_for_url(&api_url, &repo, &candidate).await }
        })
        .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(VerificationStatus::Error(format!(
                    "Failed to get release: {e}"
                )));
            }
        };

        // Find provenance assets in the release
        let provenance_asset = release.assets.iter().find(|a| {
            let name = a.name.to_lowercase();
            name.contains(".intoto.jsonl")
                || name.contains("provenance")
                || name.ends_with(".attestation")
        });

        let provenance_asset = match provenance_asset {
            Some(a) => a,
            None => return Err(VerificationStatus::NoAttestations),
        };

        // Download the provenance file
        let download_dir = tv.download_path();
        let provenance_path = download_dir.join(&provenance_asset.name);

        ctx.pr
            .set_message(format!("download {}", provenance_asset.name));
        if let Err(e) = HTTP
            .download_file(
                &provenance_asset.browser_download_url,
                &provenance_path,
                Some(ctx.pr.as_ref()),
            )
            .await
        {
            return Err(VerificationStatus::Error(format!(
                "Failed to download provenance: {e}"
            )));
        }

        ctx.pr.set_message("verify SLSA provenance".to_string());

        // Verify the provenance
        match sigstore_verification::verify_slsa_provenance(
            file_path,
            &provenance_path,
            1, // Minimum SLSA level
        )
        .await
        {
            Ok(verified) => {
                if verified {
                    debug!("SLSA provenance verified successfully for {tv}");
                }
                Ok(verified)
            }
            Err(e) => Err(VerificationStatus::Error(e.to_string())),
        }
    }
}

/// Templates a string pattern with version and target platform values
fn template_string_for_target(template: &str, tv: &ToolVersion, target: &PlatformTarget) -> String {
    let version = &tv.version;
    let os = target.os_name();
    let arch = target.arch_name();

    // Map to common naming conventions
    let darwin_os = if os == "macos" { "darwin" } else { os };
    let amd64_arch = match arch {
        "x64" => "amd64",
        _ => arch, // arm64 stays as "arm64" in amd64/arm64 convention
    };
    let x86_64_arch = match arch {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        _ => arch,
    };
    // GNU-style arch: x64 -> x86_64, arm64 stays arm64 (used by opam, etc.)
    let gnu_arch = match arch {
        "x64" => "x86_64",
        _ => arch,
    };

    template
        .replace("{version}", version)
        .replace("{os}", os)
        .replace("{arch}", arch)
        // Common aliases
        .replace("{darwin_os}", darwin_os)
        .replace("{amd64_arch}", amd64_arch)
        .replace("{x86_64_arch}", x86_64_arch)
        .replace("{gnu_arch}", gnu_arch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;

    fn create_test_backend() -> UnifiedGitBackend {
        UnifiedGitBackend::from_arg(BackendArg::new(
            "github".to_string(),
            Some("github:test/repo".to_string()),
        ))
    }

    #[test]
    fn test_pattern_matching() {
        let backend = create_test_backend();
        assert!(backend.matches_pattern("test-v1.0.0.zip", "test-*"));
        assert!(!backend.matches_pattern("other-v1.0.0.zip", "test-*"));
    }

    #[test]
    fn test_version_prefix_functionality() {
        let mut backend = create_test_backend();

        // Test with no version prefix configured
        assert_eq!(backend.strip_version_prefix("v1.0.0"), "1.0.0");
        assert_eq!(backend.strip_version_prefix("1.0.0"), "1.0.0");

        // Test with custom version prefix
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("version_prefix".to_string(), "release-".to_string());
        backend.ba = Arc::new(BackendArg::new_raw(
            "test".to_string(),
            Some("github:test/repo".to_string()),
            "test".to_string(),
            Some(opts),
        ));

        assert_eq!(backend.strip_version_prefix("release-1.0.0"), "1.0.0");
        assert_eq!(backend.strip_version_prefix("1.0.0"), "1.0.0");
    }

    #[test]
    fn test_find_asset_case_insensitive() {
        let backend = create_test_backend();

        // Mock asset structs for testing
        struct TestAsset {
            name: String,
        }

        let assets = vec![
            TestAsset {
                name: "tool-1.0.0-linux-x86_64.tar.gz".to_string(),
            },
            TestAsset {
                name: "tool-1.0.0-Darwin-x86_64.tar.gz".to_string(),
            },
            TestAsset {
                name: "tool-1.0.0-Windows-x86_64.zip".to_string(),
            },
        ];

        // Test exact match (should find immediately)
        let result =
            backend.find_asset_case_insensitive(&assets, "tool-1.0.0-linux-x86_64.tar.gz", |a| {
                &a.name
            });
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-linux-x86_64.tar.gz");

        // Test case-insensitive match for Darwin (capital D)
        let result = backend.find_asset_case_insensitive(
            &assets,
            "tool-1.0.0-darwin-x86_64.tar.gz", // lowercase 'd'
            |a| &a.name,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-Darwin-x86_64.tar.gz");

        // Test case-insensitive match for Windows (capital W)
        let result = backend.find_asset_case_insensitive(
            &assets,
            "tool-1.0.0-windows-x86_64.zip", // lowercase 'w'
            |a| &a.name,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "tool-1.0.0-Windows-x86_64.zip");

        // Test no match
        let result =
            backend.find_asset_case_insensitive(&assets, "nonexistent-asset.tar.gz", |a| &a.name);
        assert!(result.is_none());
    }
}
