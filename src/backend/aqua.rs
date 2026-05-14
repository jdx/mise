use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;

use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::Settings;
use crate::file::{TarFormat, TarOptions};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::{GithubAttestationsStatus, PlatformInfo, ProvenanceType};
use crate::path::{Path, PathBuf, PathExt};
use crate::plugins::VERSION_REGEX;
use crate::registry::REGISTRY;
use crate::toolset::{EPHEMERAL_OPT_KEYS, ToolRequest, ToolVersion, ToolVersionOptions};
use crate::ui::progress_report::SingleReport;
use crate::{
    aqua::aqua_registry_wrapper::{
        AQUA_REGISTRY, AquaChecksum, AquaChecksumType, AquaCosign, AquaMinisignType, AquaPackage,
        AquaPackageType,
    },
    cache::{CacheManager, CacheManagerBuilder},
};
use crate::{
    backend::{Backend, MISE_BINS_DIR, strict_metadata},
    config::Config,
};
use crate::{file, github, minisign};
use async_trait::async_trait;
use eyre::{ContextCompat, Result, WrapErr, bail, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use regex::Regex;
use std::borrow::Cow;
use std::fmt::Debug;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    sync::Arc,
};

#[derive(Debug)]
pub struct AquaBackend {
    ba: Arc<BackendArg>,
    id: String,
    version_tags_cache: CacheManager<Vec<(String, String)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AquaFileLink {
    src: PathBuf,
    dst: PathBuf,
    hard: bool,
    explicit_link: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GithubAttestationStatus {
    Verified,
    Unavailable,
}

#[async_trait]
impl Backend for AquaBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Aqua
    }

    async fn description(&self) -> Option<String> {
        AQUA_REGISTRY
            .package(&self.ba.tool_name)
            .await
            .ok()
            .and_then(|p| p.description.clone())
    }

    async fn install_operation_count(&self, tv: &ToolVersion, _ctx: &InstallContext) -> usize {
        let pkg = match self.package_with_options(tv, &[&tv.version]).await {
            Ok(pkg) => pkg,
            Err(_) => return 3, // fallback to default
        };
        let format = pkg.format(&tv.version, os(), arch()).unwrap_or_default();

        let mut count = 1; // download
        // Count checksum operation if explicitly configured OR if this is a GitHub release
        // (GitHub API may provide a digest even without explicit checksum config)
        if pkg.checksum.as_ref().is_some_and(|c| c.enabled())
            || pkg.r#type == AquaPackageType::GithubRelease
        {
            count += 1;
        }
        if needs_extraction(format, &pkg.r#type) {
            count += 1;
        }
        count
    }

    async fn security_info(&self) -> Vec<crate::backend::SecurityFeature> {
        use crate::backend::SecurityFeature;

        let pkg = match AQUA_REGISTRY.package(&self.ba.tool_name).await {
            Ok(pkg) => pkg,
            Err(_) => return vec![],
        };

        let mut features = vec![];

        // Check base package and all version overrides for security features
        // This gives a complete picture of available security features across all versions
        let all_pkgs: Vec<&AquaPackage> = std::iter::once(&pkg)
            .chain(pkg.version_overrides.iter())
            .collect();

        // Fetch release assets to detect actual security features
        let release_assets = if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
            let repo = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
            github::list_releases(&repo)
                .await
                .ok()
                .and_then(|releases| releases.first().cloned())
                .map(|r| r.assets)
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Checksum - check registry config OR actual release assets
        let has_checksum_config = all_pkgs.iter().any(|p| {
            p.checksum
                .as_ref()
                .is_some_and(|checksum| checksum.enabled())
        });
        let has_checksum_assets = release_assets.iter().any(|a| {
            let name = a.name.to_lowercase();
            name.contains("sha256")
                || name.contains("checksum")
                || name.ends_with(".sha256")
                || name.ends_with(".sha512")
        });
        if has_checksum_config || has_checksum_assets {
            let algorithm = all_pkgs
                .iter()
                .filter_map(|p| p.checksum.as_ref())
                .find_map(|c| c.algorithm.as_ref().map(|a| a.to_string()))
                .or_else(|| {
                    if has_checksum_assets {
                        Some("sha256".to_string())
                    } else {
                        None
                    }
                });
            features.push(SecurityFeature::Checksum { algorithm });
        }

        // GitHub Artifact Attestations - check registry config OR actual release assets
        let has_attestations_config = all_pkgs.iter().any(|p| {
            p.github_artifact_attestations
                .as_ref()
                .is_some_and(|a| a.enabled.unwrap_or(true))
        });
        let has_attestations_assets = release_assets.iter().any(|a| {
            let name = a.name.to_lowercase();
            name.ends_with(".sigstore.json") || name.ends_with(".sigstore")
        });
        if has_attestations_config || has_attestations_assets {
            let signer_workflow = all_pkgs
                .iter()
                .filter_map(|p| p.github_artifact_attestations.as_ref())
                .find_map(|a| a.signer_workflow.clone());
            features.push(SecurityFeature::GithubAttestations { signer_workflow });
        }

        // SLSA - check registry config OR actual release assets
        let has_slsa_config = all_pkgs.iter().any(|p| {
            p.slsa_provenance
                .as_ref()
                .is_some_and(|s| s.enabled.unwrap_or(true))
        });
        let has_slsa_assets = release_assets.iter().any(|a| {
            let name = a.name.to_lowercase();
            name.contains(".intoto.jsonl")
                || name.contains("provenance")
                || name.ends_with(".attestation")
        });
        if has_slsa_config || has_slsa_assets {
            features.push(SecurityFeature::Slsa { level: None });
        }

        // Cosign - check registry config OR actual release assets
        let has_cosign_config = all_pkgs.iter().any(|p| {
            Self::binary_cosign_config(p).is_some() || Self::checksum_cosign_config(p).is_some()
        });
        let has_cosign_assets = release_assets.iter().any(|a| {
            let name = a.name.to_lowercase();
            name.ends_with(".sig") || name.contains("cosign")
        });
        if has_cosign_config || has_cosign_assets {
            features.push(SecurityFeature::Cosign);
        }

        // Minisign - check registry config OR actual release assets
        let has_minisign_config = all_pkgs.iter().any(|p| {
            p.minisign
                .as_ref()
                .is_some_and(|m| m.enabled.unwrap_or(true))
        });
        let has_minisign_assets = release_assets.iter().any(|a| {
            let name = a.name.to_lowercase();
            name.ends_with(".minisig")
        });
        if has_minisign_config || has_minisign_assets {
            let public_key = all_pkgs
                .iter()
                .filter_map(|p| p.minisign.as_ref())
                .find_map(|m| m.public_key.clone());
            features.push(SecurityFeature::Minisign { public_key });
        }

        features
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let pkg = match AQUA_REGISTRY.package(&self.id).await {
            Ok(pkg) => pkg,
            Err(e) => {
                warn!("Remote versions cannot be fetched: {}", e);
                return Ok(vec![]);
            }
        };

        if pkg.repo_owner.is_empty() || pkg.repo_name.is_empty() {
            warn!(
                "aqua package {} does not have repo_owner and/or repo_name.",
                self.id
            );
            return Ok(vec![]);
        }

        // Always fetch the pre-release superset; the shared remote-versions
        // cache stores it untouched and the trait's read path filters on
        // `VersionInfo.prerelease` based on the current tool opts.
        let tags_with_timestamps = match get_tags_with_created_at(&pkg).await {
            Ok(tags) => tags,
            Err(e) => {
                if strict_metadata() {
                    return Err(e).wrap_err_with(|| {
                        format!("failed to fetch aqua release metadata for {}", self.id)
                    });
                }
                warn!("Remote versions cannot be fetched: {}", e);
                return Ok(vec![]);
            }
        };

        let target = PlatformTarget::from_current();
        let (target_os, target_arch) = Self::to_aqua_platform(&target);
        let target_libc = Self::target_variant_libc(&target);
        let mut versions = Vec::new();
        for (tag, created_at, prerelease) in tags_with_timestamps.into_iter().rev() {
            let (version, versioned_pkg) = match versioned_package_from_tag(
                &pkg,
                &tag,
                target_os,
                target_arch,
                target_libc.as_deref(),
            ) {
                Ok(Some(versioned)) => versioned,
                Ok(None) => continue,
                Err(e) => {
                    warn!("[{}] aqua version filter error: {e}", self.ba());
                    continue;
                }
            };

            // Validate the package has assets
            if package_has_asset(&versioned_pkg) {
                let release_url = format!(
                    "https://github.com/{}/{}/releases/tag/{}",
                    pkg.repo_owner, pkg.repo_name, tag
                );
                versions.push(VersionInfo {
                    version,
                    created_at,
                    release_url: Some(release_url),
                    prerelease,
                    ..Default::default()
                });
            }
        }
        Ok(versions)
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> Result<Option<String>> {
        let opts = config.get_tool_opts_with_overrides(&self.ba).await?;
        if self.include_prereleases(&opts) {
            return Ok(None);
        }
        self.latest_marked_release_version().await
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        mut tv: ToolVersion,
    ) -> Result<ToolVersion> {
        // Check if URL already exists in lockfile platforms first
        // This allows us to skip API calls when lockfile has the URL
        let platform_key = self.get_platform_key();
        let existing_platform = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|asset| asset.url.clone());

        // Skip get_version_tags() API call if we have lockfile URL
        let tag = if existing_platform.is_some() {
            None // We'll determine version from URL instead
        } else {
            match self.get_version_tags().await {
                Ok(tags) => tags
                    .iter()
                    .find(|(version, _)| version == &tv.version)
                    .map(|(_, tag)| tag.clone()),
                Err(e) => {
                    warn!(
                        "[{}] failed to fetch version tags, URL may be incorrect: {e}",
                        self.id
                    );
                    None
                }
            }
        };
        if tag.is_none() && existing_platform.is_none() && !tv.version.starts_with('v') {
            debug!(
                "[{}] no tag found for version {}, will try with 'v' prefix",
                self.id, tv.version
            );
        }
        let mut v = tag.clone().unwrap_or_else(|| tv.version.clone());
        let mut v_prefixed =
            (tag.is_none() && !tv.version.starts_with('v')).then(|| format!("v{v}"));
        let versions = match &v_prefixed {
            Some(v_prefixed) => vec![v.as_str(), v_prefixed.as_str()],
            None => vec![v.as_str()],
        };
        let pkg = self.package_with_options(&tv, &versions).await?;
        if let Some(prefix) = &pkg.version_prefix
            && !v.starts_with(prefix)
        {
            v = format!("{prefix}{v}");
            // Don't add prefix to v_prefixed if it already starts with the prefix
            v_prefixed = v_prefixed.map(|vp| {
                if vp.starts_with(prefix) {
                    vp
                } else {
                    format!("{prefix}{vp}")
                }
            });
        }
        validate(&pkg)?;

        // Validate lockfile URL matches expected asset pattern from registry
        // This handles cases where the registry format changed (e.g., raw binary -> tar.gz)
        // Only validate for GithubRelease packages - other types use fixed URL formats
        // In locked mode, trust the lockfile URL without validation to avoid API calls
        let validated_url = if let Some(ref url) = existing_platform {
            if ctx.locked || pkg.r#type != AquaPackageType::GithubRelease {
                existing_platform // Trust lockfile URL in locked mode or for non-release types
            } else {
                let cached_filename = get_filename_from_url(url);
                let cached_filename_lower = cached_filename.to_lowercase();
                // Check assets for both version variants (with and without v prefix)
                let version_variants: Vec<&str> = match &v_prefixed {
                    Some(vp) => vec![v.as_str(), vp.as_str()],
                    None => vec![v.as_str()],
                };
                let matches = version_variants.iter().any(|ver| {
                    pkg.asset_strs(ver, os(), arch())
                        .unwrap_or_default()
                        .iter()
                        .any(|expected| {
                            // Case-insensitive match to align with github_release_asset behavior
                            cached_filename == *expected
                                || cached_filename_lower == expected.to_lowercase()
                        })
                });
                if matches {
                    existing_platform
                } else {
                    warn!(
                        "lockfile asset '{}' doesn't match registry, refreshing",
                        cached_filename
                    );
                    None
                }
            }
        } else {
            None
        };

        let (url, v, filename, api_digest) = if let Some(validated_url) = validated_url.clone() {
            let url = validated_url;
            let filename = get_filename_from_url(&url);
            // Determine which version variant was used based on the URL or filename
            // Check for version_prefix (e.g., "jq-" for jq), "v" prefix, or raw version
            let v = if let Some(prefix) = &pkg.version_prefix {
                let prefixed_version = format!("{prefix}{}", tv.version);
                if url.contains(&prefixed_version) || filename.contains(&prefixed_version) {
                    prefixed_version
                } else if url.contains(&format!("v{}", tv.version))
                    || filename.contains(&format!("v{}", tv.version))
                {
                    format!("v{}", tv.version)
                } else {
                    tv.version.clone()
                }
            } else if url.contains(&format!("v{}", tv.version))
                || filename.contains(&format!("v{}", tv.version))
            {
                format!("v{}", tv.version)
            } else {
                tv.version.clone()
            };
            (url, v, filename, None)
        } else if ctx.locked {
            bail!(
                "No lockfile URL found for {} on platform {} (--locked mode requires pre-resolved URLs)",
                self.id,
                platform_key
            );
        } else {
            let (url, v, digest) = if let Some(v_prefixed) = v_prefixed {
                // Try v-prefixed version first because most aqua packages use v-prefixed versions
                match self.get_url(&pkg, v_prefixed.as_ref()).await {
                    // If the url is already checked, use it
                    Ok((url, true, digest)) => (url, v_prefixed, digest),
                    Ok((url_prefixed, false, digest_prefixed)) => {
                        let (url, _, digest) = self.get_url(&pkg, &v).await?;
                        // If the v-prefixed URL is the same as the non-prefixed URL, use it
                        if url == url_prefixed {
                            (url_prefixed, v_prefixed, digest_prefixed)
                        } else {
                            // If they are different, check existence
                            match HTTP.head(&url_prefixed).await {
                                Ok(_) => (url_prefixed, v_prefixed, digest_prefixed),
                                Err(_) => (url, v, digest),
                            }
                        }
                    }
                    Err(err) => {
                        let (url, _, digest) =
                            self.get_url(&pkg, &v).await.map_err(|e| err.wrap_err(e))?;
                        (url, v, digest)
                    }
                }
            } else {
                let (url, _, digest) = self.get_url(&pkg, &v).await?;
                (url, v, digest)
            };
            let filename = get_filename_from_url(&url);

            (url, v.to_string(), filename, digest)
        };

