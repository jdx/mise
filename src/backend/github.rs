use crate::backend::VersionInfo;
use crate::backend::asset_matcher::{self, Asset, AssetPicker, ChecksumFetcher};
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{
    get_filename_from_url, install_artifact, lookup_platform_key, lookup_with_fallback,
    template_string, try_with_v_prefix, try_with_v_prefix_and_repo, verify_artifact,
};
use crate::backend::{
    MISE_BINS_DIR, SecurityFeature, backend_arg_matches_registry_backend,
    runtime_path_for_install_path,
};
use crate::cli::args::{BackendArg, ToolVersionType};
use crate::config::{Config, Settings};
use crate::file;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::{PlatformInfo, ProvenanceType};
use crate::toolset::ToolVersionOptions;
use crate::toolset::{ToolRequest, ToolVersion};
use crate::{backend::Backend, forgejo, github, gitlab};
use async_trait::async_trait;
use eyre::Result;
use regex::Regex;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::sync::Arc;
use xx::regex;

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
const DEFAULT_FORGEJO_API_BASE_URL: &str = "https://codeberg.org/api/v1";

#[derive(Debug, Clone, Copy)]
struct GitBackendOptions<'a> {
    values: BackendOptions<'a>,
    default_api_url: &'static str,
}

impl<'a> GitBackendOptions<'a> {
    fn new(raw: &'a ToolVersionOptions, default_api_url: &'static str) -> Self {
        Self {
            values: BackendOptions::new(raw),
            default_api_url,
        }
    }

    fn raw(&self) -> &'a ToolVersionOptions {
        self.values.raw()
    }

    fn api_url(&self) -> String {
        self.values
            .str("api_url")
            .unwrap_or(self.default_api_url)
            .to_string()
    }

    fn version_prefix(&self) -> Option<&'a str> {
        self.values.str("version_prefix")
    }

    fn checksum(&self) -> Option<String> {
        self.values.platform_string("checksum")
    }

    fn bin_path(&self) -> Option<String> {
        self.values.platform_string("bin_path")
    }

    fn asset_pattern_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values
            .platform_string_for_target("asset_pattern", target)
    }

    fn direct_url_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values
            .platform_string_for_target_without_base("url", target)
    }

    fn no_app_for_target(&self, target: &PlatformTarget) -> bool {
        self.values.platform_bool_for_target("no_app", target)
    }

    fn filter_bins(&self) -> Option<Vec<String>> {
        self.values
            .platform_string("filter_bins")
            .map(|filter_bins| {
                filter_bins
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
    }

    /// Substring an asset name must contain to remain a candidate, applied as a
    /// pre-filter before platform autodetection (ported from the ubi backend).
    fn matching(&self) -> Option<&'a str> {
        self.values.str("matching")
    }

    /// Regex an asset name must match to remain a candidate, applied as a
    /// pre-filter before platform autodetection (ported from the ubi backend).
    fn matching_regex(&self) -> Option<&'a str> {
        self.values.str("matching_regex")
    }

    /// `matching`/`matching_regex` for *provenance* selection, suppressed when
    /// `asset_pattern` is set for this target.
    ///
    /// `asset_pattern` replaces autodetection and selects the binary directly,
    /// ignoring the matching pre-filter (see the asset-selection call sites). Its
    /// provenance must therefore align with that asset by platform alone:
    /// re-applying `matching` here could narrow provenance to a *different* binary
    /// than `asset_pattern` picked. Suppressing it also keeps an invalid
    /// `matching_regex` — which is never validated on the asset_pattern path,
    /// since that path skips `match_by_auto_detection` — from reaching the
    /// provenance picker and silently returning no provenance (a verification
    /// downgrade). When `asset_pattern` is unset this is just `matching`/
    /// `matching_regex` unchanged.
    fn matching_for_provenance(
        &self,
        target: &PlatformTarget,
    ) -> (Option<&'a str>, Option<&'a str>) {
        if self.asset_pattern_for_target(target).is_some() {
            (None, None)
        } else {
            (self.matching(), self.matching_regex())
        }
    }

    fn lockfile_options(&self, target: &PlatformTarget) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        if self.api_url() != self.default_api_url {
            result.insert("api_url".to_string(), self.api_url());
        }
        if let Some(value) = self.version_prefix() {
            result.insert("version_prefix".to_string(), value.to_string());
        }
        if let Some(value) = self.asset_pattern_for_target(target) {
            result.insert("asset_pattern".to_string(), value);
        }
        if let Some(value) = self.direct_url_for_target(target) {
            result.insert("url".to_string(), value);
        }
        if self.no_app_for_target(target) {
            result.insert("no_app".to_string(), "true".to_string());
        }
        if let Some(value) = self.matching() {
            result.insert("matching".to_string(), value.to_string());
        }
        if let Some(value) = self.matching_regex() {
            result.insert("matching_regex".to_string(), value.to_string());
        }
        result
    }
}

/// GitHub artifact attestations are only served by https://api.github.com. GHE
/// Server doesn't implement the attestations endpoint, so any verification
/// attempt against a custom api_url will fail. Callers gate on this so users
/// don't have to disable `MISE_GITHUB_ATTESTATIONS` globally for GHE tools.
fn attestations_supported(api_url: &str) -> bool {
    api_url.trim_end_matches('/') == DEFAULT_GITHUB_API_BASE_URL
}

/// Status returned from verification attempts
enum VerificationStatus {
    /// No attestations or provenance found (not an error, tool may not have them)
    NoAttestations,
    /// A remote API or download failed while checking provenance.
    ApiError(String),
    /// An error occurred during verification
    Error(String),
}

/// Check if an SLSA verification error indicates a format/parsing issue rather than
/// an actual verification failure. Some provenance files (e.g., BuildKit raw provenance)
/// exist but aren't in a sigstore-verifiable format.
///
/// `Sigstore(msg)` covers errors that originated in `sigstore-verify` itself
/// (e.g. `missing field 'verificationMaterial'` when the file is a legacy
/// cosign v1 bundle that the modern bundle deserializer rejects). Those are
/// format mismatches, not signature failures, so we let the caller fall
/// back to alternate verification paths.
fn is_slsa_format_issue(e: &crate::github::sigstore::AttestationError) -> bool {
    match e {
        crate::github::sigstore::AttestationError::NoAttestations => true,
        crate::github::sigstore::AttestationError::UnsupportedFormat(_) => true,
        crate::github::sigstore::AttestationError::Verification(msg)
        | crate::github::sigstore::AttestationError::Sigstore(msg) => {
            msg.contains("does not contain valid attestations")
                || msg.contains("No certificate found")
                || msg.contains("neither DSSE envelope nor message signature")
                || msg.contains("missing field")
                || msg.contains("not a sigstore or cosign bundle")
                || msg.contains("not a JSON DSSE envelope")
        }
        _ => false,
    }
}

/// Returns install-time-only option keys for GitHub/GitLab backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "asset_pattern".into(),
        "url".into(),
        "version_prefix".into(),
        "no_app".into(),
        "matching".into(),
        "matching_regex".into(),
    ]
}