        let format = pkg.format(&v, os(), arch()).unwrap_or_default();

        self.download(ctx, &tv, &url, &filename).await?;

        if validated_url.is_none() {
            // Store the asset URL and digest (if available) in the tool version
            let platform_info = tv.lock_platforms.entry(platform_key).or_default();
            platform_info.url = Some(url.clone());
            if let Some(digest) = api_digest.clone() {
                debug!("using GitHub API digest for checksum verification");
                platform_info.checksum = Some(digest);
            }
        }

        // Advance to checksum operation if applicable
        if pkg.checksum.as_ref().is_some_and(|c| c.enabled()) || api_digest.is_some() {
            ctx.pr.next_operation();
        }
        self.verify(ctx, &mut tv, &pkg, &v, &filename).await?;

        // Advance to extraction operation if applicable
        if needs_extraction(format, &pkg.r#type) {
            ctx.pr.next_operation();
        }
        self.install(ctx, &tv, &pkg, &v, &filename)?;

        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        let runtime_path = tv.runtime_path();
        let mise_bins_dir = tv.install_path().join(MISE_BINS_DIR);
        if self.symlink_bins(tv) || mise_bins_dir.is_dir() {
            return Ok(vec![runtime_path.join(MISE_BINS_DIR)]);
        }

        let install_path = tv.install_path();

        // For linked versions (external symlinks created via `mise link`),
        // skip aqua registry lookup — the linked install has its own layout.
        if let Ok(Some(target)) = file::resolve_symlink(&install_path)
            && target.is_absolute()
        {
            let bin = install_path.join("bin");
            return Ok(if bin.is_dir() {
                vec![bin]
            } else {
                vec![install_path]
            });
        }

        let request_options = tv.request.options();
        let cache_key = Self::lockfile_options(&request_options);
        let cache: CacheManager<Vec<PathBuf>> =
            CacheManagerBuilder::new(tv.cache_path().join("bin_paths.msgpack.z"))
                .with_fresh_file(install_path.clone())
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .with_cache_key(format!("{cache_key:?}"))
                .build();

        let paths = cache
            .get_or_try_init_async(async || {
                let pkg = self.package_with_options(tv, &[&tv.version]).await?;

                let srcs = Self::srcs(&pkg, tv)?;
                let paths = if srcs.is_empty() {
                    vec![install_path.clone()]
                } else {
                    srcs.iter()
                        .map(|link| link.dst.parent().unwrap().to_path_buf())
                        .collect()
                };
                Ok(paths
                    .into_iter()
                    .unique()
                    .filter(|p| p.exists())
                    .filter_map(|p| p.strip_prefix(&install_path).ok().map(|p| p.to_path_buf()))
                    .collect())
            })
            .await?
            .iter()
            .map(|p| p.mount(&runtime_path))
            .collect();
        Ok(paths)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        Self::lockfile_options(&request.options())
    }

    fn fuzzy_match_filter(
        &self,
        versions: Vec<String>,
        query: &str,
        filter_prereleases: bool,
    ) -> Vec<String> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "\\D*[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{query}([-.].+)?$")).unwrap();
        versions
            .into_iter()
            .filter(|v| {
                if query == v {
                    return true;
                }
                if filter_prereleases && VERSION_REGEX.is_match(v) {
                    return false;
                }
                query_regex.is_match(v)
            })
            .collect()
    }

    /// Resolve platform-specific lock information for any target platform.
    /// This enables cross-platform lockfile generation without installation.
    async fn resolve_lock_info(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> Result<PlatformInfo> {
        let (target_os, target_arch) = Self::to_aqua_platform(target);

        // Get version tag
        let tag = match self.get_version_tags().await {
            Ok(tags) => tags
                .iter()
                .find(|(version, _)| version == &tv.version)
                .map(|(_, tag)| tag.clone()),
            Err(e) => {
                warn!(
                    "[{}] failed to fetch version tags for lockfile, URL may be incorrect: {e}",
                    self.id
                );
                None
            }
        };
        let tag_is_none = tag.is_none();
        if tag_is_none && !tv.version.starts_with('v') {
            debug!(
                "[{}] no tag found for version {} during lock, will try with 'v' prefix",
                self.id, tv.version
            );
        }
        let mut v = tag.unwrap_or_else(|| tv.version.clone());
        let mut v_prefixed = (tag_is_none && !tv.version.starts_with('v')).then(|| format!("v{v}"));
        let versions = match &v_prefixed {
            Some(v_prefixed) => vec![v.as_str(), v_prefixed.as_str()],
            None => vec![v.as_str()],
        };

        // Get package and apply version/overrides directly for the target platform.
        // Using package_with_version() here would apply overrides for the current host
        // platform first, which can leak host-specific overrides into cross-platform lock.
        let pkg = AQUA_REGISTRY.package(&self.id).await?;
        let opts = tv.request.options();
        let target_libc = Self::target_variant_libc(target);
        let pkg = pkg.with_version_libc(&versions, target_os, target_arch, target_libc.as_deref());
        let pkg = Self::apply_aqua_libc_replacement(pkg, target_os, Self::target_libc(target));
        let pkg = Self::apply_var_options(pkg, &opts)?;

        // Apply version prefix if present
        if let Some(prefix) = &pkg.version_prefix
            && !v.starts_with(prefix)
        {
            v = format!("{prefix}{v}");
            v_prefixed = v_prefixed.map(|vp| {
                if vp.starts_with(prefix) {
                    vp
                } else {
                    format!("{prefix}{vp}")
                }
            });
        }

        // Check if this platform is supported
        if !is_platform_supported(&pkg.supported_envs, target_os, target_arch) {
            debug!(
                "aqua package {} does not support {}: supported_envs={:?}",
                self.id,
                target.to_key(),
                pkg.supported_envs
            );
            return Ok(PlatformInfo::default());
        }

        // Get URL and checksum for the target platform
        let (url, checksum) = match pkg.r#type {
            AquaPackageType::GithubRelease => {
                // Try v-prefixed version first (most aqua packages use v-prefixed tags),
                // then fall back to the non-prefixed version.
                let candidates: Vec<&str> = match &v_prefixed {
                    Some(vp) => vec![vp.as_str(), v.as_str()],
                    None => vec![v.as_str()],
                };
                let mut result = (None, None);
                for candidate in &candidates {
                    let asset_strs = pkg.asset_strs(candidate, target_os, target_arch)?;
                    match self
                        .github_release_asset_for_target(&pkg, candidate, asset_strs, target)
                        .await
                    {
                        Ok((url, digest)) => {
                            v = candidate.to_string();
                            result = (Some(url), digest);
                            break;
                        }
                        Err(e) => {
                            debug!(
                                "Failed to get GitHub release asset for {} on {}: {}",
                                self.id,
                                target.to_key(),
                                e
                            );
                        }
                    }
                }
                result
            }
            AquaPackageType::GithubArchive | AquaPackageType::GithubContent => {
                (Some(self.github_archive_url(&pkg, &v)), None)
            }
            AquaPackageType::Http => (pkg.url(&v, target_os, target_arch).ok(), None),
            _ => (None, None),
        };

        let name = url.as_ref().map(|u| get_filename_from_url(u));

        // Try to get checksum from checksum file if not available from GitHub API
        let checksum = match checksum {
            Some(c) => Some(c),
            None => self
                .fetch_checksum_from_file(&pkg, &v, target_os, target_arch, name.as_deref())
                .await
                .ok()
                .flatten(),
        };

        // Detect provenance from aqua registry config
        let mut provenance = self.detect_provenance_type(&pkg);
        let mut github_attestations = None;

        if matches!(provenance, Some(ProvenanceType::GithubAttestations))
            && let Some(digest) = checksum.as_deref().filter(|d| d.starts_with("sha256:"))
        {
            match self.detect_github_attestations(&pkg, digest).await {
                Ok(true) => {}
                Ok(false) => {
                    github_attestations = Some(GithubAttestationsStatus::Unavailable);
                    provenance = self.detect_non_github_provenance_type(&pkg);
                }
                Err(e) => {
                    warn!(
                        "GitHub attestation API query failed for {}/{}: {e}. \
                         Lockfile may not record github-attestations provenance.",
                        pkg.repo_owner, pkg.repo_name
                    );
                }
            }
        }

        // Resolve SLSA provenance URL for all platforms (not just current).
        // This ensures deterministic lockfile output regardless of host platform.
        if matches!(provenance, Some(ProvenanceType::Slsa { url: None })) {
            match self
                .resolve_slsa_url(&pkg, &v, target_os, target_arch)
                .await
            {
                Ok(resolved_url) => {
                    provenance = Some(ProvenanceType::Slsa {
                        url: Some(resolved_url),
                    });
                }
                Err(e) => {
                    warn!(
                        "failed to resolve SLSA provenance URL for {} ({}-{}), \
                         lockfile entry will use short form: {e}",
                        self.id, target_os, target_arch
                    );
                }
            }
        }

        // For the current platform, verify provenance cryptographically at lock time.
        // This ensures the lockfile's provenance entry is backed by actual verification,
        // not just registry metadata. Cross-platform entries remain detection-only.
        if provenance.is_some()
            && target.is_current()
            && let Some(ref artifact_url) = url
        {
            match self
                .verify_provenance_at_lock_time(
                    &pkg,
                    &v,
                    artifact_url,
                    provenance.as_ref().unwrap(),
                )
                .await
            {
                Ok((verified, gh_status)) => {
                    provenance = verified;
                    github_attestations = gh_status;
                }
                Err(e) => {
                    // Clear provenance so install-time verification will run.
                    // If we kept the unverified provenance, has_lockfile_integrity
                    // would be true and verify_provenance() would be skipped.
                    warn!(
                        "lock-time provenance verification failed for {}, \
                         will be verified at install time: {e}",
                        self.id
                    );
                    provenance = None;
                }
            }
        }
        if provenance.is_some() {
            github_attestations = None;
        }

        Ok(PlatformInfo {
            url,
            checksum,
            provenance,
            github_attestations,
            ..Default::default()
        })
    }
}

impl AquaBackend {
    async fn package_with_options(
        &self,
        tv: &ToolVersion,
        versions: &[&str],
    ) -> Result<AquaPackage> {
        let target = PlatformTarget::from_current();
        let (target_os, target_arch) = Self::to_aqua_platform(&target);
        let pkg = AQUA_REGISTRY.package(&self.id).await?;
        let target_libc = Self::target_variant_libc(&target);
        let pkg = pkg.with_version_libc(versions, target_os, target_arch, target_libc.as_deref());
        let pkg = Self::apply_aqua_libc_replacement(pkg, target_os, Self::target_libc(&target));
        Self::apply_var_options(pkg, &tv.request.options())
    }

    fn to_aqua_platform(target: &PlatformTarget) -> (&str, &str) {
        let target_os = match target.os_name() {
            "macos" => "darwin",
            other => other,
        };
        let target_arch = match target.arch_name() {
            "x64" => "amd64",
            other => other,
        };
        (target_os, target_arch)
    }

    fn target_libc(target: &PlatformTarget) -> Option<String> {
        target.libc().map(str::to_string).or_else(|| {
            if target.is_current() {
                Settings::get().libc().map(str::to_string)
            } else {
                None
            }
        })
    }

    fn target_variant_libc(target: &PlatformTarget) -> Option<String> {
        if target.os_name() != "linux" {
            return None;
        }
        let settings_libc = if target.is_current() {
            Settings::get().libc().map(str::to_string)
        } else {
            None
        };
        Some(
            target
                .libc()
                .map(str::to_string)
                .or(settings_libc)
                .unwrap_or_else(|| "gnu".to_string()),
        )
    }

    fn apply_aqua_libc_replacement(
        mut pkg: AquaPackage,
        target_os: &str,
        libc: Option<String>,
    ) -> AquaPackage {
        let Some(libc) = libc else {
            return pkg;
        };
        if target_os != "linux" {
            return pkg;
        }
        let Some(linux) = pkg.replacements.get_mut("linux") else {
            return pkg;
        };
        if is_aqua_linux_libc_replacement(linux) {
            let libc = if libc == "musl" { "musl" } else { "gnu" };
            let prefix = linux
                .strip_suffix("-gnu")
                .or_else(|| linux.strip_suffix("-musl"))
                .unwrap_or("unknown-linux");
            *linux = format!("{prefix}-{libc}");
        }
        pkg
    }

    fn apply_var_options(pkg: AquaPackage, opts: &ToolVersionOptions) -> Result<AquaPackage> {
        if pkg.vars.is_empty() {
            return Ok(pkg);
        }
        let mut var_values = HashMap::new();
        for var in &pkg.vars {
            if let Some(value) = aqua_var_option(opts, &var.name)? {
                var_values.insert(var.name.clone(), value);
            }
        }
        pkg.with_var_values(var_values)
    }

    fn lockfile_options(opts: &ToolVersionOptions) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        for (key, value) in opts.iter() {
            if key == "symlink_bins" || EPHEMERAL_OPT_KEYS.contains(&key.as_str()) {
                continue;
            }
            if key == "vars" {
                if let toml::Value::Table(table) = value {
                    Self::insert_vars_lockfile_options(&mut result, table);
                }
            } else if let Some(value) = toml_value_to_string(value) {
                let key = if key.starts_with("vars.") {
                    key.clone()
                } else {
                    format!("vars.{key}")
                };
                result.entry(key).or_insert(value);
            }
        }
        result
    }

    fn insert_vars_lockfile_options(result: &mut BTreeMap<String, String>, table: &toml::Table) {
        for (key, value) in table {
            if let Some(value) = toml_value_to_string(value) {
                result.insert(format!("vars.{key}"), value);
            }
        }
    }

    fn has_native_cosign(cosign: &AquaCosign) -> bool {
        cosign.enabled != Some(false) && (cosign.key.is_some() || cosign.bundle.is_some())
    }

    fn binary_cosign_config(pkg: &AquaPackage) -> Option<&AquaCosign> {
        pkg.cosign
            .as_ref()
            .filter(|cosign| Self::has_native_cosign(cosign))
    }

    fn checksum_cosign_config(pkg: &AquaPackage) -> Option<(&AquaChecksum, &AquaCosign)> {
        let checksum = pkg
            .checksum
            .as_ref()
            .filter(|checksum| checksum.enabled())?;
        let cosign = checksum
            .cosign
            .as_ref()
            .filter(|cosign| Self::has_native_cosign(cosign))?;
        Some((checksum, cosign))
    }

    /// Detect provenance type from aqua registry package config.
    ///
    /// Returns the highest-priority provenance type that is configured and
    /// enabled for the package, based on the verified `ProvenanceType` priority
    /// order: GithubAttestations > Slsa > Cosign > Minisign.
    ///
    /// This detection is based on registry metadata only — no cryptographic
    /// verification happens here. Actual verification occurs at install time
    /// (and is always performed when `locked_verify_provenance` or `paranoid`
    /// is enabled).
    fn detect_provenance_type(&self, pkg: &AquaPackage) -> Option<ProvenanceType> {
        let settings = Settings::get();

        // Check for GitHub artifact attestations (highest priority)
        // The registry metadata (enabled flag, signer_workflow) is sufficient for
        // detection at lock-time. Actual cryptographic verification happens at
        // install time (always when locked_verify_provenance/paranoid is enabled,
        // or on first install when the lockfile doesn't yet have provenance).
        if settings.github_attestations
            && settings.aqua.github_attestations
            && let Some(att) = &pkg.github_artifact_attestations
            && att.enabled != Some(false)
        {
            return Some(ProvenanceType::GithubAttestations);
        }

        self.detect_non_github_provenance_type(pkg)
    }

    fn detect_non_github_provenance_type(&self, pkg: &AquaPackage) -> Option<ProvenanceType> {
        let settings = Settings::get();

        // Check for SLSA provenance
        if settings.slsa
            && settings.aqua.slsa
            && let Some(slsa) = &pkg.slsa_provenance
            && slsa.enabled != Some(false)
        {
            return Some(ProvenanceType::Slsa { url: None });
        }

        // Check for cosign.
        // Only record cosign provenance if we can actually verify it natively
        // (key-based or bundle-based). Tools that only use opts require the external
        // cosign CLI which we don't shell out to.
        if settings.aqua.cosign
            && (Self::binary_cosign_config(pkg).is_some()
                || Self::checksum_cosign_config(pkg).is_some())
        {
            return Some(ProvenanceType::Cosign);
        }

        // Check for minisign
        if settings.aqua.minisign
            && let Some(minisign) = &pkg.minisign
            && minisign.enabled != Some(false)
        {
            return Some(ProvenanceType::Minisign);
        }

        None
    }

    async fn detect_github_attestations(&self, pkg: &AquaPackage, digest: &str) -> Result<bool> {
        crate::github::sigstore::detect_attestations(
            &pkg.repo_owner,
            &pkg.repo_name,
            github::API_URL,
            digest,
        )
        .await
        .map_err(|e| eyre!("{e}"))
    }

    /// Verify provenance at lock time by downloading the artifact to a temp directory
    /// and running the appropriate cryptographic verification. Only called for the
    /// current platform during `mise lock`.
    async fn verify_provenance_at_lock_time(
        &self,
        pkg: &AquaPackage,
        v: &str,
        artifact_url: &str,
        detected: &ProvenanceType,
    ) -> Result<(Option<ProvenanceType>, Option<GithubAttestationsStatus>)> {
        let tmp_dir = tempfile::tempdir()?;
        let filename = get_filename_from_url(artifact_url);
        let artifact_path = tmp_dir.path().join(&filename);

        info!(
            "downloading artifact for lock-time provenance verification: {}",
            filename
        );
        HTTP.download_file(artifact_url, &artifact_path, None)
            .await?;

        match detected {
            ProvenanceType::GithubAttestations => {
                match self
                    .run_github_attestation_check(&artifact_path, pkg)
                    .await?
                {
                    GithubAttestationStatus::Verified => {
                        Ok((Some(ProvenanceType::GithubAttestations), None))
                    }
                    GithubAttestationStatus::Unavailable => {
                        Ok((None, Some(GithubAttestationsStatus::Unavailable)))
                    }
                }
            }
            ProvenanceType::Slsa { .. } => {
                let provenance_url = self
                    .run_slsa_check(&artifact_path, pkg, v, tmp_dir.path(), None)
                    .await?;
                Ok((
                    Some(ProvenanceType::Slsa {
                        url: Some(provenance_url),
                    }),
                    None,
                ))
            }
            ProvenanceType::Minisign => {
                self.run_minisign_check(&artifact_path, &filename, pkg, v, tmp_dir.path(), None)
                    .await?;
                Ok((Some(ProvenanceType::Minisign), None))
            }
            ProvenanceType::Cosign => {
                if let Some(cosign) = Self::binary_cosign_config(pkg) {
                    self.run_cosign_check(&artifact_path, cosign, pkg, v, tmp_dir.path(), None)
                        .await?;
                } else {
                    let (checksum_config, cosign) = Self::checksum_cosign_config(pkg).wrap_err(
                        "cosign provenance detected but no supported binary/checksum config found",
                    )?;
                    let checksum_path = self
                        .download_checksum_file(checksum_config, pkg, v, tmp_dir.path(), None)
                        .await?;
                    self.run_cosign_check(&checksum_path, cosign, pkg, v, tmp_dir.path(), None)
                        .await?;
                }
                Ok((Some(ProvenanceType::Cosign), None))
            }
        }
    }

    // --- Shared verification helpers used by both lock-time and install-time ---

    /// Run GitHub artifact attestation verification against an already-downloaded artifact.
    async fn run_github_attestation_check(
        &self,
        artifact_path: &Path,
        pkg: &AquaPackage,
    ) -> Result<GithubAttestationStatus> {
        // The aqua registry stores signer_workflow as a regex pattern (e.g. `\.github/workflows/release\.yaml`).
        // sigstore-verification's verify_attestations() uses plain str::contains(), not regex, so we must
        // unescape regex metacharacter escapes (e.g. `\.` → `.`) before passing the value through.
        let signer_workflow = pkg
            .github_artifact_attestations
            .as_ref()
            .and_then(|att| att.signer_workflow.as_deref().map(unescape_regex_literal));

        match crate::github::sigstore::verify_attestation(
            artifact_path,
            &pkg.repo_owner,
            &pkg.repo_name,
            signer_workflow.as_deref(),
            None,
        )
        .await
        {
            Ok(true) => {
                debug!(
                    "GitHub attestations verified for {}/{}",
                    pkg.repo_owner, pkg.repo_name
                );
                Ok(GithubAttestationStatus::Verified)
            }
            Ok(false) => Err(eyre!(
                "GitHub artifact attestations verification returned false"
            )),
            Err(crate::github::sigstore::AttestationError::NoAttestations) => {
                Ok(GithubAttestationStatus::Unavailable)
            }
            Err(e) => Err(eyre!(
                "GitHub artifact attestations verification failed: {e}"
            )),
        }
    }

    /// Resolve the SLSA provenance URL for a target platform without downloading.
    /// Uses cached GitHub release data or template-based URL construction.
    async fn resolve_slsa_url(
        &self,
        pkg: &AquaPackage,
        v: &str,
        target_os: &str,
        target_arch: &str,
    ) -> Result<String> {
        let slsa = pkg
            .slsa_provenance
            .as_ref()
            .wrap_err("SLSA provenance detected but no config found")?;

        let mut slsa_pkg = pkg.clone();
        (slsa_pkg.repo_owner, slsa_pkg.repo_name) =
            resolve_repo_info(slsa.repo_owner.as_ref(), slsa.repo_name.as_ref(), pkg);

        match slsa.r#type.as_deref().unwrap_or_default() {
            "github_release" => {
                let asset_strs = slsa.asset_strs(&slsa_pkg, v, target_os, target_arch)?;
                let (url, _) = self.github_release_asset(&slsa_pkg, v, asset_strs).await?;
                Ok(url)
            }
            "http" => slsa.url(&slsa_pkg, v, target_os, target_arch),
            t => Err(eyre!("unsupported slsa type: {t}")),
        }
    }

    /// Download SLSA provenance file and verify against an already-downloaded artifact.
    /// Returns the provenance download URL on success.
    async fn run_slsa_check(
        &self,
        artifact_path: &Path,
        pkg: &AquaPackage,
        v: &str,
        download_dir: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<String> {
        let provenance_url = self.resolve_slsa_url(pkg, v, os(), arch()).await?;
        let provenance_path = download_dir.join(get_filename_from_url(&provenance_url));
        HTTP.download_file(&provenance_url, &provenance_path, pr)
            .await?;

        match crate::github::sigstore::verify_slsa_provenance(artifact_path, &provenance_path, 1u8)
            .await
        {
            Ok(true) => {
                debug!("SLSA provenance verified");
                Ok(provenance_url)
            }
            Ok(false) => Err(eyre!("SLSA provenance verification failed")),
            Err(e) => Err(e.into()),
        }
    }

    /// Download minisign signature and verify against an already-downloaded artifact.
    async fn run_minisign_check(
        &self,
        artifact_path: &Path,
        artifact_filename: &str,
        pkg: &AquaPackage,
        v: &str,
        download_dir: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let minisign_config = pkg
            .minisign
            .as_ref()
            .wrap_err("minisign provenance detected but no config found")?;

        let sig_path = match minisign_config._type() {
            AquaMinisignType::GithubRelease => {
                let asset = minisign_config.asset(pkg, v, os(), arch())?;
                let (repo_owner, repo_name) = resolve_repo_info(
                    minisign_config.repo_owner.as_ref(),
                    minisign_config.repo_name.as_ref(),
                    pkg,
                );
                let url = github::get_release(&format!("{repo_owner}/{repo_name}"), v)
                    .await?
                    .assets
                    .into_iter()
                    .find(|a| a.name == asset)
                    .map(|a| a.browser_download_url)
                    .wrap_err_with(|| format!("no asset found for minisign: {asset}"))?;
                let path = download_dir.join(&asset);
                HTTP.download_file(&url, &path, pr).await?;
                path
            }
            AquaMinisignType::Http => {
                let url = minisign_config.url(pkg, v, os(), arch())?;
                let path = download_dir.join(format!("{artifact_filename}.minisig"));
                HTTP.download_file(&url, &path, pr).await?;
                path
            }
        };
        let data = file::read(artifact_path)?;
        let sig = file::read_to_string(&sig_path)?;
        minisign::verify(
            &minisign_config.public_key(pkg, v, os(), arch())?,
            &data,
            &sig,
        )?;
        debug!("minisign verified");
        Ok(())
    }

    /// Download cosign key/signature/bundle and verify a target file.
    async fn run_cosign_check(
        &self,
        target_path: &Path,
        cosign: &AquaCosign,
        pkg: &AquaPackage,
        v: &str,
        download_dir: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<()> {
        if let Some(key) = &cosign.key {
            let mut key_pkg = pkg.clone();
            (key_pkg.repo_owner, key_pkg.repo_name) =
                resolve_repo_info(key.repo_owner.as_ref(), key.repo_name.as_ref(), pkg);
            let key_url = match key.r#type.as_deref().unwrap_or_default() {
                "github_release" => {
                    let asset_strs = key.asset_strs(pkg, v, os(), arch())?;
                    self.github_release_asset(&key_pkg, v, asset_strs).await?.0
                }
                "http" => key.url(pkg, v, os(), arch())?,
                t => return Err(eyre!("unsupported cosign key type: {t}")),
            };
            let key_path = download_dir.join(get_filename_from_url(&key_url));
            HTTP.download_file(&key_url, &key_path, pr).await?;

            let sig_path = if let Some(signature) = &cosign.signature {
                let mut sig_pkg = pkg.clone();
                (sig_pkg.repo_owner, sig_pkg.repo_name) = resolve_repo_info(
                    signature.repo_owner.as_ref(),
                    signature.repo_name.as_ref(),
                    pkg,
                );
                let sig_url = match signature.r#type.as_deref().unwrap_or_default() {
                    "github_release" => {
                        let asset_strs = signature.asset_strs(pkg, v, os(), arch())?;
                        self.github_release_asset(&sig_pkg, v, asset_strs).await?.0
                    }
                    "http" => signature.url(pkg, v, os(), arch())?,
                    t => return Err(eyre!("unsupported cosign signature type: {t}")),
                };
                let path = download_dir.join(get_filename_from_url(&sig_url));
                HTTP.download_file(&sig_url, &path, pr).await?;
                path
            } else {
                target_path.with_extension("sig")
            };

            match crate::github::sigstore::verify_cosign_signature_with_key(
                target_path,
                &sig_path,
                &key_path,
            )
            .await
            {
                Ok(true) => {
                    debug!("cosign (key) verified");
                    Ok(())
                }
                Ok(false) => Err(eyre!("cosign key-based verification returned false")),
                Err(e) => Err(eyre!("cosign key-based verification failed: {e}")),
            }
        } else if let Some(bundle) = &cosign.bundle {
            let mut bundle_pkg = pkg.clone();
            (bundle_pkg.repo_owner, bundle_pkg.repo_name) =
                resolve_repo_info(bundle.repo_owner.as_ref(), bundle.repo_name.as_ref(), pkg);
            let bundle_url = match bundle.r#type.as_deref().unwrap_or_default() {
                "github_release" => {
                    let asset_strs = bundle.asset_strs(pkg, v, os(), arch())?;
                    self.github_release_asset(&bundle_pkg, v, asset_strs)
                        .await?
                        .0
                }
                "http" => bundle.url(pkg, v, os(), arch())?,
                t => return Err(eyre!("unsupported cosign bundle type: {t}")),
            };
            let bundle_path = download_dir.join(get_filename_from_url(&bundle_url));
            HTTP.download_file(&bundle_url, &bundle_path, pr).await?;

            match crate::github::sigstore::verify_cosign_signature(target_path, &bundle_path).await
            {
                Ok(true) => {
                    debug!("cosign (bundle) verified");
                    Ok(())
                }
                Ok(false) => Err(eyre!("cosign bundle-based verification returned false")),
                Err(e) => Err(eyre!("cosign bundle-based verification failed: {e}")),
            }
        } else {
            Err(eyre!("cosign detected but no key or bundle configured"))
        }
    }

    /// Download checksum file to the given directory.
    async fn download_checksum_file(
        &self,
        checksum: &AquaChecksum,
        pkg: &AquaPackage,
        v: &str,
        download_dir: &Path,
        pr: Option<&dyn SingleReport>,
    ) -> Result<PathBuf> {
        let url = match checksum._type() {
            AquaChecksumType::GithubRelease => {
                let asset_strs = checksum.asset_strs(pkg, v, os(), arch())?;
                self.github_release_asset(pkg, v, asset_strs).await?.0
            }
            AquaChecksumType::Http => checksum.url(pkg, v, os(), arch())?,
        };
        let path = download_dir.join(get_filename_from_url(&url));
        HTTP.download_file(&url, &path, pr).await?;
        Ok(path)
    }

    pub fn from_arg(ba: BackendArg) -> Self {
        let full = ba.full_without_opts();
        let mut id = full.split_once(":").unwrap_or(("", &full)).1;
        if !id.contains("/") {
            id = REGISTRY
                .get(id)
                .and_then(|t| t.backends.iter().find_map(|s| s.full.strip_prefix("aqua:")))
                .unwrap_or_else(|| {
                    warn!("invalid aqua tool: {}", id);
                    id
                });
        }
        let cache_path = ba.cache_path.clone();
        Self {
            id: id.to_string(),
            ba: Arc::new(ba),
            // Bumped from `version_tags.msgpack.z`: this cache used to be filtered
            // by the inline `prerelease` opt, so previously cached lists could be
            // missing pre-release tags needed at install/lock time. The new cache
            // always stores the superset.
            version_tags_cache: CacheManagerBuilder::new(
                cache_path.join("version_tags_v2.msgpack.z"),
            )
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .build(),
        }
    }

    async fn latest_marked_release_version(&self) -> Result<Option<String>> {
        if Settings::get().offline() {
            trace!("Skipping latest stable version due to offline mode");
            return Ok(None);
        }

        let pkg = match AQUA_REGISTRY.package(&self.id).await {
            Ok(pkg) => pkg,
            Err(e) => {
                warn!("Latest version cannot be fetched: {}", e);
                return Ok(None);
            }
        };

        if pkg.repo_owner.is_empty() || pkg.repo_name.is_empty() {
            warn!(
                "aqua package {} does not have repo_owner and/or repo_name.",
                self.id
            );
            return Ok(None);
        }

        if pkg.version_source.as_deref() == Some("github_tag") {
            return Ok(None);
        }

        let repo = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let release = match github::get_release(&repo, "latest").await {
            Ok(release) => release,
            Err(e) => {
                debug!(
                    "Failed to fetch latest GitHub release for aqua package {}: {e}",
                    self.id
                );
                return Ok(None);
            }
        };

        let target = PlatformTarget::from_current();
        let (target_os, target_arch) = Self::to_aqua_platform(&target);
        let target_libc = Self::target_variant_libc(&target);
        match versioned_package_from_tag(
            &pkg,
            &release.tag_name,
            target_os,
            target_arch,
            target_libc.as_deref(),
        ) {
            Ok(Some((version, versioned_pkg))) if package_has_asset(&versioned_pkg) => {
                Ok(Some(version))
            }
            Ok(Some(_)) | Ok(None) => Ok(None),
            Err(e) => {
                debug!(
                    "Failed to resolve latest GitHub release tag for aqua package {}: {e}",
                    self.id
                );
                Ok(None)
            }
        }
    }

    async fn get_version_tags(&self) -> Result<&Vec<(String, String)>> {
        self.version_tags_cache
            .get_or_try_init_async(|| async {
                let pkg = AQUA_REGISTRY.package(&self.id).await?;
                let mut versions = Vec::new();
                if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
                    // Always fetch the superset; install/lock resolution needs
                    // every tag (including pre-releases) regardless of the
                    // current `prerelease` opt, since the user may have pinned
                    // a pre-release version under a project-local override.
                    let tags = get_tags(&pkg).await?;
                    let target = PlatformTarget::from_current();
                    let (target_os, target_arch) = Self::to_aqua_platform(&target);
                    let target_libc = Self::target_variant_libc(&target);
                    for tag in tags.into_iter().rev() {
                        let (version, _) = match versioned_package_from_tag(
                            &pkg,
                            &tag,
                            target_os,
                            target_arch,
                            target_libc.as_deref(),
                        ) {
                            Ok(Some(versioned)) => versioned,
                            Ok(None) => continue,
                            Err(e) => {
                                warn!("[{}] aqua version filter error: {e}", self.ba());
                                continue;
                            }
                        };
                        versions.push((version, tag));
                    }
                } else {
                    bail!(
                        "aqua package {} does not have repo_owner and/or repo_name.",
                        self.id
                    );
                }
                Ok(versions)
            })
            .await
    }

    async fn get_url(&self, pkg: &AquaPackage, v: &str) -> Result<(String, bool, Option<String>)> {
        match pkg.r#type {
            AquaPackageType::GithubRelease => self
                .github_release_url(pkg, v)
                .await
                .map(|(url, digest)| (url, true, digest)),
            AquaPackageType::GithubContent => {
                if pkg.path.is_some() {
                    Ok((self.github_content_url(pkg, v), false, None))
                } else {
                    bail!("github_content package requires `path`")
                }
            }
            AquaPackageType::GithubArchive => Ok((self.github_archive_url(pkg, v), false, None)),
            AquaPackageType::Http => pkg.url(v, os(), arch()).map(|url| (url, false, None)),
            ref t => bail!("unsupported aqua package type: {t}"),
        }
    }

    async fn github_release_url(
        &self,
        pkg: &AquaPackage,
        v: &str,
    ) -> Result<(String, Option<String>)> {
        let target = PlatformTarget::from_current();
        let asset_strs = pkg.asset_strs(v, os(), arch())?;
        self.github_release_asset_for_target(pkg, v, asset_strs, &target)
            .await
    }

    async fn github_release_asset(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
    ) -> Result<(String, Option<String>)> {
        self.github_release_asset_matching(pkg, v, asset_strs, false)
            .await
    }

    async fn github_release_asset_for_target(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
        target: &PlatformTarget,
    ) -> Result<(String, Option<String>)> {
        // TODO: remove this when aqua supports musl variants natively.
        // For now aqua templates only see linux/amd64 or linux/arm64, so a
        // linux-*-musl lock target would otherwise choose the glibc asset even
        // when a release also publishes the same asset name with an added musl
        // token.
        self.github_release_asset_matching(pkg, v, asset_strs, target_prefers_musl(target))
            .await
    }

    async fn github_release_asset_matching(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
        prefer_musl: bool,
    ) -> Result<(String, Option<String>)> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let gh_release = github::get_release(&gh_id, v).await?;

        // Prioritize order of asset_strs
        let asset = select_github_release_asset(&gh_release.assets, &asset_strs, prefer_musl)
            .wrap_err_with(|| {
                format!(
                    "no asset found: {}\nAvailable assets:\n{}",
                    asset_strs.iter().join(", "),
                    gh_release.assets.iter().map(|a| &a.name).join("\n")
                )
            })?;

        Ok((asset.browser_download_url.to_string(), asset.digest.clone()))
    }

    fn github_archive_url(&self, pkg: &AquaPackage, v: &str) -> String {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        format!("https://github.com/{gh_id}/archive/refs/tags/{v}.tar.gz")
    }

    fn github_content_url(&self, pkg: &AquaPackage, v: &str) -> String {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let path = pkg.path.as_deref().unwrap();
        format!("https://raw.githubusercontent.com/{gh_id}/{v}/{path}")
    }

    /// Fetch checksum from a checksum file without downloading the actual tarball.
    /// This is used for cross-platform lockfile generation.
    async fn fetch_checksum_from_file(
        &self,
        pkg: &AquaPackage,
        v: &str,
        target_os: &str,
        target_arch: &str,
        filename: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(checksum_config) = &pkg.checksum else {
            return Ok(None);
        };
        if !checksum_config.enabled() {
            return Ok(None);
        }
        let Some(filename) = filename else {
            return Ok(None);
        };

        // Get the checksum file URL
        let url = match checksum_config._type() {
            AquaChecksumType::GithubRelease => {
                let asset_strs = checksum_config.asset_strs(pkg, v, target_os, target_arch)?;
                match self.github_release_asset(pkg, v, asset_strs).await {
                    Ok((url, _)) => url,
                    Err(e) => {
                        debug!("Failed to get checksum file asset: {}", e);
                        return Ok(None);
                    }
                }
            }
            AquaChecksumType::Http => checksum_config.url(pkg, v, target_os, target_arch)?,
        };

        // Download checksum file content
        let checksum_content = match HTTP.get_text(&url).await {
            Ok(content) => content,
            Err(e) => {
                debug!("Failed to download checksum file {}: {}", url, e);
                return Ok(None);
            }
        };

        // Parse checksum from file content
        let checksum_str =
            self.parse_checksum_from_content(&checksum_content, checksum_config, filename)?;

        Ok(Some(format!(
            "{}:{}",
            checksum_config.algorithm(),
            checksum_str
        )))
    }

    /// Parse a checksum from checksum file content for a specific filename.
    fn parse_checksum_from_content(
        &self,
        content: &str,
        checksum_config: &AquaChecksum,
        filename: &str,
    ) -> Result<String> {
        let mut checksum_file = content.to_string();

        if checksum_config.file_format() == "regexp" {
            let pattern = checksum_config.pattern();
            if let Some(file_pattern) = &pattern.file {
                let re = regex::Regex::new(file_pattern.as_str())?;
                if let Some(line) = checksum_file
                    .lines()
                    .find(|l| re.captures(l).is_some_and(|c| c[1].to_string() == filename))
                {
                    checksum_file = line.to_string();
                } else {
                    debug!(
                        "no line found matching {} in checksum file for {}",
                        file_pattern, filename
                    );
                }
            }
            let re = regex::Regex::new(pattern.checksum.as_str())?;
            if let Some(caps) = re.captures(checksum_file.as_str()) {
                checksum_file = caps[1].to_string();
            } else {
                debug!(
                    "no checksum found matching {} in checksum file",
                    pattern.checksum
                );
            }
        }

        // Standard format: "<hash>  <filename>" or "<hash> *<filename>"
        let checksum_str = checksum_file
            .lines()
            .filter_map(|l| {
                let split = l.split_whitespace().collect_vec();
                if split.len() == 2 {
                    Some((
                        split[0].to_string(),
                        split[1]
                            .rsplit_once('/')
                            .map(|(_, f)| f)
                            .unwrap_or(split[1])
                            .trim_matches('*')
                            .to_string(),
                    ))
                } else {
                    None
                }
            })
            .find(|(_, f)| f == filename)
            .map(|(c, _)| c)
            .unwrap_or(checksum_file);

        let checksum_str = checksum_str
            .split_whitespace()
            .next()
            .unwrap_or(&checksum_str);
        Ok(checksum_str.to_string())
    }

    async fn download(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        url: &str,
        filename: &str,
    ) -> Result<()> {
        let tarball_path = tv.download_path().join(filename);
        if tarball_path.exists() {
            return Ok(());
        }
        ctx.pr.set_message(format!("download {filename}"));
        HTTP.download_file(url, &tarball_path, Some(ctx.pr.as_ref()))
            .await?;
        Ok(())
    }

    async fn verify(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        // Skip provenance verification if the lockfile already has both a checksum and
        // provenance entry for this platform — the artifact integrity is already guaranteed
        // by the checksum, so re-verifying attestations would just be redundant API calls.
        // However, still check that the recorded provenance type's setting is enabled —
        // disabling a verification setting with a provenance-bearing lockfile is a downgrade.
        //
        // When locked_verify_provenance is enabled (or paranoid mode is on), always
        // re-verify provenance at install time regardless of what the lockfile contains.
        // This closes the gap where lock-time detection records provenance from registry
        // metadata without cryptographic verification.
        let settings = Settings::get();
        let force_verify = settings.force_provenance_verify();
        let platform_key = self.get_platform_key();
        let has_lockfile_integrity = tv
            .lock_platforms
            .get(&platform_key)
            .is_some_and(PlatformInfo::has_checksum_and_verified_provenance);
        if has_lockfile_integrity && !force_verify {
            self.ensure_provenance_setting_enabled(tv, &platform_key)?;
        } else {
            self.verify_provenance(ctx, tv, pkg, v, filename).await?;
        }

        let tarball_path = tv.download_path().join(filename);
        self.verify_checksum(ctx, tv, &tarball_path)?;
        Ok(())
    }

    async fn verify_provenance(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        // Check if the lockfile expects provenance for this platform, then clear it
        // so we can detect whether verification actually re-set it
        let platform_key = self.get_platform_key();
        let skip_cached_absent_attestations = !Settings::get().force_provenance_verify()
            && tv
                .lock_platforms
                .get(&platform_key)
                .is_some_and(PlatformInfo::has_checksum_and_github_attestations_unavailable);
        let locked_provenance = tv
            .lock_platforms
            .get_mut(&platform_key)
            .and_then(|pi| pi.provenance.take());
        let expected_provenance = locked_provenance.as_ref();
        let mut github_attestations_unavailable = skip_cached_absent_attestations;

        // When the lockfile specifies a provenance type, only run that specific mechanism.
        // This prevents false-positive downgrade errors when a tool supports multiple mechanisms
        // (e.g., both minisign and cosign) that would otherwise compete for the provenance slot.
        let skip_attestations = skip_cached_absent_attestations
            || expected_provenance.is_some_and(|l| !l.is_github_attestations());
        let skip_slsa = expected_provenance.is_some_and(|l| !l.is_slsa());
        let skip_minisign = expected_provenance.is_some_and(|l| !l.is_minisign());
        let skip_cosign = expected_provenance.is_some_and(|l| !l.is_cosign());

        if !skip_attestations
            && let Some(status) = self
                .verify_github_artifact_attestations(ctx, tv, pkg, v, filename)
                .await?
        {
            match status {
                GithubAttestationStatus::Verified => {
                    let pi = tv.lock_platforms.entry(platform_key.clone()).or_default();
                    if pi.provenance.is_none() {
                        pi.provenance = Some(ProvenanceType::GithubAttestations);
                    }
                }
                GithubAttestationStatus::Unavailable => {
                    github_attestations_unavailable = true;
                }
            }
        }
        if !skip_slsa {
            // Short-circuit: if a higher-priority mechanism already recorded provenance, skip SLSA
            let already_verified = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|pi| pi.provenance.as_ref())
                .is_some_and(|p| *p > ProvenanceType::Slsa { url: None });
            if !already_verified {
                self.verify_slsa(ctx, tv, pkg, v, filename).await?;
            }
        }
        if !skip_minisign {
            // Short-circuit: if SLSA or GithubAttestations already recorded provenance, skip minisign.
            // Cosign runs later, so it cannot be set at this point.
            let already_verified = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|pi| pi.provenance.as_ref())
                .is_some_and(|p| p.is_slsa() || p.is_github_attestations());
            if !already_verified {
                self.verify_minisign(ctx, tv, pkg, v, filename).await?;
            }
        }

        let download_path = tv.download_path();
        let mut cosign_already_verified = tv
            .lock_platforms
            .get(&platform_key)
            .and_then(|pi| pi.provenance.as_ref())
            .is_some_and(|p| *p > ProvenanceType::Cosign);

        if !skip_cosign
            && Settings::get().aqua.cosign
            && !cosign_already_verified
            && let Some(cosign) = Self::binary_cosign_config(pkg)
        {
            let artifact_path = download_path.join(filename);
            self.cosign_artifact(ctx, cosign, pkg, v, tv, &artifact_path)
                .await?;
            cosign_already_verified = true;
        }

        if let Some(checksum) = &pkg.checksum
            && checksum.enabled()
        {
            let checksum_path = download_path.join(format!("{filename}.checksum"));
            let platform_key = self.get_platform_key();
            let needs_checksum = tv
                .lock_platforms
                .get(&platform_key)
                .is_none_or(|pi| pi.checksum.is_none());

            let checksum_cosign = (!skip_cosign && Settings::get().aqua.cosign)
                .then(|| Self::checksum_cosign_config(pkg).map(|(_, cosign)| cosign))
                .flatten();
            let needs_cosign = checksum_cosign.is_some();
            // Re-download only if the checksum file doesn't exist yet. An existing file
            // from a prior attempt is trusted because the download directory is version-specific
            // and the final artifact is independently verified by verify_checksum at the end.
            if (needs_checksum || (needs_cosign && !cosign_already_verified))
                && !checksum_path.exists()
            {
                let url = match checksum._type() {
                    AquaChecksumType::GithubRelease => {
                        let asset_strs = checksum.asset_strs(pkg, v, os(), arch())?;
                        self.github_release_asset(pkg, v, asset_strs).await?.0
                    }
                    AquaChecksumType::Http => checksum.url(pkg, v, os(), arch())?,
                };
                HTTP.download_file(&url, &checksum_path, Some(ctx.pr.as_ref()))
                    .await?;
            }

            if let Some(cosign) = checksum_cosign
                && !cosign_already_verified
                && checksum_path.exists()
            {
                self.cosign_checksums(ctx, cosign, pkg, v, tv, &checksum_path)
                    .await?;
            }

            if needs_checksum && checksum_path.exists() {
                let checksum_content = file::read_to_string(&checksum_path)?;
                let checksum_str =
                    self.parse_checksum_from_content(&checksum_content, checksum, filename)?;
                let checksum_val = format!("{}:{}", checksum.algorithm(), checksum_str);
                let platform_key = self.get_platform_key();
                let platform_info = tv.lock_platforms.entry(platform_key).or_default();
                platform_info.checksum = Some(checksum_val);
            }
        }
        if github_attestations_unavailable {
            let platform_key = self.get_platform_key();
            if let Some(pi) = tv.lock_platforms.get_mut(&platform_key)
                && pi.checksum.is_some()
                && pi.provenance.is_none()
            {
                pi.github_attestations = Some(GithubAttestationsStatus::Unavailable);
            }
        }
        if let Some(pi) = tv.lock_platforms.get_mut(&platform_key)
            && pi.provenance.is_some()
        {
            pi.github_attestations = None;
        }

        // If lockfile recorded verified provenance, verify that the type matches
        // (checked after all verification methods including cosign have had a chance to record)
        if let Some(expected) = expected_provenance {
            let platform_key = self.get_platform_key();
            let got = tv
                .lock_platforms
                .get(&platform_key)
                .and_then(|pi| pi.provenance.as_ref());
            if !got.is_some_and(|g| std::mem::discriminant(g) == std::mem::discriminant(expected)) {
                let got_str = got
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "no verification".to_string());
                return Err(eyre!(
                    "Lockfile requires {expected} provenance for {tv} but {got_str} was used. \
                     This may indicate a downgrade attack. Enable the corresponding verification setting \
                     or update the lockfile."
                ));
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
            Ok(match provenance {
                ProvenanceType::GithubAttestations => {
                    !settings.github_attestations || !settings.aqua.github_attestations
                }
                ProvenanceType::Slsa { .. } => !settings.slsa || !settings.aqua.slsa,
                ProvenanceType::Cosign => !settings.aqua.cosign,
                ProvenanceType::Minisign => !settings.aqua.minisign,
            })
        })
    }

    async fn verify_minisign(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        if !Settings::get().aqua.minisign {
            return Ok(());
        }
        if let Some(minisign) = &pkg.minisign {
            if minisign.enabled == Some(false) {
                debug!("minisign is disabled for {tv}");
                return Ok(());
            }
            ctx.pr.set_message("verify minisign".to_string());
            debug!("minisign: {:?}", minisign);
            let artifact_path = tv.download_path().join(filename);
            self.run_minisign_check(
                &artifact_path,
                filename,
                pkg,
                v,
                &tv.download_path(),
                Some(ctx.pr.as_ref()),
            )
            .await?;

            // Record minisign provenance if no higher-priority verification already recorded
            let platform_key = self.get_platform_key();
            let pi = tv.lock_platforms.entry(platform_key).or_default();
            if pi.provenance.is_none() {
                pi.provenance = Some(ProvenanceType::Minisign);
            }
        }
        Ok(())
    }

    async fn verify_slsa(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        let settings = Settings::get();
        if !settings.slsa || !settings.aqua.slsa {
            return Ok(());
        }
        if let Some(slsa) = &pkg.slsa_provenance {
            if slsa.enabled == Some(false) {
                debug!("slsa is disabled for {tv}");
                return Ok(());
            }

            ctx.pr.set_message("verify slsa".to_string());
            let artifact_path = tv.download_path().join(filename);
            let provenance_url = self
                .run_slsa_check(
                    &artifact_path,
                    pkg,
                    v,
                    &tv.download_path(),
                    Some(ctx.pr.as_ref()),
                )
                .await?;

            ctx.pr.set_message("✓ SLSA provenance verified".to_string());
            // Record provenance in lockfile only if not already set by a
            // higher-priority verification (github-attestations runs first)
            let platform_key = self.get_platform_key();
            let pi = tv.lock_platforms.entry(platform_key).or_default();
            if pi.provenance.is_none() {
                pi.provenance = Some(ProvenanceType::Slsa {
                    url: Some(provenance_url),
                });
            }
        }
        Ok(())
    }

    async fn verify_github_artifact_attestations(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        pkg: &AquaPackage,
        _v: &str,
        filename: &str,
    ) -> Result<Option<GithubAttestationStatus>> {
        // Check if attestations are enabled via global and aqua-specific settings
        let settings = Settings::get();
        if !settings.github_attestations || !settings.aqua.github_attestations {
            debug!("GitHub artifact attestations verification disabled");
            return Ok(None);
        }

        if let Some(github_attestations) = &pkg.github_artifact_attestations {
            if github_attestations.enabled == Some(false) {
                debug!("GitHub artifact attestations verification is disabled for {tv}");
                return Ok(None);
            }

            ctx.pr
                .set_message("verify GitHub artifact attestations".to_string());
            let artifact_path = tv.download_path().join(filename);
            match self
                .run_github_attestation_check(&artifact_path, pkg)
                .await?
            {
                GithubAttestationStatus::Verified => {}
                GithubAttestationStatus::Unavailable => {
                    return Ok(Some(GithubAttestationStatus::Unavailable));
                }
            }

            ctx.pr
                .set_message("✓ GitHub artifact attestations verified".to_string());
            return Ok(Some(GithubAttestationStatus::Verified));
        }

        Ok(None)
    }

    async fn cosign_artifact(
        &self,
        ctx: &InstallContext,
        cosign: &AquaCosign,
        pkg: &AquaPackage,
        v: &str,
        tv: &mut ToolVersion,
        artifact_path: &Path,
    ) -> Result<()> {
        let download_path = tv.download_path();
        ctx.pr
            .set_message("verify artifact with cosign".to_string());
        self.run_cosign_check(
            artifact_path,
            cosign,
            pkg,
            v,
            &download_path,
            Some(ctx.pr.as_ref()),
        )
        .await?;

        ctx.pr.set_message("✓ Cosign verified".to_string());
        self.record_cosign_provenance(tv);
        Ok(())
    }

    async fn cosign_checksums(
        &self,
        ctx: &InstallContext,
        cosign: &AquaCosign,
        pkg: &AquaPackage,
        v: &str,
        tv: &mut ToolVersion,
        checksum_path: &Path,
    ) -> Result<()> {
        let download_path = tv.download_path();
        ctx.pr
            .set_message("verify checksums with cosign".to_string());
        self.run_cosign_check(
            checksum_path,
            cosign,
            pkg,
            v,
            &download_path,
            Some(ctx.pr.as_ref()),
        )
        .await?;

        ctx.pr.set_message("✓ Cosign verified".to_string());
        self.record_cosign_provenance(tv);
        Ok(())
    }

    fn record_cosign_provenance(&self, tv: &mut ToolVersion) {
        let platform_key = self.get_platform_key();
        let pi = tv.lock_platforms.entry(platform_key).or_default();
        if pi
            .provenance
            .as_ref()
            .is_none_or(|p| *p < ProvenanceType::Cosign)
        {
            pi.provenance = Some(ProvenanceType::Cosign);
        }
    }

    fn install(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        pkg: &AquaPackage,
        v: &str,
        filename: &str,
    ) -> Result<()> {
        let tarball_path = tv.download_path().join(filename);
        ctx.pr.set_message(format!("extract {filename}"));
        let install_path = tv.install_path();
        file::remove_all(&install_path)?;
        let format = pkg.format(v, os(), arch())?;
        let mut bin_names: Vec<Cow<'_, str>> = pkg
            .files
            .iter()
            .filter_map(|file| match file.src(pkg, v, os(), arch()) {
                Ok(Some(s)) => Some(Cow::Owned(s)),
                Ok(None) => Some(Cow::Borrowed(file.name.as_str())),
                Err(_) => None,
            })
            .collect();
        if bin_names.is_empty() {
            let fallback_name = pkg
                .name
                .as_deref()
                .and_then(|n| n.split('/').next_back())
                .unwrap_or(&pkg.repo_name);
            bin_names = vec![Cow::Borrowed(fallback_name)];
        }
        let bin_paths: Vec<_> = bin_names
            .iter()
            .map(|name| {
                let name_str: &str = name.as_ref();
                install_path.join(name_str)
            })
            .map(|path| complete_windows_ext(path, pkg.complete_windows_ext, os()))
            .collect();
        let first_bin_path = bin_paths
            .first()
            .expect("at least one bin path should exist");
        let tar_opts = TarOptions {
            pr: Some(ctx.pr.as_ref()),
            ..TarOptions::new(TarFormat::from_ext(format))
        };
        let mut make_executable = false;
        if let AquaPackageType::GithubArchive = pkg.r#type {
            file::untar(&tarball_path, &install_path, &tar_opts)?;
        } else if let AquaPackageType::GithubContent = pkg.r#type {
            file::create_dir_all(&install_path)?;
            file::copy(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format == "raw" {
            file::create_dir_all(&install_path)?;
            file::copy(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format.starts_with("tar") {
            file::untar(&tarball_path, &install_path, &tar_opts)?;
            make_executable = true;
        } else if format == "zip" {
            file::unzip(&tarball_path, &install_path, &Default::default())?;
            make_executable = true;
        } else if format == "gz" {
            file::create_dir_all(&install_path)?;
            file::un_gz(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format == "xz" {
            file::create_dir_all(&install_path)?;
            file::un_xz(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format == "zst" {
            file::create_dir_all(&install_path)?;
            file::un_zst(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format == "bz2" {
            file::create_dir_all(&install_path)?;
            file::un_bz2(&tarball_path, first_bin_path)?;
            make_executable = true;
        } else if format == "dmg" {
            file::un_dmg(&tarball_path, &install_path)?;
        } else if format == "pkg" {
            file::un_pkg(&tarball_path, &install_path)?;
        } else {
            bail!("unsupported format: {}", format);
        }

        if make_executable {
            for bin_path in &bin_paths {
                // bin_path should exist, but doesn't when the registry is outdated
                if bin_path.exists() {
                    file::make_executable(bin_path)?;
                } else {
                    warn!("bin path does not exist: {}", bin_path.display());
                }
            }
        }

        let srcs = Self::srcs_for_platform(pkg, v, &install_path, os(), arch())?;
        for link in &srcs {
            if link.src != link.dst && link.src.exists() {
                Self::create_file_link(link)?;
            }
        }

        if self.symlink_bins(tv) {
            self.create_symlink_bin_dir(tv, &srcs)?;
        }

        Ok(())
    }

    /// Creates a `.mise-bins` directory with symlinks only to the binaries defined in the aqua registry.
    /// This prevents bundled dependencies (like Python in aws-cli) from being exposed on PATH.
    fn create_symlink_bin_dir(&self, tv: &ToolVersion, srcs: &[AquaFileLink]) -> Result<()> {
        let symlink_dir = tv.install_path().join(MISE_BINS_DIR);
        file::create_dir_all(&symlink_dir)?;

        for link in srcs {
            if let Some(bin_name) = link.dst.file_name() {
                let symlink_path = symlink_dir.join(bin_name);
                if link.dst.exists() && !symlink_path.exists() {
                    file::make_symlink_or_copy(&link.dst, &symlink_path)?;
                }
            }
        }
        Ok(())
    }

    fn symlink_bins(&self, tv: &ToolVersion) -> bool {
        tv.request
            .options()
            .get_string("symlink_bins")
            .is_some_and(|v| v == "true" || v == "1")
    }

    fn srcs(pkg: &AquaPackage, tv: &ToolVersion) -> Result<Vec<AquaFileLink>> {
        Self::srcs_for_platform(pkg, &tv.version, &tv.install_path(), os(), arch())
    }

    fn srcs_for_platform(
        pkg: &AquaPackage,
        version: &str,
        install_path: &Path,
        os: &str,
        arch: &str,
    ) -> Result<Vec<AquaFileLink>> {
        if pkg.files.is_empty() {
            let fallback_name = pkg
                .name
                .as_deref()
                .and_then(|n| n.split('/').next_back())
                .unwrap_or(&pkg.repo_name);

            let mut path = install_path.join(fallback_name);
            path = complete_windows_ext(path, pkg.complete_windows_ext, os);

            return Ok(vec![AquaFileLink {
                src: path.clone(),
                dst: path,
                hard: false,
                explicit_link: false,
            }]);
        }

        let versions = version_candidates(version, pkg.version_prefix.as_deref());
        let files: Vec<AquaFileLink> = pkg
            .files
            .iter()
            .map(|f| {
                let srcs = versions
                    .iter()
                    .map(|version| {
                        Self::file_link_for_version(
                            f,
                            pkg,
                            version.as_ref(),
                            install_path,
                            os,
                            arch,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(srcs.into_iter().flatten())
            })
            .flatten_ok()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .unique_by(|link| (link.src.to_path_buf(), link.dst.to_path_buf()))
            .collect();
        Ok(files)
    }

    fn file_link_for_version(
        f: &aqua_registry::AquaFile,
        pkg: &AquaPackage,
        version: &str,
        install_path: &Path,
        os: &str,
        arch: &str,
    ) -> Result<Option<AquaFileLink>> {
        let explicit_link = f.link.is_some();
        let src = match f.src(pkg, version, os, arch)? {
            Some(src) => src,
            None if explicit_link => f.name.clone(),
            None => return Ok(None),
        };
        let link = f.link(pkg, version, os, arch)?;

        let mut src = install_path.join(src);
        let mut dst = src
            .parent()
            .wrap_err_with(|| format!("file source has no parent: {}", src.display()))?
            .join(link.as_deref().unwrap_or(f.name.as_str()));
        src = complete_windows_ext(src, pkg.complete_windows_ext, os);
        dst = complete_windows_dst_ext(&src, dst, pkg.complete_windows_ext, os);

        Ok(Some(AquaFileLink {
            src,
            dst,
            hard: f.hard,
            explicit_link,
        }))
    }

    fn create_file_link(link: &AquaFileLink) -> Result<()> {
        if let Some(parent) = link.dst.parent() {
            file::create_dir_all(parent)?;
        }

        if link.hard || (cfg!(windows) && link.explicit_link) {
            trace!("ln {} {}", link.src.display(), link.dst.display());
            if link.dst.is_dir() {
                return Err(eyre!(
                    "destination is a directory, cannot create hard link: {}",
                    link.dst.display()
                ));
            }
            if link.dst.is_file() || link.dst.is_symlink() {
                fs::remove_file(&link.dst)?;
            }
            fs::hard_link(&link.src, &link.dst).wrap_err_with(|| {
                format!(
                    "failed to hard link {} {}",
                    link.src.display(),
                    link.dst.display()
                )
            })?;
            return Ok(());
        }

        if cfg!(windows) {
            file::copy(&link.src, &link.dst)?;
        } else {
            let target = link
                .dst
                .parent()
                .and_then(|parent| relative_path(parent, &link.src))
                .unwrap_or_else(|| link.src.clone());
            file::make_symlink(&target, &link.dst)?;
        }
        Ok(())
    }
}

fn relative_path(from: &Path, to: &Path) -> Option<PathBuf> {
    let from_components = from.components().collect_vec();
    let to_components = to.components().collect_vec();
    let common_len = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(from, to)| from == to)
        .count();

    let mut result = PathBuf::new();
    for component in &from_components[common_len..] {
        match component {
            std::path::Component::Normal(_) => result.push(".."),
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    for component in &to_components[common_len..] {
        match component {
            std::path::Component::Normal(_) | std::path::Component::CurDir => {
                result.push(component.as_os_str())
            }
            _ => return None,
        }
    }
    if result.as_os_str().is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(result)
    }
}

fn unescape_regex_literal(pattern: &str) -> Cow<'_, str> {
    // Fast path: If there are no backslashes, we return the original slice.
    // .contains() is highly optimized and avoids any heap allocation.
    if !pattern.contains('\\') {
        return Cow::Borrowed(pattern);
    }

    // Slow path: We have escapes to process, so we must allocate a new String.
    // Capacity is set to pattern.len() to ensure exactly one allocation.
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // If there's a character after the backslash, push it (unescaping).
            if let Some(next) = chars.next() {
                out.push(next);
            } else {
                // Handle trailing backslash: push the backslash itself.
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

fn toml_value_to_string(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn aqua_var_option(opts: &ToolVersionOptions, name: &str) -> Result<Option<String>> {
    if let Some(toml::Value::Table(vars)) = opts.opts.get("vars")
        && let Some(value) = vars.get(name)
    {
        return toml_string_var(&format!("vars.{name}"), value).map(Some);
    }
    opts.opts
        .get(name)
        .map(|value| toml_string_var(name, value).map(Some))
        .unwrap_or(Ok(None))
}

fn toml_string_var(key: &str, value: &toml::Value) -> Result<String> {
    match value {
        toml::Value::String(s) => Ok(s.clone()),
        value => bail!(
            "aqua var `{}` must be a string, got {}",
            key,
            toml_value_kind(value)
        ),
    }
}

fn toml_value_kind(value: &toml::Value) -> &'static str {
    match value {
        toml::Value::String(_) => "string",
        toml::Value::Integer(_) | toml::Value::Float(_) => "number",
        toml::Value::Boolean(_) => "boolean",
        toml::Value::Array(_) => "array",
        toml::Value::Table(_) => "object",
        toml::Value::Datetime(_) => "datetime",
    }
}

fn version_with_prefix<'a>(version: &'a str, version_prefix: Option<&str>) -> Cow<'a, str> {
    if let Some(prefix) = version_prefix
        && !version.starts_with(prefix)
    {
        Cow::Owned(format!("{prefix}{version}"))
    } else {
        Cow::Borrowed(version)
    }
}

fn version_candidates<'a>(version: &'a str, version_prefix: Option<&str>) -> Vec<Cow<'a, str>> {
    let mut candidates = vec![version_with_prefix(version, version_prefix)];
    if let Some(prefix) = version_prefix {
        let base = version.strip_prefix(prefix).unwrap_or(version);
        if !prefix.is_empty() && !starts_with_v(base) && !ends_with_v(prefix) {
            candidates.push(Cow::Owned(format!("{prefix}v{base}")));
        }
    } else if !starts_with_v(version) {
        candidates.push(Cow::Owned(format!("v{version}")));
    }
    candidates.into_iter().unique().collect()
}

fn starts_with_v(s: &str) -> bool {
    s.starts_with('v') || s.starts_with('V')
}

fn ends_with_v(s: &str) -> bool {
    s.ends_with('v') || s.ends_with('V')
}

fn complete_windows_ext(path: PathBuf, complete: bool, target_os: &str) -> PathBuf {
    if target_os == "windows" && complete && path.extension().is_none() {
        path.with_extension("exe")
    } else {
        path
    }
}

fn complete_windows_dst_ext(src: &Path, dst: PathBuf, complete: bool, target_os: &str) -> PathBuf {
    if target_os != "windows" || !complete || dst.extension().is_some() {
        return dst;
    }
    match src.extension() {
        Some(ext) => dst.with_extension(ext),
        None => dst.with_extension("exe"),
    }
}

/// Returns install-time-only option keys for the Aqua backend.
///
/// Aqua registry vars may be provided either as a nested `vars` table or as
/// flat top-level keys whose names are declared by the registry package. The
/// flat names are not statically knowable here, so `is_install_time_option_key`
/// handles the precise filtering rule.
pub fn install_time_option_keys() -> Vec<String> {
    vec!["vars".into()]
}

pub fn is_install_time_option_key(key: &str) -> bool {
    key != "symlink_bins"
}

#[cfg(test)]
mod tests {
    use super::*;
    use aqua_registry::{AquaFile, AquaVar};

    fn aqua_var(name: &str, required: bool) -> AquaVar {
        AquaVar {
            name: name.to_string(),
            default: None,
            required,
        }
    }

    #[test]
    fn test_version_with_prefix_does_not_double_prefix() {
        assert_eq!(version_with_prefix("1.0.0", Some("tool-")), "tool-1.0.0");
        assert_eq!(
            version_with_prefix("tool-1.0.0", Some("tool-")),
            "tool-1.0.0"
        );
    }

    #[test]
    fn test_version_candidates_include_prefixed_v_tag() {
        let candidates = version_candidates("1.2.3", Some("tool/"))
            .into_iter()
            .map(|v| v.into_owned())
            .collect_vec();

        assert_eq!(candidates, vec!["tool/1.2.3", "tool/v1.2.3"]);
    }

    #[test]
    fn test_version_candidates_include_prefixed_v_tag_for_prefixed_version() {
        let candidates = version_candidates("tool/1.2.3", Some("tool/"))
            .into_iter()
            .map(|v| v.into_owned())
            .collect_vec();

        assert_eq!(candidates, vec!["tool/1.2.3", "tool/v1.2.3"]);
    }

    #[test]
    fn test_version_candidates_do_not_double_v_prefix() {
        let candidates = version_candidates("1.2.3", Some("tool-v"))
            .into_iter()
            .map(|v| v.into_owned())
            .collect_vec();

        assert_eq!(candidates, vec!["tool-v1.2.3"]);
    }

    #[test]
    fn test_complete_windows_ext_preserves_existing_extension() {
        assert_eq!(
            complete_windows_ext(PathBuf::from("bat/arq.bat"), true, "windows"),
            PathBuf::from("bat/arq.bat")
        );
        assert_eq!(
            complete_windows_ext(PathBuf::from("bin/tool"), true, "windows"),
            PathBuf::from("bin/tool.exe")
        );
    }

    #[test]
    fn test_complete_windows_dst_ext_uses_source_extension() {
        assert_eq!(
            complete_windows_dst_ext(
                Path::new("bat/arq.bat"),
                PathBuf::from("bat/arq"),
                true,
                "windows",
            ),
            PathBuf::from("bat/arq.bat")
        );
        assert_eq!(
            complete_windows_dst_ext(
                Path::new("bin/tool"),
                PathBuf::from("bin/tool"),
                true,
                "windows"
            ),
            PathBuf::from("bin/tool.exe")
        );
    }

    #[test]
    fn test_apply_var_options_prefers_nested_vars() {
        let mut pkg = AquaPackage::default();
        pkg.asset = "tool-{{.Vars.channel}}-{{.Version}}.tar.gz".to_string();
        pkg.vars = vec![aqua_var("channel", true)];
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "channel".to_string(),
            toml::Value::String("stable".to_string()),
        );
        let mut vars = toml::Table::new();
        vars.insert(
            "channel".to_string(),
            toml::Value::String("beta".to_string()),
        );
        opts.opts
            .insert("vars".to_string(), toml::Value::Table(vars));

        let pkg = AquaBackend::apply_var_options(pkg, &opts).unwrap();

        assert_eq!(
            pkg.asset("1.0.0", "linux", "amd64").unwrap(),
            "tool-beta-1.0.0.tar.gz"
        );
    }

    #[test]
    fn test_apply_var_options_errors_for_array_vars() {
        let mut pkg = AquaPackage::default();
        pkg.vars = vec![aqua_var("channels", true)];
        let mut opts = ToolVersionOptions::default();
        let mut vars = toml::Table::new();
        vars.insert(
            "channels".to_string(),
            toml::Value::Array(vec![toml::Value::String("stable".to_string())]),
        );
        opts.opts
            .insert("vars".to_string(), toml::Value::Table(vars));

        let err = AquaBackend::apply_var_options(pkg, &opts).unwrap_err();

        assert!(
            err.to_string()
                .contains("aqua var `vars.channels` must be a string, got array"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_apply_var_options_errors_for_missing_required_var() {
        let mut pkg = AquaPackage::default();
        pkg.vars = vec![aqua_var("go_version", true)];
        let err = AquaBackend::apply_var_options(pkg, &ToolVersionOptions::default()).unwrap_err();

        assert!(
            err.to_string()
                .contains("required aqua var not set: go_version"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_lockfile_options_include_aqua_vars() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "channel".to_string(),
            toml::Value::String("stable".to_string()),
        );
        opts.opts.insert(
            "symlink_bins".to_string(),
            toml::Value::String("true".to_string()),
        );
        opts.opts.insert(
            "postinstall".to_string(),
            toml::Value::String("echo ok".to_string()),
        );
        let mut vars = toml::Table::new();
        vars.insert("go_version".to_string(), toml::Value::String("1.24".into()));
        opts.opts
            .insert("vars".to_string(), toml::Value::Table(vars));

        let lock_opts = AquaBackend::lockfile_options(&opts);

        assert_eq!(lock_opts.get("vars.channel"), Some(&"stable".to_string()));
        assert_eq!(lock_opts.get("vars.go_version"), Some(&"1.24".to_string()));
        assert!(!lock_opts.contains_key("symlink_bins"));
        assert!(!lock_opts.contains_key("postinstall"));
    }

    #[test]
    fn test_lockfile_options_canonicalize_equivalent_aqua_vars() {
        let mut top_level = ToolVersionOptions::default();
        top_level.opts.insert(
            "channel".to_string(),
            toml::Value::String("stable".to_string()),
        );

        let mut nested = ToolVersionOptions::default();
        let mut vars = toml::Table::new();
        vars.insert(
            "channel".to_string(),
            toml::Value::String("stable".to_string()),
        );
        nested
            .opts
            .insert("vars".to_string(), toml::Value::Table(vars));

        assert_eq!(
            AquaBackend::lockfile_options(&top_level),
            AquaBackend::lockfile_options(&nested)
        );
    }

    #[test]
    fn test_lockfile_options_nested_aqua_vars_take_precedence() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "channel".to_string(),
            toml::Value::String("stable".to_string()),
        );
        let mut vars = toml::Table::new();
        vars.insert(
            "channel".to_string(),
            toml::Value::String("beta".to_string()),
        );
        opts.opts
            .insert("vars".to_string(), toml::Value::Table(vars));

        let lock_opts = AquaBackend::lockfile_options(&opts);

        assert_eq!(lock_opts.get("vars.channel"), Some(&"beta".to_string()));
    }

    #[test]
    fn test_aqua_install_time_options_include_flat_vars() {
        assert!(is_install_time_option_key("channel"));
        assert!(is_install_time_option_key("vars.channel"));
        assert!(is_install_time_option_key("vars"));
        assert!(!is_install_time_option_key("symlink_bins"));
    }

    #[test]
    fn test_srcs_support_file_link_with_default_src() {
        let mut pkg = AquaPackage::default();
        pkg.files = vec![AquaFile {
            name: "mc".to_string(),
            link: Some("mc.exe".to_string()),
            ..Default::default()
        }];
        pkg.complete_windows_ext = false;

        let links = AquaBackend::srcs_for_platform(
            &pkg,
            "RELEASE.2025-08-13T08-35-41Z",
            Path::new("install"),
            "windows",
            "amd64",
        )
        .unwrap();

        assert_eq!(
            links,
            vec![AquaFileLink {
                src: PathBuf::from("install").join("mc"),
                dst: PathBuf::from("install").join("mc.exe"),
                hard: false,
                explicit_link: true,
            }]
        );
    }

    #[test]
    fn test_srcs_support_hard_file_link() {
        let mut pkg = AquaPackage::default();
        pkg.files = vec![AquaFile {
            name: "pnpm".to_string(),
            src: Some("bin/pnpm".to_string()),
            link: Some("pnpm-hard".to_string()),
            hard: true,
        }];

        let links =
            AquaBackend::srcs_for_platform(&pkg, "1.0.0", Path::new("install"), "linux", "amd64")
                .unwrap();

        assert_eq!(
            links,
            vec![AquaFileLink {
                src: PathBuf::from("install").join("bin/pnpm"),
                dst: PathBuf::from("install").join("bin/pnpm-hard"),
                hard: true,
                explicit_link: true,
            }]
        );
    }

    #[test]
    fn test_srcs_include_prefixed_v_version_paths() {
        let mut pkg = AquaPackage::default();
        pkg.asset = "tool-{{.Version}}-{{.OS}}-{{.Arch}}.tar.gz".to_string();
        pkg.version_prefix = Some("tool-".to_string());
        pkg.files = vec![AquaFile {
            name: "tool".to_string(),
            src: Some("{{.AssetWithoutExt}}/bin/tool".to_string()),
            ..Default::default()
        }];

        let links =
            AquaBackend::srcs_for_platform(&pkg, "1.2.3", Path::new("install"), "linux", "amd64")
                .unwrap();

        assert_eq!(
            links,
            vec![
                AquaFileLink {
                    src: PathBuf::from("install").join("tool-tool-1.2.3-linux-amd64/bin/tool"),
                    dst: PathBuf::from("install").join("tool-tool-1.2.3-linux-amd64/bin/tool"),
                    hard: false,
                    explicit_link: false,
                },
                AquaFileLink {
                    src: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                    dst: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                    hard: false,
                    explicit_link: false,
                },
            ]
        );
    }

    #[test]
    fn test_srcs_resolved_tag_version_does_not_add_extra_candidates() {
        let mut pkg = AquaPackage::default();
        pkg.asset = "tool-{{.Version}}-{{.OS}}-{{.Arch}}.tar.gz".to_string();
        pkg.version_prefix = Some("tool-".to_string());
        pkg.files = vec![AquaFile {
            name: "tool".to_string(),
            src: Some("{{.AssetWithoutExt}}/bin/tool".to_string()),
            ..Default::default()
        }];

        let links = AquaBackend::srcs_for_platform(
            &pkg,
            "tool-v1.2.3",
            Path::new("install"),
            "linux",
            "amd64",
        )
        .unwrap();

        assert_eq!(
            links,
            vec![AquaFileLink {
                src: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                dst: PathBuf::from("install").join("tool-tool-v1.2.3-linux-amd64/bin/tool"),
                hard: false,
                explicit_link: false,
            }]
        );
    }

    #[test]
    fn test_relative_path_between_link_and_source() {
        assert_eq!(
            relative_path(
                Path::new("/tmp/install/bin/aliases"),
                Path::new("/tmp/install/bin/tool"),
            )
            .unwrap(),
            PathBuf::from("../tool")
        );
    }

    #[test]
    fn test_relative_path_with_shared_curdir() {
        assert_eq!(
            relative_path(
                Path::new("./install/bin/aliases"),
                Path::new("./install/bin/tool"),
            )
            .unwrap(),
            PathBuf::from("../tool")
        );
    }

    #[test]
    fn test_create_file_link_rejects_hard_link_directory_destination() -> Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let src = tmp_dir.path().join("tool");
        let dst = tmp_dir.path().join("tool-hard");
        fs::write(&src, "tool")?;
        fs::create_dir(&dst)?;

        let err = AquaBackend::create_file_link(&AquaFileLink {
            src,
            dst,
            hard: true,
            explicit_link: true,
        })
        .unwrap_err()
        .to_string();

        assert!(err.contains("destination is a directory"));
        Ok(())
    }

    #[test]
    fn test_unescape_regex_literal_no_backslash_is_borrowed() {
        let result = unescape_regex_literal("astral-sh/ruff/.github/workflows/release.yml");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "astral-sh/ruff/.github/workflows/release.yml");
    }

    #[test]
    fn test_unescape_regex_literal_escaped_dot() {
        assert_eq!(unescape_regex_literal(r"\."), ".");
    }

    #[test]
    fn test_unescape_regex_literal_updatecli_signer_workflow() {
        assert_eq!(
            unescape_regex_literal(r"updatecli/updatecli/\.github/workflows/release\.yaml"),
            "updatecli/updatecli/.github/workflows/release.yaml"
        );
    }

    #[test]
    fn test_unescape_regex_literal_escaped_backslash() {
        assert_eq!(unescape_regex_literal(r"\\"), "\\");
    }

    #[test]
    fn test_unescape_regex_literal_trailing_backslash() {
        assert_eq!(unescape_regex_literal("foo\\"), "foo\\");
    }

    #[test]
    fn test_unescape_regex_literal_empty_string() {
        let result = unescape_regex_literal("");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
        assert_eq!(result, "");
    }

    #[test]
    fn test_unescape_regex_literal_only_backslash() {
        assert_eq!(unescape_regex_literal("\\"), "\\");
    }
}

async fn get_tags(pkg: &AquaPackage) -> Result<Vec<String>> {
    Ok(get_tags_with_created_at(pkg)
        .await?
        .into_iter()
        .map(|(tag, _, _)| tag)
        .collect())
}

#[cfg(test)]
fn version_from_tag(pkg: &AquaPackage, tag: &str) -> Result<Option<String>> {
    let target = PlatformTarget::from_current();
    let (target_os, target_arch) = AquaBackend::to_aqua_platform(&target);
    let target_libc = AquaBackend::target_variant_libc(&target);
    Ok(
        versioned_package_from_tag(pkg, tag, target_os, target_arch, target_libc.as_deref())?
            .map(|(version, _)| version),
    )
}

fn versioned_package_from_tag(
    pkg: &AquaPackage,
    tag: &str,
    target_os: &str,
    target_arch: &str,
    target_libc: Option<&str>,
) -> Result<Option<(String, AquaPackage)>> {
    if !pkg.version_filter_ok(tag)? || !pkg.version_constraint_ok(&[tag]) {
        return Ok(None);
    }

    let mut version = tag;
    let versioned_pkg = pkg
        .clone()
        .with_version_libc(&[tag], target_os, target_arch, target_libc);
    if let Some(prefix) = &versioned_pkg.version_prefix {
        let Some(stripped) = version.strip_prefix(prefix) else {
            return Ok(None);
        };
        version = stripped;
    }
    let version = version.strip_prefix('v').unwrap_or(version);
    Ok(Some((version.to_string(), versioned_pkg)))
}

fn package_has_asset(pkg: &AquaPackage) -> bool {
    !pkg.no_asset && pkg.error_message.is_none()
}

/// Get tags with optional created_at timestamps and a pre-release flag.
/// Returns `(tag_name, Option<created_at>, prerelease)` triples.
///
/// Always fetches the pre-release superset so the shared remote-versions cache
/// is independent of the `prerelease` tool option; callers filter on the
/// returned `prerelease` bit at read time. Git tags (the `github_tag` version
/// source) carry no pre-release flag, so those entries are reported as
/// `prerelease = false` and rely on the shared regex-based fuzzy-match filter.
async fn get_tags_with_created_at(
    pkg: &AquaPackage,
) -> Result<Vec<(String, Option<String>, bool)>> {
    if let Some("github_tag") = pkg.version_source.as_deref() {
        // Tags don't have created_at timestamps or a prerelease flag
        let versions = github::list_tags(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
        return Ok(versions.into_iter().map(|v| (v, None, false)).collect());
    }
    let repo = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
    let releases = github::list_releases_including_prereleases(&repo).await?;
    Ok(releases
        .into_iter()
        .map(|r| (r.tag_name, Some(r.created_at), r.prerelease))
        .collect())
}

fn validate(pkg: &AquaPackage) -> Result<()> {
    if pkg.no_asset {
        bail!("no asset released");
    }
    if let Some(message) = &pkg.error_message {
        bail!("{}", message);
    }
    if !is_platform_supported(&pkg.supported_envs, os(), arch()) {
        bail!(
            "unsupported env: {}/{} (supported: {:?})",
            os(),
            arch(),
            pkg.supported_envs
        );
    }
    match pkg.r#type {
        AquaPackageType::Cargo => {
            bail!(
                "package type `cargo` is not supported in the aqua backend. Use the cargo backend instead{}.",
                pkg.name
                    .as_ref()
                    .and_then(|s| s.strip_prefix("crates.io/"))
                    .map(|name| format!(": cargo:{name}"))
                    .unwrap_or_default()
            )
        }
        AquaPackageType::GoInstall | AquaPackageType::GoBuild => {
            bail!(
                "package type `{}` is not supported in the aqua backend. Use the go backend instead{}.",
                pkg.r#type,
                pkg.path
                    .as_ref()
                    .map(|path| format!(": go:{path}"))
                    .unwrap_or_else(|| {
                        format!(": go:github.com/{}/{}", pkg.repo_owner, pkg.repo_name)
                    })
            )
        }
        _ => {}
    }
    Ok(())
}

/// Resolve repo owner and name from an override config, falling back to pkg defaults.
fn resolve_repo_info(
    override_owner: Option<&String>,
    override_name: Option<&String>,
    pkg: &AquaPackage,
) -> (String, String) {
    let owner = override_owner
        .cloned()
        .unwrap_or_else(|| pkg.repo_owner.clone());
    let name = override_name
        .cloned()
        .unwrap_or_else(|| pkg.repo_name.clone());
    (owner, name)
}

/// Check if extraction is needed based on format and package type.
fn needs_extraction(format: &str, pkg_type: &AquaPackageType) -> bool {
    (!format.is_empty() && format != "raw")
        || matches!(
            pkg_type,
            AquaPackageType::GithubArchive | AquaPackageType::GithubContent
        )
}

/// Check if a platform is supported by the package's supported_envs.
/// Returns true if supported, false if not.
fn is_platform_supported(supported_envs: &[String], os: &str, arch: &str) -> bool {
    if supported_envs.is_empty() {
        return true;
    }
    let envs: HashSet<&str> = supported_envs.iter().map(|s| s.as_str()).collect();
    let os_arch = format!("{os}/{arch}");
    let mut myself: HashSet<&str> = ["all", os, arch, os_arch.as_str()].into();
    // Windows ARM64 can typically run AMD64 binaries via emulation
    if os == "windows" && arch == "arm64" {
        myself.insert("windows/amd64");
        myself.insert("amd64");
    }
    !envs.is_disjoint(&myself)
}

fn target_prefers_musl(target: &PlatformTarget) -> bool {
    target.os_name() == "linux" && AquaBackend::target_libc(target).as_deref() == Some("musl")
}

fn is_aqua_linux_libc_replacement(replacement: &str) -> bool {
    matches!(
        replacement,
        "unknown-linux-gnu" | "unknown-linux-musl" | "linux-gnu" | "linux-musl"
    )
}

fn select_github_release_asset<'a>(
    assets: &'a [github::GithubAsset],
    asset_strs: &IndexSet<String>,
    prefer_musl: bool,
) -> Option<&'a github::GithubAsset> {
    let assets_with_tokens = if prefer_musl {
        assets
            .iter()
            .map(|asset| (asset, asset_name_tokens(&asset.name)))
            .collect_vec()
    } else {
        vec![]
    };
    asset_strs.iter().find_map(|expected| {
        let exact = assets
            .iter()
            .find(|a| a.name == *expected || a.name.to_lowercase() == expected.to_lowercase());

        let expected_tokens = asset_name_tokens(expected);
        if prefer_musl
            && let Some(musl_asset) = assets_with_tokens.iter().find_map(|(asset, tokens)| {
                is_musl_variant_of_expected_asset(tokens, &expected_tokens).then_some(*asset)
            })
        {
            return Some(musl_asset);
        }

        exact
    })
}

fn is_musl_variant_of_expected_asset(asset_tokens: &[String], expected_tokens: &[String]) -> bool {
    asset_tokens.iter().any(|token| token == "musl")
        && !expected_tokens.iter().any(|token| token == "musl")
        && itertools::equal(
            asset_tokens
                .iter()
                .filter(|token| !matches!(token.as_str(), "musl" | "gnu" | "glibc")),
            expected_tokens
                .iter()
                .filter(|token| !matches!(token.as_str(), "musl" | "gnu" | "glibc")),
        )
}

fn asset_name_tokens(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_lowercase())
        .collect()
}

pub fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        &OS
    }
}

pub fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "arm") {
        "armv6l"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        &ARCH
    }
}