#[async_trait]
impl Backend for UnifiedGitBackend {
    fn get_type(&self) -> BackendType {
        if self.is_gitlab() {
            BackendType::Gitlab
        } else if self.is_forgejo() {
            BackendType::Forgejo
        } else {
            BackendType::Github
        }
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn security_info(&self) -> Vec<SecurityFeature> {
        // Only report security features for GitHub (not GitLab yet)
        if self.is_gitlab() || self.is_forgejo() {
            return vec![];
        }

        let mut features = vec![];

        // Get the latest release to check for security assets
        let repo = self.ba.tool_name();
        let raw_opts = self.ba.opts();
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();

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

        // Check for GitHub artifact Attestations (assets with .sigstore.json or .sigstore extension)
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

    fn remote_version_listing_tool_option_keys(&self) -> &'static [&'static str] {
        &["api_url", "version_prefix"]
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let repo = self.ba.tool_name();
        let id = self.ba.to_string();
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();
        let version_prefix = opts.version_prefix();

        // Derive web URL base from API URL for enterprise support
        let web_url_base = if self.is_gitlab() {
            if api_url == DEFAULT_GITLAB_API_BASE_URL {
                format!("https://gitlab.com/{}", repo)
            } else {
                // Enterprise GitLab - derive web URL from API URL
                let web_url = api_url.replace("/api/v4", "");
                format!("{}/{}", web_url, repo)
            }
        } else if self.is_forgejo() {
            if api_url == DEFAULT_FORGEJO_API_BASE_URL {
                format!("https://codeberg.org/{}", repo)
            } else {
                // Enterprise Forgejo - derive web URL from API URL
                let web_url = api_url.replace("/api/v1", "");
                format!("{}/{}", web_url, repo)
            }
        } else if api_url == DEFAULT_GITHUB_API_BASE_URL {
            format!("https://github.com/{}", repo)
        } else {
            // Enterprise GitHub - derive web URL from API URL
            let web_url = api_url.replace("/api/v3", "").replace("api.", "");
            format!("{}/{}", web_url, repo)
        };

        // Get releases with full metadata from GitHub, GitLab, or Forgejo
        let raw_versions: Vec<VersionInfo> = if self.is_gitlab() {
            gitlab::list_releases_from_url(api_url.as_str(), &repo)
                .await?
                .into_iter()
                .filter(|r| version_prefix.is_none_or(|p| r.tag_name.starts_with(p)))
                .map(|r| VersionInfo {
                    version: self.strip_version_prefix(&r.tag_name, &opts),
                    created_at: r.released_at,
                    release_url: Some(format!("{}/-/releases/{}", web_url_base, r.tag_name)),
                    ..Default::default()
                })
                .collect()
        } else if self.is_forgejo() {
            forgejo::list_releases_including_prereleases_from_url(api_url.as_str(), &repo)
                .await?
                .into_iter()
                .filter(|r| version_prefix.is_none_or(|p| r.tag_name.starts_with(p)))
                .map(|r| VersionInfo {
                    version: self.strip_version_prefix(&r.tag_name, &opts),
                    created_at: Some(r.created_at),
                    release_url: Some(format!("{}/releases/tag/{}", web_url_base, r.tag_name)),
                    prerelease: r.prerelease,
                    ..Default::default()
                })
                .collect()
        } else {
            // Always fetch the pre-release superset and stamp `prerelease` on
            // each entry. The shared remote-versions cache stores the superset
            // so flipping the `prerelease` tool option (e.g. via a project
            // override) is correct without invalidating the cache; the read
            // path filters on `prerelease` according to the current opts.
            github::list_releases_including_prereleases_from_url(api_url.as_str(), &repo)
                .await?
                .into_iter()
                .filter(|r| version_prefix.is_none_or(|p| r.tag_name.starts_with(p)))
                .map(|r| VersionInfo {
                    version: self.strip_version_prefix(&r.tag_name, &opts),
                    created_at: Some(r.created_at),
                    release_url: Some(format!("{}/releases/tag/{}", web_url_base, r.tag_name)),
                    prerelease: r.prerelease,
                    ..Default::default()
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

    async fn latest_stable_version_info(
        &self,
        config: &Arc<Config>,
    ) -> eyre::Result<Option<VersionInfo>> {
        if Settings::get().offline() {
            trace!("Skipping latest stable version due to offline mode");
            return Ok(None);
        }

        let repo = self.ba.tool_name();
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();
        let version_prefix = opts.version_prefix();

        // When `prerelease = true`, skip the `/releases/latest` shortcut
        // (which returns whichever release the repo owner marked as "Latest",
        // defaulting to the newest non-prerelease). Returning `None` lets the
        // trait's `latest_version` fall through to `latest_version_for_query`,
        // which resolves against the full list — now including pre-releases.
        if self.include_prereleases(opts.raw()) {
            return Ok(None);
        }

        let latest_release = if self.is_gitlab() {
            // GitLab doesn't have a "latest" endpoint
            return Ok(None);
        } else if self.is_forgejo() {
            match forgejo::get_release_for_url(&api_url, &repo, "latest").await {
                Ok(r) => Some((r.tag_name, r.created_at, r.prerelease)),
                Err(e) => {
                    debug!("Failed to fetch latest Forgejo release for {repo}: {e}");
                    None
                }
            }
        } else {
            match self
                .get_github_release_for_url(&api_url, &repo, "latest")
                .await
            {
                Ok(r) => Some((r.tag_name, r.created_at, r.prerelease)),
                Err(e) => {
                    debug!("Failed to fetch latest GitHub release for {repo}: {e}");
                    None
                }
            }
        };

        Ok(latest_release
            .filter(|(tag, _, _)| version_prefix.is_none_or(|p| tag.starts_with(p)))
            .map(|(tag, created_at, prerelease)| VersionInfo {
                version: self.strip_version_prefix(&tag, &opts),
                created_at: Some(created_at),
                prerelease,
                ..Default::default()
            }))
    }

    async fn resolve_exact_version(
        &self,
        config: &Arc<Config>,
        version: &str,
    ) -> eyre::Result<Option<String>> {
        if Settings::get().offline() || self.is_gitlab() || self.is_forgejo() {
            return Ok(None);
        }

        let repo = self.repo();
        let raw_opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();
        let version_prefix = opts.version_prefix();

        let use_versions_host = self.use_versions_host_for_github_metadata();
        match try_with_v_prefix_and_repo(version, version_prefix, Some(&repo), |candidate| {
            let api_url = api_url.clone();
            let repo = repo.clone();
            async move {
                github::get_release_for_url_with_versions_host(
                    &api_url,
                    &repo,
                    &candidate,
                    use_versions_host,
                )
                .await
            }
        })
        .await
        {
            Ok(release) => Ok(Some(self.strip_version_prefix(&release.tag_name, &opts))),
            Err(e) => {
                debug!("Failed to resolve exact GitHub release for {repo}@{version}: {e}");
                Ok(None)
            }
        }
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        let repo = self.repo();
        let raw_opts = ctx.config.get_tool_opts_with_overrides(&self.ba).await?;
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();

        // Validate `matching_regex` up front, before the cached-URL branch below.
        // Reusing a cached lockfile URL skips binary selection (the path that
        // normally hard-errors on a bad pattern), so without this an invalid
        // regex would reach the provenance picker, return `None`, and silently
        // skip SLSA verification rather than failing closed.
        //
        // Skip this when `asset_pattern` is set: it supersedes `matching`/
        // `matching_regex` for both binary selection and provenance (see
        // `matching_for_provenance` and `resolve_*_asset_url_for_target`), so the
        // regex is never consulted and an invalid one is irrelevant. mise doesn't
        // hard-fail on options it won't act on — `url` short-circuits before
        // `asset_pattern` is ever templated (resolve_asset_url_for_target), an
        // ignored hook `shell` is dropped with a warning (src/hooks.rs), and
        // unknown tool-option keys are silently ignored. Validating here would be
        // the lone exception that rejects a superseded option.
        if opts
            .asset_pattern_for_target(&PlatformTarget::from_current())
            .is_none()
        {
            asset_matcher::validate_matching_regex(opts.matching_regex())?;
        }

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
        let raw_opts = tv.request.options();
        let opts = self.options(&raw_opts);
        let mise_bins_dir = tv.install_path().join(MISE_BINS_DIR);
        if opts.filter_bins().is_some() || mise_bins_dir.is_dir() {
            return Ok(vec![tv.runtime_path().join(MISE_BINS_DIR)]);
        }

        Ok(self
            .discover_bin_paths(tv)?
            .into_iter()
            .map(|path| runtime_path_for_install_path(tv, path))
            .collect())
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(self.options(&raw_opts).lockfile_options(target))
    }

    /// Resolve platform-specific lock information for cross-platform lockfile generation.
    /// This fetches release asset metadata including SHA256 digests from GitHub/GitLab API.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let repo = self.repo();
        let raw_opts = tv.request.options();
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();

        // Fail closed on an invalid `matching_regex` instead of writing an empty
        // entry. The `Err` arm below intentionally swallows resolution failures so a
        // platform with no matching asset is skipped rather than failing the whole
        // (best-effort) lock — but `resolve_asset_url_for_target` returns the same
        // `Err` for an invalid regex, which would then be caught and written as a
        // url-less `PlatformInfo::default()`. Returning `Err` here makes the lock
        // orchestration skip the platform (no entry written) instead. Gated on the
        // same `asset_pattern` precedence as everywhere else, so an ignored regex is
        // never validated.
        if opts.asset_pattern_for_target(target).is_none() {
            asset_matcher::validate_matching_regex(opts.matching_regex())?;
        }

        // Resolve asset for the target platform
        let asset = self
            .resolve_asset_url_for_target(tv, &opts, &repo, &api_url, target)
            .await;

        match asset {
            Ok(asset) => {
                // Detect provenance availability from release assets and attestation API
                let mut provenance = if !self.is_gitlab() && !self.is_forgejo() {
                    self.detect_provenance_type(
                        tv,
                        &opts,
                        &repo,
                        &api_url,
                        asset.digest.as_deref(),
                        target,
                    )
                    .await?
                } else {
                    None
                };

                // For the current platform, verify provenance cryptographically at lock time.
                // This ensures the lockfile's provenance entry is backed by actual verification,
                // not just an API query. Cross-platform entries remain detection-only.
                if provenance.is_some() && target.is_current() {
                    match self
                        .verify_provenance_at_lock_time(tv, &opts, &repo, &api_url, &asset)
                        .await
                    {
                        Ok(verified) => {
                            provenance = verified;
                        }
                        Err(e) => {
                            // Clear provenance so install-time verification will run.
                            warn!(
                                "lock-time provenance verification failed for {}, \
                                 will be verified at install time: {e}",
                                self.ba.full()
                            );
                            provenance = None;
                        }
                    }
                }
                Ok(PlatformInfo {
                    url: Some(asset.url),
                    url_api: Some(asset.url_api),
                    checksum: asset.digest,
                    provenance,
                    github_attestations: None,
                    ..Default::default()
                })
            }
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

    fn options<'a>(&self, raw: &'a ToolVersionOptions) -> GitBackendOptions<'a> {
        GitBackendOptions::new(raw, self.default_api_url())
    }

    fn default_api_url(&self) -> &'static str {
        if self.is_gitlab() {
            DEFAULT_GITLAB_API_BASE_URL
        } else if self.is_forgejo() {
            DEFAULT_FORGEJO_API_BASE_URL
        } else {
            DEFAULT_GITHUB_API_BASE_URL
        }
    }

    fn use_versions_host_for_github_metadata(&self) -> bool {
        backend_arg_matches_registry_backend(&self.ba)
    }

    async fn get_github_release_for_url(
        &self,
        api_url: &str,
        repo: &str,
        tag: &str,
    ) -> Result<github::GithubRelease> {
        github::get_release_for_url_with_versions_host(
            api_url,
            repo,
            tag,
            self.use_versions_host_for_github_metadata(),
        )
        .await
    }

    /// Detect what provenance type is available for a release by checking its assets
    /// and querying the GitHub attestation API.
    async fn detect_provenance_type(
        &self,
        tv: &ToolVersion,
        opts: &GitBackendOptions<'_>,
        repo: &str,
        api_url: &str,
        asset_digest: Option<&str>,
        target: &PlatformTarget,
    ) -> Result<Option<ProvenanceType>> {
        let settings = Settings::get();
        let version = &tv.version;
        let version_prefix = opts.version_prefix();

        let use_versions_host = self.use_versions_host_for_github_metadata();
        let release =
            try_with_v_prefix_and_repo(version, version_prefix, Some(repo), |candidate| {
                let api_url = api_url.to_string();
                let repo = repo.to_string();
                async move {
                    github::get_release_for_url_with_versions_host(
                        &api_url,
                        &repo,
                        &candidate,
                        use_versions_host,
                    )
                    .await
                }
            })
            .await
            .ok();
        let Some(release) = release else {
            return Ok(None);
        };

        // Check github-attestations first (higher priority, matching install verification order)
        // Uses the asset digest from the GitHub API to query attestations without downloading
        if settings.github_attestations
            && settings.github.github_attestations
            && attestations_supported(api_url)
            && let Some(digest) = asset_digest
        {
            let parts: Vec<&str> = repo.split('/').collect();
            if parts.len() == 2 {
                let (owner, repo_name) = (parts[0], parts[1]);
                match crate::github::sigstore::detect_attestations(
                    owner,
                    repo_name,
                    api_url,
                    digest,
                    self.use_versions_host_for_github_metadata(),
                )
                .await
                {
                    Ok(true) => return Ok(Some(ProvenanceType::GithubAttestations)),
                    Ok(false) => {}
                    Err(crate::github::sigstore::DetectError::SourceCreation(e)) => {
                        if !settings.provenance_api_failures_fatal
                            && crate::github::sigstore::is_api_failure(&e)
                        {
                            warn!(
                                "failed to create GitHub attestation source for {owner}/{repo_name}, skipping attestation provenance: {e}"
                            );
                        } else {
                            return Err(eyre::eyre!(
                                "failed to create GitHub attestation source for {owner}/{repo_name}: {e}"
                            ));
                        }
                    }
                    Err(crate::github::sigstore::DetectError::Fetch(e)) => {
                        if !settings.provenance_api_failures_fatal
                            && crate::github::sigstore::is_api_failure(&e)
                        {
                            warn!(
                                "GitHub attestation API query failed for {owner}/{repo_name}, skipping attestation provenance: {e}"
                            );
                        } else {
                            return Err(eyre::eyre!(
                                "GitHub attestation API query failed for {owner}/{repo_name}: {e}"
                            ));
                        }
                    }
                }
            }
        }

        // Check for SLSA provenance from release assets using the same platform-aware
        // picker as install-time verification. This ensures we only record SLSA provenance
        // when a matching provenance file exists for the target platform.
        if settings.slsa && settings.github.slsa {
            let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
            // Narrow provenance the same way the binary is narrowed, so a
            // multi-binary release's per-binary provenance files don't
            // cross-verify the wrong digest. Suppressed when `asset_pattern` is
            // set (it selects the binary, ignoring `matching`).
            let (matching, matching_regex) = opts.matching_for_provenance(target);
            let picker = AssetPicker::with_libc(
                target.os_name().to_string(),
                target.arch_name().to_string(),
                target.qualifier().map(|s| s.to_string()),
            )
            .with_matching(matching.unwrap_or_default())
            .with_matching_regex(matching_regex.unwrap_or_default());
            if let Some(provenance_name) = picker.pick_best_provenance(&asset_names) {
                let url = release
                    .assets
                    .iter()
                    .find(|a| a.name == provenance_name)
                    .map(|a| a.browser_download_url.clone());
                return Ok(Some(ProvenanceType::Slsa { url }));
            }
        }

        Ok(None)
    }

    /// Verify provenance at lock time by downloading the artifact to a temp directory
    /// and running cryptographic verification. Only called for the current platform
    /// during `mise lock`.
    async fn verify_provenance_at_lock_time(
        &self,
        tv: &ToolVersion,
        opts: &GitBackendOptions<'_>,
        repo: &str,
        api_url: &str,
        asset: &ReleaseAsset,
    ) -> Result<Option<ProvenanceType>> {
        let tmp_dir = tempfile::tempdir()?;
        let filename = get_filename_from_url(&asset.url);
        let artifact_path = tmp_dir.path().join(&filename);

        info!(
            "downloading artifact for lock-time provenance verification: {}",
            filename
        );

        // Use the API URL with appropriate headers for downloading
        let download_url = if self.is_gitlab() {
            asset.url.clone()
        } else {
            asset.url_api.clone()
        };
        let headers = if self.is_gitlab() {
            gitlab::get_headers(&download_url)
        } else if self.is_forgejo() {
            forgejo::get_headers(&download_url)
        } else {
            github::get_headers(&download_url)?
        };
        HTTP.download_file_with_headers(&download_url, &artifact_path, &headers, None)
            .await?;

        let settings = Settings::get();

        // Try GitHub artifact attestations first (highest priority)
        if settings.github_attestations
            && settings.github.github_attestations
            && attestations_supported(api_url)
        {
            let parts: Vec<&str> = repo.split('/').collect();
            if parts.len() == 2 {
                let (owner, repo_name) = (parts[0], parts[1]);
                match crate::github::sigstore::verify_attestation(
                    &artifact_path,
                    owner,
                    repo_name,
                    None,
                    Some(api_url),
                    self.use_versions_host_for_github_metadata(),
                )
                .await
                {
                    Ok(true) => {
                        debug!("lock-time GitHub attestations verified for {}", repo);
                        return Ok(Some(ProvenanceType::GithubAttestations));
                    }
                    Ok(false) => {
                        return Err(eyre::eyre!(
                            "GitHub artifact attestations verification returned false"
                        ));
                    }
                    Err(crate::github::sigstore::AttestationError::NoAttestations) => {
                        debug!("no GitHub attestations found at lock time, trying SLSA");
                    }
                    Err(e) => {
                        return Err(eyre::eyre!(
                            "GitHub artifact attestations verification failed: {e}"
                        ));
                    }
                }
            }
        }

        // Fall back to SLSA provenance
        if settings.slsa && settings.github.slsa {
            let version = &tv.version;
            let version_prefix = opts.version_prefix();
            let use_versions_host = self.use_versions_host_for_github_metadata();
            let release =
                try_with_v_prefix_and_repo(version, version_prefix, Some(repo), |candidate| {
                    let api_url = api_url.to_string();
                    let repo = repo.to_string();
                    async move {
                        github::get_release_for_url_with_versions_host(
                            &api_url,
                            &repo,
                            &candidate,
                            use_versions_host,
                        )
                        .await
                    }
                })
                .await?;

            let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
            let current_platform = PlatformTarget::from_current();
            // Keep provenance aligned with the matching-selected binary, unless
            // `asset_pattern` is set (it selects the binary, ignoring `matching`).
            let (matching, matching_regex) = opts.matching_for_provenance(&current_platform);
            let picker = AssetPicker::with_libc(
                current_platform.os_name().to_string(),
                current_platform.arch_name().to_string(),
                current_platform.qualifier().map(|s| s.to_string()),
            )
            .with_matching(matching.unwrap_or_default())
            .with_matching_regex(matching_regex.unwrap_or_default());

            if let Some(provenance_name) = picker.pick_best_provenance(&asset_names) {
                let provenance_asset = release
                    .assets
                    .iter()
                    .find(|a| a.name == provenance_name)
                    .expect("provenance asset should exist since we found its name");

                let provenance_path = tmp_dir.path().join(&provenance_asset.name);
                HTTP.download_file(
                    &provenance_asset.browser_download_url,
                    &provenance_path,
                    None,
                )
                .await?;

                let provenance_url = provenance_asset.browser_download_url.clone();
                match crate::github::sigstore::verify_slsa_provenance(
                    &artifact_path,
                    &provenance_path,
                    1u8,
                )
                .await
                {
                    Ok(true) => {
                        debug!("lock-time SLSA provenance verified for {}", repo);
                        return Ok(Some(ProvenanceType::Slsa {
                            url: Some(provenance_url),
                        }));
                    }
                    Ok(false) => {
                        return Err(eyre::eyre!("SLSA provenance verification failed"));
                    }
                    Err(e) => {
                        if crate::github::sigstore::is_slsa_subject_mismatch(&e) {
                            debug!(
                                "lock-time SLSA provenance did not cover downloaded artifact for {}; trying archive content subjects: {e}",
                                repo
                            );
                            match self
                                .try_verify_slsa_archive_contents(
                                    tv,
                                    &artifact_path,
                                    &provenance_path,
                                )
                                .await
                            {
                                Ok(true) => {
                                    debug!(
                                        "lock-time SLSA provenance verified archive contents for {}",
                                        repo
                                    );
                                    return Ok(Some(ProvenanceType::Slsa {
                                        url: Some(provenance_url),
                                    }));
                                }
                                Ok(false) => {
                                    return Err(eyre::eyre!(
                                        "SLSA archive content verification failed"
                                    ));
                                }
                                Err(content_err) => {
                                    return Err(eyre::eyre!(
                                        "SLSA archive content verification error: {content_err}"
                                    ));
                                }
                            }
                        } else if is_slsa_format_issue(&e) {
                            debug!("SLSA provenance file not in verifiable format: {e}");
                        } else {
                            return Err(eyre::eyre!("SLSA verification error: {e}"));
                        }
                    }
                }
            }
        }

        Err(eyre::eyre!(
            "provenance was detected but could not be verified at lock time"
        ))
    }

    fn is_gitlab(&self) -> bool {
        self.ba.backend_type() == BackendType::Gitlab
    }

    fn is_forgejo(&self) -> bool {
        self.ba.backend_type() == BackendType::Forgejo
    }

    fn repo(&self) -> String {
        // Use tool_name() method to properly resolve aliases
        // This ensures that when an alias like "test-edit = github:microsoft/edit" is used,
        // the repository name is correctly extracted as "microsoft/edit"
        self.ba.tool_name()
    }

    fn preferred_asset_name(&self) -> String {
        self.repo()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }

    // Helper to format asset names for error messages
    fn format_asset_list<'a, I>(assets: I) -> String
    where
        I: Iterator<Item = &'a String>,
    {
        assets.cloned().collect::<Vec<_>>().join(", ")
    }

    /// Downloads and installs the asset
    async fn download_and_install(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        asset: &ReleaseAsset,
        opts: &GitBackendOptions<'_>,
    ) -> Result<()> {
        let filename = asset.name.clone();
        let file_path = tv.download_path().join(&filename);

        // Check if we'll verify checksum
        let has_checksum = opts.checksum().is_some();

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
            || asset.url_api.starts_with(DEFAULT_FORGEJO_API_BASE_URL)
        {
            // check if url is reachable, 404 might indicate a private repo or asset.
            // This is needed, because private repos and assets cannot be downloaded
            // via browser url, therefore a fallback to api_url is needed in such cases.
            // Also check Content-Type - if it's text/html, we got a login page (private repo).
            true => match HTTP.head(asset.url.clone()).await {
                Ok(resp) => {
                    let content_type = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if content_type.contains("text/html") {
                        debug!("Browser URL returned HTML (likely auth page), using API URL");
                        asset.url_api.clone()
                    } else {
                        asset.url.clone()
                    }
                }
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
        } else if self.is_forgejo() {
            forgejo::get_headers(&url)
        } else {
            github::get_headers(&url)?
        };

        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file_with_headers(url, &file_path, &headers, Some(ctx.pr.as_ref()))
            .await?;

        // Verify and install
        ctx.pr.next_operation();
        if has_checksum {
            verify_artifact(tv, &file_path, opts.raw(), Some(ctx.pr.as_ref()))?;
        }

        // Check before verify_checksum, which may generate a new checksum from the
        // downloaded file. We only want to skip provenance when the lockfile already
        // had integrity data before this install.
        let platform_key = self.get_platform_key();
        let has_lockfile_integrity = tv
            .lock_platforms
            .get(&platform_key)
            .is_some_and(PlatformInfo::has_checksum_and_verified_provenance);

        self.verify_checksum(ctx, tv, &file_path)?;

        let settings = Settings::get();
        let force_verify = settings.force_provenance_verify();
        if has_lockfile_integrity && !force_verify {
            // Still check that the recorded provenance type's setting is enabled —
            // disabling a verification setting with a provenance-bearing lockfile is a downgrade.
            self.ensure_provenance_setting_enabled(tv, &platform_key)?;
        } else {
            let provenance_result = self
                .verify_attestations_or_slsa(ctx, tv, &file_path)
                .await?;

            // Record provenance verification result in lock_platforms
            if provenance_result.is_some() {
                let platform_info = tv.lock_platforms.entry(platform_key).or_default();
                platform_info.provenance = provenance_result;
                platform_info.github_attestations = None;
            }
        }

        ctx.pr.next_operation();
        install_artifact(tv, &file_path, opts.raw(), Some(ctx.pr.as_ref()))?;

        if let Some(bins) = opts.filter_bins() {
            self.create_symlink_bin_dir(tv, bins)?;
        }

        Ok(())
    }

    /// Discovers bin paths in the installation directory
    fn discover_bin_paths(&self, tv: &ToolVersion) -> Result<Vec<std::path::PathBuf>> {
        let raw_opts = tv.request.options();
        let opts = self.options(&raw_opts);
        if let Some(bin_path_template) = opts.bin_path() {
            let bin_path = template_string(&bin_path_template, tv);
            return Ok(vec![tv.install_path().join(&bin_path)]);
        }

        let bin_path = tv.install_path().join("bin");
        if bin_path.exists() {
            return Ok(vec![bin_path]);
        }

        // Check for macOS .app bundle structure at root (happens when auto-strip removed .app wrapper)
        // Look for Contents/MacOS/ which indicates a stripped .app bundle
        let contents_macos = tv.install_path().join("Contents").join("MacOS");
        if contents_macos.is_dir() {
            return Ok(vec![contents_macos]);
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
                    // Check for macOS .app bundles (e.g., SwiftFormat.app/Contents/MacOS/)
                    let path_str = path.file_name().unwrap_or_default().to_string_lossy();
                    if path_str.ends_with(".app") {
                        let macos_dir = path.join("Contents").join("MacOS");
                        if macos_dir.is_dir() {
                            paths.push(macos_dir);
                            continue;
                        }
                    }
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
        opts: &GitBackendOptions<'_>,
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
        opts: &GitBackendOptions<'_>,
        repo: &str,
        api_url: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        // Check for direct platform-specific URLs first
        if let Some(direct_url) = opts.direct_url_for_target(target) {
            return Ok(ReleaseAsset {
                name: get_filename_from_url(&direct_url),
                url: direct_url.clone(),
                url_api: direct_url.clone(),
                digest: None, // Direct URLs don't have API digest
            });
        }

        let version = &tv.version;
        let version_prefix = opts.version_prefix();
        if self.is_gitlab() {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_gitlab_asset_url_for_target(
                    tv, opts, repo, api_url, &candidate, target,
                )
                .await
            })
            .await
        } else if self.is_forgejo() {
            try_with_v_prefix(version, version_prefix, |candidate| async move {
                self.resolve_forgejo_asset_url_for_target(
                    tv, opts, repo, api_url, &candidate, target,
                )
                .await
            })
            .await
        } else {
            // Pass full repo for trying reponame@version formats
            try_with_v_prefix_and_repo(
                version,
                version_prefix,
                Some(repo),
                |candidate| async move {
                    self.resolve_github_asset_url_for_target(
                        tv, opts, repo, api_url, &candidate, target,
                    )
                    .await
                },
            )
            .await
        }
    }

    /// Resolves GitHub asset URL for a specific target platform
    async fn resolve_github_asset_url_for_target(
        &self,
        tv: &ToolVersion,
        opts: &GitBackendOptions<'_>,
        repo: &str,
        api_url: &str,
        version: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        let release = self
            .get_github_release_for_url(api_url, repo, version)
            .await?;
        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();

        // Build asset list with URLs for checksum fetching
        let assets_with_urls: Vec<Asset> = release
            .assets
            .iter()
            .map(|a| Asset::new(&a.name, &a.browser_download_url))
            .collect();

        // Try explicit pattern first. `asset_pattern` replaces autodetection
        // entirely, so it intentionally takes precedence over and ignores
        // `matching`/`matching_regex` (there is no autodetected candidate set left
        // to narrow). Do NOT thread the matching filter into this branch.
        if let Some(pattern) = opts.asset_pattern_for_target(target) {
            // Template the pattern for the target platform
            let templated_pattern = template_string_for_target(&pattern, tv, target);

            let asset = self
                .pick_by_pattern(release.assets, &templated_pattern, |a| &a.name)
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
        let asset_name = asset_matcher::AssetMatcher::new()
            .for_target(target)
            .with_no_app(opts.no_app_for_target(target))
            .with_preferred_name(self.preferred_asset_name())
            .with_matching(opts.matching().unwrap_or_default())
            .with_matching_regex(opts.matching_regex().unwrap_or_default())
            .pick_from(&available_assets)?
            .name;
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
        opts: &GitBackendOptions<'_>,
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

        // Try explicit pattern first. `asset_pattern` replaces autodetection
        // entirely, so it intentionally takes precedence over and ignores
        // `matching`/`matching_regex` (there is no autodetected candidate set left
        // to narrow). Do NOT thread the matching filter into this branch.
        if let Some(pattern) = opts.asset_pattern_for_target(target) {
            // Template the pattern for the target platform
            let templated_pattern = template_string_for_target(&pattern, tv, target);

            let asset = self
                .pick_by_pattern(release.assets.links, &templated_pattern, |a| &a.name)
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
        let asset_name = asset_matcher::AssetMatcher::new()
            .for_target(target)
            .with_no_app(opts.no_app_for_target(target))
            .with_preferred_name(self.preferred_asset_name())
            .with_matching(opts.matching().unwrap_or_default())
            .with_matching_regex(opts.matching_regex().unwrap_or_default())
            .pick_from(&available_assets)?
            .name;
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

    /// Resolves Forgejo asset URL for a specific target platform
    async fn resolve_forgejo_asset_url_for_target(
        &self,
        tv: &ToolVersion,
        opts: &GitBackendOptions<'_>,
        repo: &str,
        api_url: &str,
        version: &str,
        target: &PlatformTarget,
    ) -> Result<ReleaseAsset> {
        let release = forgejo::get_release_for_url(api_url, repo, version).await?;
        let available_assets: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();

        // Build asset list with URLs for checksum fetching
        let assets_with_urls: Vec<Asset> = release
            .assets
            .iter()
            .map(|a| Asset::new(&a.name, &a.browser_download_url))
            .collect();

        // Helper to build API attachment URL
        let asset_url_api = |asset_uuid: &str| {
            format!(
                "{}/attachments/{}",
                api_url.replace("/api/v1", ""),
                asset_uuid
            )
        };

        // Try explicit pattern first. `asset_pattern` replaces autodetection
        // entirely, so it intentionally takes precedence over and ignores
        // `matching`/`matching_regex` (there is no autodetected candidate set left
        // to narrow). Do NOT thread the matching filter into this branch.
        if let Some(pattern) = opts.asset_pattern_for_target(target) {
            // Template the pattern for the target platform
            let templated_pattern = template_string_for_target(&pattern, tv, target);

            let asset = self
                .pick_by_pattern(release.assets, &templated_pattern, |a| &a.name)
                .ok_or_else(|| {
                    eyre::eyre!(
                        "No matching asset found for pattern: {}\nAvailable assets: {}",
                        templated_pattern,
                        Self::format_asset_list(available_assets.iter())
                    )
                })?;

            // Try to get checksum from API digest or fetch from release assets
            let digest = self
                .try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
                .await;

            return Ok(ReleaseAsset {
                name: asset.name,
                url: asset.browser_download_url,
                url_api: asset_url_api(&asset.uuid),
                digest,
            });
        }

        // Fall back to auto-detection for target platform
        let asset_name = asset_matcher::AssetMatcher::new()
            .for_target(target)
            .with_no_app(opts.no_app_for_target(target))
            .with_preferred_name(self.preferred_asset_name())
            .with_matching(opts.matching().unwrap_or_default())
            .with_matching_regex(opts.matching_regex().unwrap_or_default())
            .pick_from(&available_assets)?
            .name;
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
        let digest = self
            .try_fetch_checksum_from_assets(&assets_with_urls, &asset.name)
            .await;

        Ok(ReleaseAsset {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            url_api: asset_url_api(&asset.uuid),
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

    /// Picks the best asset from `assets` whose name matches `pattern`.
    ///
    /// When a pattern matches more than one asset (e.g. `*linux*64` matching both
    /// `cloudflared-linux-amd64` and `cloudflared-fips-linux-amd64`), prefer the
    /// shortest name, then lexicographic order for determinism. Mirrors the
    /// tiebreaker used by auto-detection.
    /// See: https://github.com/jdx/mise/discussions/9358
    fn pick_by_pattern<T, I, F>(&self, assets: I, pattern: &str, name_of: F) -> Option<T>
    where
        I: IntoIterator<Item = T>,
        F: Fn(&T) -> &str,
    {
        // Compile the regex once instead of recompiling per asset.
        let regex_pattern = pattern
            .replace(".", "\\.")
            .replace("*", ".*")
            .replace("?", ".");
        let re = Regex::new(&format!("^{regex_pattern}$")).ok();

        assets
            .into_iter()
            .filter(|a| {
                let name = name_of(a);
                match &re {
                    Some(re) => re.is_match(name),
                    None => name.contains(pattern),
                }
            })
            .min_by(|a, b| {
                let na = name_of(a);
                let nb = name_of(b);
                na.len().cmp(&nb.len()).then_with(|| na.cmp(nb))
            })
    }

    fn strip_version_prefix(&self, tag_name: &str, opts: &GitBackendOptions<'_>) -> String {
        // If a custom version_prefix is configured, strip it first
        if let Some(prefix) = opts.version_prefix()
            && let Some(stripped) = tag_name.strip_prefix(prefix)
        {
            return stripped.to_string();
        }

        // Handle projectname@version format (e.g., "tectonic@0.15.0" -> "0.15.0")
        // Only strip if the prefix matches the repo short name or full repo name to ensure
        // we can reconstruct the tag later during installation. For repos with multiple
        // packages (e.g., tectonic@ and tectonic_xetex_layout@), users must configure
        // version_prefix to install packages that don't match the repo name.
        if let Some(caps) = regex!(r"^([^@]+)@(\d.*)$").captures(tag_name) {
            let prefix = caps.get(1).unwrap().as_str();
            let version = caps.get(2).unwrap().as_str();
            let repo = self.repo();
            let repo_short_name = repo.split('/').next_back();
            // Strip if prefix matches repo short name OR full repo name
            if repo_short_name == Some(prefix) || repo == prefix {
                return version.to_string();
            }
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

    /// Creates a `.mise-bins` directory with symlinks only to the binaries specified in filter_bins.
    fn create_symlink_bin_dir(&self, tv: &ToolVersion, bins: Vec<String>) -> Result<()> {
        let symlink_dir = tv.install_path().join(MISE_BINS_DIR);
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

    /// When skipping full provenance re-verification (lockfile has checksum+provenance),
    /// check that the setting for the recorded provenance type is still enabled.
    /// Disabling a verification setting while the lockfile expects it is a downgrade.
    fn ensure_provenance_setting_enabled(
        &self,
        tv: &ToolVersion,
        platform_key: &str,
    ) -> Result<()> {
        super::ensure_provenance_setting_enabled(tv, platform_key, |provenance| {
            let settings = Settings::get();
            match provenance {
                ProvenanceType::GithubAttestations => {
                    Ok(!settings.github_attestations || !settings.github.github_attestations)
                }
                ProvenanceType::Slsa { .. } => Ok(!settings.slsa || !settings.github.slsa),
                // The github backend only writes GithubAttestations and Slsa; reaching here means
                // a lockfile was hand-edited or migrated incorrectly.
                _ => Err(eyre::eyre!(
                    "Lockfile has unexpected provenance type {provenance} for github backend tool {tv}. \
                     Update the lockfile to remove the stale provenance entry."
                )),
            }
        })
    }

    /// Verify artifact using GitHub artifact attestations or SLSA provenance.
    /// Tries attestations first, falls back to SLSA if no attestations found.
    /// If verification is attempted and fails, it's a hard error.
    ///
    /// Returns the verified provenance type and the GitHub attestation probe status.
    async fn verify_attestations_or_slsa(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
    ) -> Result<Option<ProvenanceType>> {
        let settings = Settings::get();

        // Read the expected provenance from the lockfile. We use .clone() because tv is
        // &ToolVersion. The result is validated against this expectation at every return
        // point: successful verification checks type match, and no-verification triggers
        // a downgrade error.
        let platform_key = self.get_platform_key();
        let locked_provenance = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|pi| pi.provenance.clone());
        let expected_provenance = locked_provenance.as_ref();
        // Only verify for GitHub repos (not GitLab/Forgejo)
        if self.is_gitlab() || self.is_forgejo() {
            if let Some(expected) = expected_provenance {
                return Err(eyre::eyre!(
                    "Lockfile requires {expected} provenance for {tv} but verification is not available \
                     for GitLab/Forgejo backends. This may indicate a downgrade attack."
                ));
            }
            return Ok(None);
        }

        // When the lockfile specifies a provenance type, only run that specific mechanism
        let skip_attestations = expected_provenance.is_some_and(|l| !l.is_github_attestations());
        let skip_slsa = expected_provenance.is_some_and(|l| !l.is_slsa());

        // If the lockfile expects github-attestations but the configured api_url
        // doesn't support them (e.g. GHE Server), surface a clear, actionable
        // error rather than falling through to the generic "downgrade attack"
        // path below.
        let raw_opts = tv.request.options();
        let opts = self.options(&raw_opts);
        let api_url = opts.api_url();
        if !attestations_supported(&api_url)
            && let Some(expected) = expected_provenance
            && expected.is_github_attestations()
        {
            return Err(eyre::eyre!(
                "Lockfile requires github-attestations provenance for {tv} but the \
                 configured api_url ({api_url}) does not serve attestations. \
                 Re-run `mise lock` to refresh the lockfile, or remove the custom api_url."
            ));
        }

        // Try GitHub artifact attestations first (if enabled globally and for github backend)
        if !skip_attestations
            && settings.github_attestations
            && settings.github.github_attestations
            && attestations_supported(&api_url)
        {
            match self
                .try_verify_github_attestations(ctx, tv, file_path, &api_url)
                .await
            {
                Ok(true) => {
                    // Defense-in-depth: verify the result matches the lockfile expectation
                    if let Some(expected) = expected_provenance
                        && !expected.is_github_attestations()
                    {
                        return Err(eyre::eyre!(
                            "Lockfile requires {expected} provenance for {tv} but github-attestations was verified. \
                             This may indicate a provenance type mismatch."
                        ));
                    }
                    return Ok(Some(ProvenanceType::GithubAttestations));
                }
                Ok(false) => {
                    // Attestations exist but verification failed - hard error
                    return Err(eyre::eyre!(
                        "GitHub artifact attestations verification failed for {tv}"
                    ));
                }
                Err(VerificationStatus::NoAttestations) => {
                    // No attestations - fall through to try SLSA
                    debug!("No GitHub artifact attestations found for {tv}, trying SLSA");
                }
                Err(VerificationStatus::ApiError(e)) => {
                    if expected_provenance.is_some() || settings.provenance_api_failures_fatal {
                        return Err(eyre::eyre!(
                            "GitHub artifact attestations verification error for {tv}: {e}"
                        ));
                    }
                    warn!("GitHub artifact attestations API failed for {tv}, trying SLSA: {e}");
                }
                Err(VerificationStatus::Error(e)) => {
                    // Error during verification - hard error
                    return Err(eyre::eyre!(
                        "GitHub artifact attestations verification error for {tv}: {e}"
                    ));
                }
            }
        }

        // Fall back to SLSA provenance (if enabled globally and for github backend)
        if !skip_slsa && settings.slsa && settings.github.slsa {
            match self.try_verify_slsa(ctx, tv, file_path, &api_url).await {
                Ok((true, provenance_url)) => {
                    // Defense-in-depth: verify the result matches the lockfile expectation
                    if let Some(expected) = expected_provenance
                        && !expected.is_slsa()
                    {
                        return Err(eyre::eyre!(
                            "Lockfile requires {expected} provenance for {tv} but slsa was verified. \
                             This may indicate a provenance type mismatch."
                        ));
                    }
                    return Ok(Some(ProvenanceType::Slsa {
                        url: provenance_url,
                    }));
                }
                Ok((false, _)) => {
                    // Provenance exists but verification failed - hard error
                    return Err(eyre::eyre!("SLSA provenance verification failed for {tv}"));
                }
                Err(VerificationStatus::NoAttestations) => {
                    // No provenance found - this is fine
                    debug!("No SLSA provenance found for {tv}");
                }
                Err(VerificationStatus::ApiError(e)) => {
                    if expected_provenance.is_some() || settings.provenance_api_failures_fatal {
                        return Err(eyre::eyre!("SLSA verification error for {tv}: {e}"));
                    }
                    warn!("SLSA provenance API failed for {tv}, skipping SLSA provenance: {e}");
                }
                Err(VerificationStatus::Error(e)) => {
                    // Error during verification - hard error
                    return Err(eyre::eyre!("SLSA verification error for {tv}: {e}"));
                }
            }
        }

        // If lockfile recorded provenance but no verification succeeded, it's a downgrade attack
        if let Some(expected) = expected_provenance {
            return Err(eyre::eyre!(
                "Lockfile requires {expected} provenance for {tv} but verification was not performed. \
                 This may indicate a downgrade attack. Enable the corresponding verification setting \
                 or update the lockfile."
            ));
        }

        Ok(None)
    }

    /// Try to verify GitHub artifact attestations. Returns:
    /// - Ok(true) if attestations exist and verified successfully
    /// - Ok(false) if attestations exist but verification failed
    /// - Err(NoAttestations) if no attestations found
    /// - Err(Error) if an error occurred during verification
    async fn try_verify_github_attestations(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
        api_url: &str,
    ) -> std::result::Result<bool, VerificationStatus> {
        ctx.pr
            .set_message("verify GitHub artifact attestations".to_string());

        // Parse owner/repo from the repo string
        let repo = self.repo();
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(VerificationStatus::Error(format!(
                "Invalid repo format: {repo}"
            )));
        }
        let (owner, repo_name) = (parts[0], parts[1]);

        match crate::github::sigstore::verify_attestation(
            file_path,
            owner,
            repo_name,
            None, // We don't know the expected workflow
            Some(api_url),
            self.use_versions_host_for_github_metadata(),
        )
        .await
        {
            Ok(verified) => {
                if verified {
                    ctx.pr
                        .set_message("✓ GitHub artifact attestations verified".to_string());
                    debug!("GitHub artifact attestations verified successfully for {tv}");
                }
                Ok(verified)
            }
            Err(crate::github::sigstore::AttestationError::NoAttestations) => {
                Err(VerificationStatus::NoAttestations)
            }
            Err(e) if crate::github::sigstore::is_api_failure(&e) => {
                Err(VerificationStatus::ApiError(e.to_string()))
            }
            Err(e) => Err(VerificationStatus::Error(e.to_string())),
        }
    }

    async fn try_verify_slsa_archive_contents(
        &self,
        tv: &ToolVersion,
        file_path: &std::path::Path,
        provenance_path: &std::path::Path,
    ) -> Result<bool> {
        let raw_opts = tv.request.options();
        let format = if let Some(format_opt) = lookup_with_fallback(&raw_opts, "format") {
            file::ExtractionFormat::from_ext(&format_opt).unwrap_or(file::ExtractionFormat::Raw)
        } else {
            file::ExtractionFormat::from_file_name(
                &file_path.file_name().unwrap_or_default().to_string_lossy(),
            )
        };

        if !format.is_archive() {
            return Err(eyre::eyre!(
                "SLSA provenance subject mismatch and content-level fallback is only supported for archives"
            ));
        }

        let mut strip_components = lookup_platform_key(&raw_opts, "strip_components")
            .or_else(|| raw_opts.get_string("strip_components"))
            .and_then(|s| s.parse().ok());
        if strip_components.is_none()
            && lookup_with_fallback(&raw_opts, "bin_path").is_none()
            && file::should_strip_components(file_path, format)?
        {
            strip_components = Some(1);
        }

        let contents =
            file::archive_content_files(file_path, format, strip_components.unwrap_or(0))?;
        let artifacts = contents
            .into_iter()
            .map(|content| crate::github::sigstore::SlsaArtifact {
                name: content.name,
                sha256: content.sha256,
            })
            .collect::<Vec<_>>();

        crate::github::sigstore::verify_slsa_provenance_artifacts(provenance_path, &artifacts, 1u8)
            .await
            .map_err(|e| eyre::eyre!("content-level SLSA verification failed: {e}"))
    }

    /// Try to verify SLSA provenance. Returns:
    /// - Ok((true, Some(url))) if provenance exists and verified successfully
    /// - Ok((false, _)) if provenance exists but verification failed
    /// - Err(NoAttestations) if no provenance found
    /// - Err(Error) if an error occurred during verification
    async fn try_verify_slsa(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        file_path: &std::path::Path,
        api_url: &str,
    ) -> std::result::Result<(bool, Option<String>), VerificationStatus> {
        if self.is_gitlab() || self.is_forgejo() {
            return Err(VerificationStatus::NoAttestations);
        }

        ctx.pr.set_message("verify SLSA provenance".to_string());

        // Get the release to find provenance assets
        let repo = self.repo();
        let raw_opts = tv.request.options();
        let opts = self.options(&raw_opts);
        let version = &tv.version;

        // Try to get the release (with version prefix support)
        let version_prefix = opts.version_prefix();
        let use_versions_host = self.use_versions_host_for_github_metadata();
        let release =
            match try_with_v_prefix_and_repo(version, version_prefix, Some(&repo), |candidate| {
                let api_url = api_url.to_string();
                let repo = repo.clone();
                async move {
                    github::get_release_for_url_with_versions_host(
                        &api_url,
                        &repo,
                        &candidate,
                        use_versions_host,
                    )
                    .await
                }
            })
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    return Err(VerificationStatus::ApiError(format!(
                        "Failed to get release: {e}"
                    )));
                }
            };

        // Find the best provenance asset for the current platform
        let asset_names: Vec<String> = release.assets.iter().map(|a| a.name.clone()).collect();
        let current_platform = PlatformTarget::from_current();
        // Keep provenance aligned with the matching-selected binary at install
        // time, matching the selection used at lock time. Suppressed when
        // `asset_pattern` is set (it selects the binary, ignoring `matching`).
        let (matching, matching_regex) = opts.matching_for_provenance(&current_platform);
        let picker = AssetPicker::with_libc(
            current_platform.os_name().to_string(),
            current_platform.arch_name().to_string(),
            current_platform.qualifier().map(|s| s.to_string()),
        )
        .with_matching(matching.unwrap_or_default())
        .with_matching_regex(matching_regex.unwrap_or_default());

        let provenance_name = match picker.pick_best_provenance(&asset_names) {
            Some(name) => name,
            None => return Err(VerificationStatus::NoAttestations),
        };

        let provenance_asset = release
            .assets
            .iter()
            .find(|a| a.name == provenance_name)
            .expect("provenance asset should exist since we found its name");

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
            return Err(VerificationStatus::ApiError(format!(
                "Failed to download provenance: {e}"
            )));
        }

        ctx.pr.set_message("verify SLSA provenance".to_string());

        // Verify the provenance
        let provenance_download_url = provenance_asset.browser_download_url.clone();
        match crate::github::sigstore::verify_slsa_provenance(
            file_path,
            &provenance_path,
            1, // Minimum SLSA level
        )
        .await
        {
            Ok(verified) => {
                if verified {
                    debug!("SLSA provenance verified successfully for {tv}");
                    Ok((true, Some(provenance_download_url)))
                } else {
                    Ok((false, None))
                }
            }
            Err(e) => {
                if crate::github::sigstore::is_slsa_subject_mismatch(&e) {
                    debug!(
                        "SLSA provenance did not cover downloaded artifact for {tv}; trying archive content subjects: {e}"
                    );
                    match self
                        .try_verify_slsa_archive_contents(tv, file_path, &provenance_path)
                        .await
                    {
                        Ok(true) => {
                            debug!(
                                "SLSA provenance verified archive contents successfully for {tv}"
                            );
                            Ok((true, Some(provenance_download_url)))
                        }
                        Ok(false) => Ok((false, None)),
                        Err(content_err) => Err(VerificationStatus::Error(content_err.to_string())),
                    }
                } else if is_slsa_format_issue(&e) {
                    debug!("SLSA provenance file not in verifiable format for {tv}: {e}");
                    Err(VerificationStatus::NoAttestations)
                } else {
                    Err(VerificationStatus::Error(e.to_string()))
                }
            }
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

    // Check for legacy {placeholder} syntax (any of the supported placeholders)
    let has_legacy_placeholder = [
        "{version}",
        "{os}",
        "{arch}",
        "{darwin_os}",
        "{amd64_arch}",
        "{x86_64_arch}",
        "{gnu_arch}",
    ]
    .iter()
    .any(|p| template.contains(p) && !template.contains(&format!("{{{p}}}")));

    if has_legacy_placeholder {
        deprecated_at!(
            "2026.3.0",
            "2027.3.0",
            "legacy-version-template",
            "Use Tera syntax (e.g., {{{{ version }}}}) instead of legacy {{version}} in templates"
        );
        // Legacy support: replace {placeholder} patterns
        return template
            .replace("{version}", version)
            .replace("{os}", os)
            .replace("{arch}", arch)
            .replace("{darwin_os}", darwin_os)
            .replace("{amd64_arch}", amd64_arch)
            .replace("{x86_64_arch}", x86_64_arch)
            .replace("{gnu_arch}", gnu_arch);
    }

    if !crate::tera::contains_template_syntax(template) {
        return template.to_string();
    }

    // Use Tera rendering for templates
    let mut ctx = crate::tera::BASE_CONTEXT.clone();
    ctx.insert("version", version);
    ctx.insert("os", os);
    ctx.insert("arch", arch);
    ctx.insert("darwin_os", darwin_os);
    ctx.insert("amd64_arch", amd64_arch);
    ctx.insert("x86_64_arch", x86_64_arch);
    ctx.insert("gnu_arch", gnu_arch);

    let mut tera = crate::tera::get_tera(None);
    // Register target-aware os() and arch() functions that use the target platform
    // instead of the compile-time platform
    let make_remapping_fn = |value: String| {
        move |args: &HashMap<String, tera::Value>| -> tera::Result<tera::Value> {
            if let Some(s) = args.get(value.as_str()).and_then(|v| v.as_str()) {
                Ok(tera::Value::String(s.to_string()))
            } else {
                Ok(tera::Value::String(value.clone()))
            }
        }
    };
    tera.register_function("os", make_remapping_fn(os.to_string()));
    tera.register_function("arch", make_remapping_fn(arch.to_string()));

    match crate::tera::render_str(&mut tera, template, &ctx) {
        Ok(rendered) => rendered,
        Err(e) => {
            warn!("Failed to render template '{}': {}", template, e);
            template.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;

    fn create_test_backend() -> UnifiedGitBackend {
        UnifiedGitBackend::from_arg(BackendArg::new(
            "github:test/repo".to_string(),
            Some("github:test/repo".to_string()),
        ))
    }

    fn create_test_gitlab_backend() -> UnifiedGitBackend {
        UnifiedGitBackend::from_arg(BackendArg::new(
            "gitlab:test/repo".to_string(),
            Some("gitlab:test/repo".to_string()),
        ))
    }

    fn create_test_forgejo_backend() -> UnifiedGitBackend {
        UnifiedGitBackend::from_arg(BackendArg::new(
            "forgejo:test/repo".to_string(),
            Some("forgejo:test/repo".to_string()),
        ))
    }

    #[test]
    fn test_pick_by_pattern_basic() {
        // Single-match cases that the old `matches_pattern` test covered.
        let backend = create_test_backend();
        let matches = |asset: &str, pat: &str| {
            backend
                .pick_by_pattern(vec![asset.to_string()], pat, |s| s.as_str())
                .is_some()
        };
        assert!(matches("test-v1.0.0.zip", "test-*"));
        assert!(!matches("other-v1.0.0.zip", "test-*"));
    }

    #[test]
    fn test_pick_by_pattern_shortest_match_wins() {
        // When asset_pattern matches more than one asset, prefer the shortest
        // (then lexicographic for determinism). Mirrors the auto-detection
        // tiebreaker so users don't get the GitHub-API-order asset on a
        // broad pattern like `*linux*64`.
        // See: https://github.com/jdx/mise/discussions/9358
        let backend = create_test_backend();
        let assets = vec![
            "cloudflared-fips-linux-amd64".to_string(),
            "cloudflared-linux-amd64".to_string(),
        ];
        let picked = backend.pick_by_pattern(assets.clone(), "*linux*64", |s| s.as_str());
        assert_eq!(picked.as_deref(), Some("cloudflared-linux-amd64"));

        // Order-independent.
        let assets_reordered = vec![
            "cloudflared-linux-amd64".to_string(),
            "cloudflared-fips-linux-amd64".to_string(),
        ];
        let picked = backend.pick_by_pattern(assets_reordered, "*linux*64", |s| s.as_str());
        assert_eq!(picked.as_deref(), Some("cloudflared-linux-amd64"));
    }

    #[test]
    fn test_pick_by_pattern_no_match() {
        let backend = create_test_backend();
        let assets = vec!["a.zip".to_string(), "b.zip".to_string()];
        let picked = backend.pick_by_pattern(assets, "c.zip", |s| s.as_str());
        assert!(picked.is_none());
    }

    #[test]
    fn test_pick_by_pattern_exact_match() {
        // Anchored pattern with no wildcards only matches one asset.
        let backend = create_test_backend();
        let assets = vec![
            "cloudflared-fips-linux-amd64".to_string(),
            "cloudflared-linux-amd64".to_string(),
        ];
        let picked = backend.pick_by_pattern(assets, "cloudflared-linux-amd64", |s| s.as_str());
        assert_eq!(picked.as_deref(), Some("cloudflared-linux-amd64"));
    }

    #[test]
    fn test_version_prefix_functionality() {
        let backend = create_test_backend();
        let default_raw_opts = ToolVersionOptions::default();
        let default_opts = backend.options(&default_raw_opts);

        // Test with no version prefix configured
        assert_eq!(
            backend.strip_version_prefix("v1.0.0", &default_opts),
            "1.0.0"
        );
        assert_eq!(
            backend.strip_version_prefix("1.0.0", &default_opts),
            "1.0.0"
        );

        // Test projectname@version format - only strips if prefix matches repo name
        // Backend uses "github:test/repo" so repo short name is "repo", full name is "test/repo"
        assert_eq!(
            backend.strip_version_prefix("repo@0.15.0", &default_opts),
            "0.15.0"
        );
        assert_eq!(
            backend.strip_version_prefix("repo@1.2.3", &default_opts),
            "1.2.3"
        );
        // Also accepts full repo name as prefix
        assert_eq!(
            backend.strip_version_prefix("test/repo@2.0.0", &default_opts),
            "2.0.0"
        );
        // Should NOT strip if prefix doesn't match repo name (prevents listing
        // versions that can't be installed)
        assert_eq!(
            backend.strip_version_prefix("other_package@0.15.0", &default_opts),
            "other_package@0.15.0"
        );
        // Should not match if part after @ doesn't start with a digit
        assert_eq!(
            backend.strip_version_prefix("repo@beta", &default_opts),
            "repo@beta"
        );

        // Test with custom version prefix
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "version_prefix".to_string(),
            toml::Value::String("release-".to_string()),
        );
        let opts = backend.options(&opts);

        assert_eq!(
            backend.strip_version_prefix("release-1.0.0", &opts),
            "1.0.0"
        );
        assert_eq!(backend.strip_version_prefix("1.0.0", &opts), "1.0.0");
    }

    #[test]
    fn test_matching_options_are_install_time_keys() {
        // `matching`/`matching_regex` must be install-time-only keys so a stale
        // cached filter from a prior install can't silently override what's in
        // mise.toml now. They are deliberately NOT folded into the install path
        // (that stays keyed by tool name + version) — `tool_alias` is the way to
        // install multiple binaries from one repo into distinct dirs.
        let keys = install_time_option_keys();
        assert!(keys.contains(&"matching".to_string()));
        assert!(keys.contains(&"matching_regex".to_string()));
    }

    #[test]
    fn test_lockfile_options_use_target_artifact_inputs() {
        let backend = create_test_backend();
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "api_url".to_string(),
            toml::Value::String("https://github.example.com/api/v3".to_string()),
        );
        opts.opts.insert(
            "version_prefix".to_string(),
            toml::Value::String("release-".to_string()),
        );
        // matching/matching_regex are top-level (not per-platform) options; they
        // must round-trip into lockfile_options for every target so a relock on
        // another OS reproduces the same asset selection.
        opts.opts.insert(
            "matching".to_string(),
            toml::Value::String("tool".to_string()),
        );
        opts.opts.insert(
            "matching_regex".to_string(),
            toml::Value::String("^tool-".to_string()),
        );
        let mut platforms = toml::Table::new();
        let mut linux = toml::Table::new();
        linux.insert(
            "asset_pattern".to_string(),
            toml::Value::String("tool-*-linux.tar.gz".to_string()),
        );
        let mut windows = toml::Table::new();
        windows.insert(
            "asset_pattern".to_string(),
            toml::Value::String("tool-*-windows.zip".to_string()),
        );
        windows.insert("no_app".to_string(), toml::Value::Boolean(true));
        platforms.insert("linux-x64".to_string(), toml::Value::Table(linux));
        platforms.insert("windows-x64".to_string(), toml::Value::Table(windows));
        opts.opts
            .insert("platforms".to_string(), toml::Value::Table(platforms));

        let linux = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());
        let windows = PlatformTarget::new(crate::platform::Platform::parse("windows-x64").unwrap());

        assert_eq!(
            backend.options(&opts).lockfile_options(&linux),
            BTreeMap::from([
                (
                    "api_url".to_string(),
                    "https://github.example.com/api/v3".to_string()
                ),
                (
                    "asset_pattern".to_string(),
                    "tool-*-linux.tar.gz".to_string()
                ),
                ("matching".to_string(), "tool".to_string()),
                ("matching_regex".to_string(), "^tool-".to_string()),
                ("version_prefix".to_string(), "release-".to_string()),
            ])
        );
        assert_eq!(
            backend.options(&opts).lockfile_options(&windows),
            BTreeMap::from([
                (
                    "api_url".to_string(),
                    "https://github.example.com/api/v3".to_string()
                ),
                (
                    "asset_pattern".to_string(),
                    "tool-*-windows.zip".to_string()
                ),
                ("matching".to_string(), "tool".to_string()),
                ("matching_regex".to_string(), "^tool-".to_string()),
                ("no_app".to_string(), "true".to_string()),
                ("version_prefix".to_string(), "release-".to_string()),
            ])
        );
    }

    #[test]
    fn test_matching_for_provenance_suppressed_when_asset_pattern_set() {
        // `asset_pattern` selects the binary directly and ignores `matching`, so
        // provenance must NOT be narrowed by `matching` on that path — otherwise a
        // self-contradictory config could attach a *different* binary's provenance
        // than `asset_pattern` picked, and an invalid `matching_regex` (never
        // validated on the asset_pattern path) could silently skip verification.
        let backend = create_test_backend();
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "matching".to_string(),
            toml::Value::String("oxlint".to_string()),
        );
        opts.opts.insert(
            "matching_regex".to_string(),
            toml::Value::String("^oxlint-".to_string()),
        );
        // asset_pattern set for linux only, not for macos.
        let mut platforms = toml::Table::new();
        let mut linux = toml::Table::new();
        linux.insert(
            "asset_pattern".to_string(),
            toml::Value::String("oxlint-*-linux.tar.gz".to_string()),
        );
        platforms.insert("linux-x64".to_string(), toml::Value::Table(linux));
        opts.opts
            .insert("platforms".to_string(), toml::Value::Table(platforms));

        let linux = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());
        let macos = PlatformTarget::new(crate::platform::Platform::parse("macos-arm64").unwrap());

        // asset_pattern set for this target -> matching suppressed for provenance.
        assert_eq!(
            backend.options(&opts).matching_for_provenance(&linux),
            (None, None)
        );
        // No asset_pattern for this target -> matching flows through to provenance.
        assert_eq!(
            backend.options(&opts).matching_for_provenance(&macos),
            (Some("oxlint"), Some("^oxlint-"))
        );
    }

    #[test]
    fn test_matching_plumbing_parity_across_git_backends() {
        // The github/gitlab/forgejo backends share one option struct
        // (`GitBackendOptions`) and one `AssetMatcher`, but each has its OWN
        // `resolve_*_asset_url_for_target` function that threads
        // `matching`/`matching_regex` separately (copy-paste identical today, with
        // only backend-specific asset/digest plumbing differing). This test guards
        // the shared seams those three paths depend on from drifting per backend
        // type: the option accessors, lockfile serialization, and install-time-key
        // inheritance must behave identically for all three. The resolve functions
        // themselves are covered end-to-end for github by
        // e2e/backend/test_github_matching, and the matcher all three feed is
        // covered by the asset_matcher unit tests.
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "matching".to_string(),
            toml::Value::String("oxlint".to_string()),
        );
        opts.opts.insert(
            "matching_regex".to_string(),
            toml::Value::String("^oxlint-".to_string()),
        );
        let target = PlatformTarget::new(crate::platform::Platform::parse("linux-x64").unwrap());

        // Guard that the helpers really build distinct backend types, so the loop
        // below genuinely exercises gitlab/forgejo and isn't three githubs.
        assert!(create_test_gitlab_backend().is_gitlab());
        assert!(create_test_forgejo_backend().is_forgejo());

        for backend in [
            create_test_backend(),
            create_test_gitlab_backend(),
            create_test_forgejo_backend(),
        ] {
            let backend_type = backend.ba.backend_type();
            let resolved = backend.options(&opts);

            // Accessors the three resolve_*_asset_url_for_target functions read.
            assert_eq!(
                resolved.matching(),
                Some("oxlint"),
                "matching() must be readable for {backend_type:?}"
            );
            assert_eq!(
                resolved.matching_regex(),
                Some("^oxlint-"),
                "matching_regex() must be readable for {backend_type:?}"
            );

            // Both keys must round-trip to the lockfile for every git backend so a
            // relock on another platform reproduces the same asset selection.
            let lf = resolved.lockfile_options(&target);
            assert_eq!(
                lf.get("matching").map(String::as_str),
                Some("oxlint"),
                "matching must round-trip to lockfile for {backend_type:?}"
            );
            assert_eq!(
                lf.get("matching_regex").map(String::as_str),
                Some("^oxlint-"),
                "matching_regex must round-trip to lockfile for {backend_type:?}"
            );

            // Cache-keying: a stale cached filter must never silently override
            // mise.toml, so both keys must be install-time keys for every type
            // (gitlab/forgejo inherit github's list via the routing in mod.rs).
            let itk = crate::backend::install_time_option_keys_for_type(&backend_type);
            assert!(
                itk.contains(&"matching".to_string())
                    && itk.contains(&"matching_regex".to_string()),
                "matching/matching_regex must be install-time keys for {backend_type:?}"
            );
            // ...including the per-platform `platforms.<target>.matching` form the
            // stale-cache check uses.
            assert!(
                crate::backend::is_install_time_option_key_for_type(&backend_type, "matching")
                    && crate::backend::is_install_time_option_key_for_type(
                        &backend_type,
                        "platforms.linux-x64.matching"
                    ),
                "is_install_time_option_key_for_type must report matching for {backend_type:?}"
            );
        }
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

    #[test]
    fn test_is_slsa_format_issue_no_attestations() {
        let err = crate::github::sigstore::AttestationError::NoAttestations;
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_invalid_format() {
        // This is the exact error from BuildKit raw provenance files parsed line-by-line
        let err = crate::github::sigstore::AttestationError::Verification(
            "File does not contain valid attestations or SLSA provenance".to_string(),
        );
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_no_certificate() {
        let err = crate::github::sigstore::AttestationError::Verification(
            "No certificate found in attestation bundle".to_string(),
        );
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_no_dsse_envelope() {
        let err = crate::github::sigstore::AttestationError::Verification(
            "Bundle has neither DSSE envelope nor message signature".to_string(),
        );
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_real_verification_failure() {
        // Digest mismatch = real verification failure, NOT a format issue
        let err = crate::github::sigstore::AttestationError::Verification(
            "Artifact digest mismatch: expected abc123".to_string(),
        );
        assert!(!is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_signature_failure() {
        // Signature verification failure = real failure, NOT a format issue
        let err = crate::github::sigstore::AttestationError::Verification(
            "P-256 signature verification failed: invalid signature".to_string(),
        );
        assert!(!is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_api_error() {
        let err = crate::github::sigstore::AttestationError::Api("connection refused".to_string());
        assert!(!is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_sigstore_missing_field() {
        // mise-sigstore maps sigstore-verify's "missing field …" JSON parse
        // failures into the Sigstore variant. Treat those as format issues.
        let err = crate::github::sigstore::AttestationError::Sigstore(
            "JSON error: missing field `verificationMaterial` at line 1 column 8480".to_string(),
        );
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_is_slsa_format_issue_unsupported_format() {
        let err = crate::github::sigstore::AttestationError::UnsupportedFormat(
            "Not an SLSA provenance predicate: https://in-toto.io/Statement/v1".to_string(),
        );
        assert!(is_slsa_format_issue(&err));
    }

    #[test]
    fn test_attestations_supported_default_api() {
        assert!(attestations_supported("https://api.github.com"));
        // Trailing slashes are common when users hand-write api_url
        assert!(attestations_supported("https://api.github.com/"));
    }

    #[test]
    fn test_attestations_supported_custom_api_url() {
        assert!(!attestations_supported("https://ghe.example.com/api/v3"));
        assert!(!attestations_supported("https://gitlab.com/api/v4"));
        assert!(!attestations_supported("https://codeberg.org/api/v1"));
    }
}