#[cfg(test)]
mod lock_candidate_tests {
    use crate::github::GithubAsset;

    use super::*;

    fn build_lock_candidates(
        version: &str,
        tag: Option<&str>,
        version_prefix: Option<&str>,
    ) -> (String, Vec<String>) {
        let tag_is_none = tag.is_none();
        let mut v = tag.unwrap_or(version).to_string();
        let mut v_prefixed = (tag_is_none && !version.starts_with('v')).then(|| format!("v{v}"));

        if let Some(prefix) = version_prefix
            && !v.starts_with(prefix)
        {
            v = format!("{prefix}{v}");
            v_prefixed = v_prefixed.map(|vp| {
                if vp.starts_with(prefix) {
                    vp
                } else {
                    format!("{prefix}{vp}")
                }
            });
        }

        let candidates = match &v_prefixed {
            Some(vp) => vec![vp.clone(), v.clone()],
            None => vec![v.clone()],
        };
        (v, candidates)
    }

    // When tag lookup fails (e.g. rate limit), we try both v-prefixed and bare versions.
    #[test]
    fn test_lock_candidates_no_tag() {
        let (v, candidates) = build_lock_candidates("10.20.0", None, None);
        assert_eq!(v, "10.20.0");
        assert_eq!(candidates, vec!["v10.20.0", "10.20.0"]);
    }

    #[test]
    fn test_lock_candidates_no_tag_with_version_prefix() {
        let (v, candidates) = build_lock_candidates("1.7.1", None, Some("jq-"));
        assert_eq!(v, "jq-1.7.1");
        assert_eq!(candidates, vec!["jq-v1.7.1", "jq-1.7.1"]);
    }

    #[test]
    fn test_version_from_tag_strips_v_prefix() {
        let pkg = AquaPackage::default();
        assert_eq!(
            version_from_tag(&pkg, "v1.2.3").unwrap(),
            Some("1.2.3".to_string())
        );
    }

    #[test]
    fn test_version_from_tag_strips_aqua_version_prefix() {
        let mut pkg = AquaPackage::default();
        pkg.version_prefix = Some("mountpoint-s3-".to_string());

        assert_eq!(
            version_from_tag(&pkg, "mountpoint-s3-1.2.3").unwrap(),
            Some("1.2.3".to_string())
        );
        assert_eq!(version_from_tag(&pkg, "other-1.2.3").unwrap(), None);
    }

    fn pkg_from_yaml(yaml: &str) -> AquaPackage {
        let mut pkg: AquaPackage = serde_yaml::from_str(yaml).unwrap();
        pkg.setup_version_filter().unwrap();
        pkg
    }

    #[test]
    fn test_version_from_tag_rejects_version_filter_mismatch() {
        let pkg = pkg_from_yaml(
            r#"
type: github_release
repo_owner: owner
repo_name: repo
version_filter: semver(">= 1.0.0")
"#,
        );

        assert_eq!(
            version_from_tag(&pkg, "v1.0.0").unwrap(),
            Some("1.0.0".to_string())
        );
        assert_eq!(version_from_tag(&pkg, "v0.9.0").unwrap(), None);
    }

    #[test]
    fn test_version_from_tag_rejects_version_constraint_mismatch() {
        let pkg = pkg_from_yaml(
            r#"
type: github_release
repo_owner: owner
repo_name: repo
version_constraint: "false"
version_overrides:
  - version_constraint: Version == "v1.2.3"
    asset: tool.tar.gz
    format: tar.gz
"#,
        );

        assert_eq!(
            version_from_tag(&pkg, "v1.2.3").unwrap(),
            Some("1.2.3".to_string())
        );
        assert_eq!(version_from_tag(&pkg, "v1.2.4").unwrap(), None);
    }

    #[test]
    fn test_package_has_asset_rejects_no_asset_and_errors() {
        let mut pkg = AquaPackage::default();
        assert!(package_has_asset(&pkg));

        pkg.no_asset = true;
        assert!(!package_has_asset(&pkg));

        pkg.no_asset = false;
        pkg.error_message = Some("unsupported version".to_string());
        assert!(!package_has_asset(&pkg));
    }

    fn asset(name: &str) -> GithubAsset {
        GithubAsset {
            name: name.to_string(),
            browser_download_url: format!("https://example.com/{name}"),
            url: format!("https://api.example.com/{name}"),
            digest: None,
        }
    }

    #[test]
    fn test_select_github_release_asset_prefers_musl_variant() {
        let assets = vec![
            asset("tool-1.0.0-x86_64-unknown-linux-gnu.tar.gz"),
            asset("tool-1.0.0-x86_64-unknown-linux-musl.tar.gz"),
        ];
        let asset_strs = IndexSet::from(["tool-1.0.0-x86_64-unknown-linux-gnu.tar.gz".to_string()]);

        let selected = select_github_release_asset(&assets, &asset_strs, true).unwrap();

        assert_eq!(selected.name, "tool-1.0.0-x86_64-unknown-linux-musl.tar.gz");
    }

    #[test]
    fn test_select_github_release_asset_keeps_exact_without_musl_preference() {
        let assets = vec![
            asset("tool-1.0.0-x86_64-unknown-linux-gnu.tar.gz"),
            asset("tool-1.0.0-x86_64-unknown-linux-musl.tar.gz"),
        ];
        let asset_strs = IndexSet::from(["tool-1.0.0-x86_64-unknown-linux-gnu.tar.gz".to_string()]);

        let selected = select_github_release_asset(&assets, &asset_strs, false).unwrap();

        assert_eq!(selected.name, "tool-1.0.0-x86_64-unknown-linux-gnu.tar.gz");
    }

    #[test]
    fn test_select_github_release_asset_uses_musl_when_exact_missing() {
        let assets = vec![asset("tool-1.0.0-linux-amd64-musl.tar.gz")];
        let asset_strs = IndexSet::from(["tool-1.0.0-linux-amd64.tar.gz".to_string()]);

        let selected = select_github_release_asset(&assets, &asset_strs, true).unwrap();

        assert_eq!(selected.name, "tool-1.0.0-linux-amd64-musl.tar.gz");
    }

    #[test]
    fn test_musl_variant_match_requires_standalone_token() {
        let asset_tokens = asset_name_tokens("tool-1.0.0-linux-amd64-muslvariant.tar.gz");
        let expected_tokens = asset_name_tokens("tool-1.0.0-linux-amd64.tar.gz");

        assert!(!is_musl_variant_of_expected_asset(
            &asset_tokens,
            &expected_tokens,
        ));
    }

    #[test]
    fn test_apply_aqua_libc_replacement_switches_target_triples() {
        let mut pkg = AquaPackage::default();
        pkg.replacements
            .insert("linux".to_string(), "unknown-linux-gnu".to_string());

        let pkg = AquaBackend::apply_aqua_libc_replacement(pkg, "linux", Some("musl".to_string()));

        assert_eq!(
            pkg.replacements.get("linux").map(String::as_str),
            Some("unknown-linux-musl")
        );
    }

    #[test]
    fn test_apply_aqua_libc_replacement_preserves_linux_prefix() {
        let mut pkg = AquaPackage::default();
        pkg.replacements
            .insert("linux".to_string(), "linux-gnu".to_string());

        let pkg = AquaBackend::apply_aqua_libc_replacement(pkg, "linux", Some("musl".to_string()));

        assert_eq!(
            pkg.replacements.get("linux").map(String::as_str),
            Some("linux-musl")
        );
    }

    #[test]
    fn test_apply_aqua_libc_replacement_keeps_non_libc_replacements() {
        let mut pkg = AquaPackage::default();
        pkg.replacements
            .insert("linux".to_string(), "Linux".to_string());

        let pkg = AquaBackend::apply_aqua_libc_replacement(pkg, "linux", Some("musl".to_string()));

        assert_eq!(
            pkg.replacements.get("linux").map(String::as_str),
            Some("Linux")
        );
    }
}
