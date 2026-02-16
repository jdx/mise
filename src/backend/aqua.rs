use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;

use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::cli::args::BackendArg;
use crate::cli::version::{ARCH, OS};
use crate::config::Settings;
use crate::file::TarOptions;
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::path::{Path, PathBuf, PathExt};
use crate::plugins::VERSION_REGEX;
use crate::registry::REGISTRY;
use crate::toolset::ToolVersion;
use crate::{
    aqua::aqua_registry_wrapper::{
        AQUA_REGISTRY, AquaChecksum, AquaChecksumType, AquaMinisignType, AquaPackage,
        AquaPackageType,
    },
    cache::{CacheManager, CacheManagerBuilder},
};
use crate::{backend::Backend, config::Config};
use crate::{env, file, github, minisign};
use async_trait::async_trait;
use dashmap::DashMap;
use eyre::{ContextCompat, Result, bail, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use regex::Regex;
use std::borrow::Cow;
use std::fmt::Debug;
use std::{collections::HashSet, sync::Arc};

#[derive(Debug)]
pub struct AquaBackend {
    ba: Arc<BackendArg>,
    id: String,
    version_tags_cache: CacheManager<Vec<(String, String)>>,
    bin_path_caches: DashMap<String, CacheManager<Vec<PathBuf>>>,
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
        let pkg = match AQUA_REGISTRY
            .package_with_version(&self.id, &[&tv.version])
            .await
        {
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

        // GitHub Attestations - check registry config OR actual release assets
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

        // Cosign (nested in checksum) - check registry config OR actual release assets
        let has_cosign_config = all_pkgs.iter().any(|p| {
            p.checksum
                .as_ref()
                .and_then(|c| c.cosign.as_ref())
                .is_some_and(|cosign| cosign.enabled.unwrap_or(true))
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

        let tags_with_timestamps = match get_tags_with_created_at(&pkg).await {
            Ok(tags) => tags,
            Err(e) => {
                warn!("Remote versions cannot be fetched: {}", e);
                return Ok(vec![]);
            }
        };

        let mut versions = Vec::new();
        for (tag, created_at) in tags_with_timestamps.into_iter().rev() {
            let mut version = tag.as_str();
            match pkg.version_filter_ok(version) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    warn!("[{}] aqua version filter error: {e}", self.ba());
                    continue;
                }
            }
            let versioned_pkg = pkg.clone().with_version(&[version], os(), arch());
            if let Some(prefix) = &versioned_pkg.version_prefix {
                if let Some(_v) = version.strip_prefix(prefix) {
                    version = _v;
                } else {
                    continue;
                }
            }
            version = version.strip_prefix('v').unwrap_or(version);

            // Validate the package has assets
            let check_pkg = AQUA_REGISTRY
                .package_with_version(&self.id, &[&tag])
                .await
                .unwrap_or_default();
            if !check_pkg.no_asset && check_pkg.error_message.is_none() {
                let release_url = format!(
                    "https://github.com/{}/{}/releases/tag/{}",
                    pkg.repo_owner, pkg.repo_name, tag
                );
                versions.push(VersionInfo {
                    version: version.to_string(),
                    created_at,
                    release_url: Some(release_url),
                    ..Default::default()
                });
            }
        }
        Ok(versions)
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
        let pkg = AQUA_REGISTRY
            .package_with_version(&self.id, &versions)
            .await?;
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
        let validated_url = if let Some(ref url) = existing_platform {
            if pkg.r#type != AquaPackageType::GithubRelease {
                existing_platform // Skip validation for non-release package types
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
        if self.symlink_bins(tv) {
            return Ok(vec![tv.install_path().join(".mise-bins")]);
        }

        let cache = self
            .bin_path_caches
            .entry(tv.version.clone())
            .or_insert_with(|| {
                CacheManagerBuilder::new(tv.cache_path().join("bin_paths.msgpack.z"))
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .build()
            });
        let install_path = tv.install_path();
        let paths = cache
            .get_or_try_init_async(async || {
                // TODO: align this logic with the one in `install_version_`
                let pkg = AQUA_REGISTRY
                    .package_with_version(&self.id, &[&tv.version])
                    .await?;

                let srcs = self.srcs(&pkg, tv)?;
                let paths = if srcs.is_empty() {
                    vec![install_path.clone()]
                } else {
                    srcs.iter()
                        .map(|(_, dst)| dst.parent().unwrap().to_path_buf())
                        .collect()
                };
                Ok(paths
                    .into_iter()
                    .unique()
                    .filter(|p| p.exists())
                    .map(|p| p.strip_prefix(&install_path).unwrap().to_path_buf())
                    .collect())
            })
            .await?
            .iter()
            .map(|p| p.mount(&install_path))
            .collect();
        Ok(paths)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
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
                if VERSION_REGEX.is_match(v) {
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
        // Map Platform to Aqua's os/arch conventions
        let target_os = match target.os_name() {
            "macos" => "darwin",
            other => other,
        };
        let target_arch = match target.arch_name() {
            "x64" => "amd64",
            other => other,
        };

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
        let v_prefixed = (tag_is_none && !tv.version.starts_with('v')).then(|| format!("v{v}"));
        let versions = match &v_prefixed {
            Some(v_prefixed) => vec![v.as_str(), v_prefixed.as_str()],
            None => vec![v.as_str()],
        };

        // Get package and apply version/overrides directly for the target platform.
        // Using package_with_version() here would apply overrides for the current host
        // platform first, which can leak host-specific overrides into cross-platform lock.
        let pkg = AQUA_REGISTRY.package(&self.id).await?;
        let pkg = pkg.with_version(&versions, target_os, target_arch);

        // Apply version prefix if present
        if let Some(prefix) = &pkg.version_prefix
            && !v.starts_with(prefix)
        {
            v = format!("{prefix}{v}");
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
                // For GitHub releases, we need to find the asset for the target platform
                let asset_strs = pkg.asset_strs(&v, target_os, target_arch)?;
                match self.github_release_asset(&pkg, &v, asset_strs).await {
                    Ok((url, digest)) => (Some(url), digest),
                    Err(e) => {
                        debug!(
                            "Failed to get GitHub release asset for {} on {}: {}",
                            self.id,
                            target.to_key(),
                            e
                        );
                        (None, None)
                    }
                }
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

        Ok(PlatformInfo {
            url,
            checksum,
            size: None,
            url_api: None,
            conda_deps: None,
        })
    }
}

impl AquaBackend {
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
            version_tags_cache: CacheManagerBuilder::new(cache_path.join("version_tags.msgpack.z"))
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                .build(),
            bin_path_caches: Default::default(),
        }
    }

    async fn get_version_tags(&self) -> Result<&Vec<(String, String)>> {
        self.version_tags_cache
            .get_or_try_init_async(|| async {
                let pkg = AQUA_REGISTRY.package(&self.id).await?;
                let mut versions = Vec::new();
                if !pkg.repo_owner.is_empty() && !pkg.repo_name.is_empty() {
                    let tags = get_tags(&pkg).await?;
                    for tag in tags.into_iter().rev() {
                        let mut version = tag.as_str();
                        match pkg.version_filter_ok(version) {
                            Ok(true) => {}
                            Ok(false) => continue,
                            Err(e) => {
                                warn!("[{}] aqua version filter error: {e}", self.ba());
                                continue;
                            }
                        }
                        let pkg = pkg.clone().with_version(&[version], os(), arch());
                        if let Some(prefix) = &pkg.version_prefix {
                            if let Some(_v) = version.strip_prefix(prefix) {
                                version = _v;
                            } else {
                                continue;
                            }
                        }
                        version = version.strip_prefix('v').unwrap_or(version);
                        versions.push((version.to_string(), tag));
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
        let asset_strs = pkg.asset_strs(v, os(), arch())?;
        self.github_release_asset(pkg, v, asset_strs).await
    }

    async fn github_release_asset(
        &self,
        pkg: &AquaPackage,
        v: &str,
        asset_strs: IndexSet<String>,
    ) -> Result<(String, Option<String>)> {
        let gh_id = format!("{}/{}", pkg.repo_owner, pkg.repo_name);
        let gh_release = github::get_release(&gh_id, v).await?;

        // Prioritize order of asset_strs
        let asset = asset_strs
            .iter()
            .find_map(|expected| {
                gh_release.assets.iter().find(|a| {
                    a.name == *expected || a.name.to_lowercase() == expected.to_lowercase()
                })
            })
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

    /// Download a URL to a path, or convert a local path string to PathBuf.
    /// Returns the path where the file is located.
    async fn download_url_to_path(
        &self,
        url: &str,
        download_path: &Path,
        ctx: &InstallContext,
    ) -> Result<PathBuf> {
        if url.starts_with("http") {
            let path = download_path.join(get_filename_from_url(url));
            HTTP.download_file(url, &path, Some(ctx.pr.as_ref()))
                .await?;
            Ok(path)
        } else {
            Ok(PathBuf::from(url))
        }
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
        self.verify_slsa(ctx, tv, pkg, v, filename).await?;
        self.verify_minisign(ctx, tv, pkg, v, filename).await?;
        self.verify_github_attestations(ctx, tv, pkg, v, filename)
            .await?;

        let download_path = tv.download_path();
        let platform_key = self.get_platform_key();
        let platform_info = tv.lock_platforms.entry(platform_key).or_default();
        if platform_info.checksum.is_none()
            && let Some(checksum) = &pkg.checksum
            && checksum.enabled()
        {
            let url = match checksum._type() {
                AquaChecksumType::GithubRelease => {
                    let asset_strs = checksum.asset_strs(pkg, v, os(), arch())?;
                    self.github_release_asset(pkg, v, asset_strs).await?.0
                }
                AquaChecksumType::Http => checksum.url(pkg, v, os(), arch())?,
            };
            let checksum_path = download_path.join(format!("{filename}.checksum"));
            HTTP.download_file(&url, &checksum_path, Some(ctx.pr.as_ref()))
                .await?;
            self.cosign_checksums(ctx, pkg, v, tv, &checksum_path, &download_path)
                .await?;
            let checksum_content = file::read_to_string(&checksum_path)?;
            let checksum_str =
                self.parse_checksum_from_content(&checksum_content, checksum, filename)?;
            let checksum_val = format!("{}:{}", checksum.algorithm(), checksum_str);
            // Now set the checksum after all borrows are done
            let platform_key = self.get_platform_key();
            let platform_info = tv.lock_platforms.get_mut(&platform_key).unwrap();
            platform_info.checksum = Some(checksum_val);
        }
        let tarball_path = tv.download_path().join(filename);
        self.verify_checksum(ctx, tv, &tarball_path)?;
        Ok(())
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
            let sig_path = match minisign._type() {
                AquaMinisignType::GithubRelease => {
                    let asset = minisign.asset(pkg, v, os(), arch())?;
                    let (repo_owner, repo_name) = resolve_repo_info(
                        minisign.repo_owner.as_ref(),
                        minisign.repo_name.as_ref(),
                        pkg,
                    );
                    let url = github::get_release(&format!("{repo_owner}/{repo_name}"), v)
                        .await?
                        .assets
                        .into_iter()
                        .find(|a| a.name == asset)
                        .map(|a| a.browser_download_url);
                    if let Some(url) = url {
                        let path = tv.download_path().join(asset);
                        HTTP.download_file(&url, &path, Some(ctx.pr.as_ref()))
                            .await?;
                        path
                    } else {
                        warn!("no asset found for minisign of {tv}: {asset}");
                        return Ok(());
                    }
                }
                AquaMinisignType::Http => {
                    let url = minisign.url(pkg, v, os(), arch())?;
                    let path = tv.download_path().join(filename).with_extension(".minisig");
                    HTTP.download_file(&url, &path, Some(ctx.pr.as_ref()))
                        .await?;
                    path
                }
            };
            let data = file::read(tv.download_path().join(filename))?;
            let sig = file::read_to_string(sig_path)?;
            minisign::verify(&minisign.public_key(pkg, v, os(), arch())?, &data, &sig)?;
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

            // Download the provenance file
            let mut slsa_pkg = pkg.clone();
            (slsa_pkg.repo_owner, slsa_pkg.repo_name) =
                resolve_repo_info(slsa.repo_owner.as_ref(), slsa.repo_name.as_ref(), pkg);

            let provenance_path = match slsa.r#type.as_deref().unwrap_or_default() {
                "github_release" => {
                    let asset_strs = slsa.asset_strs(pkg, v, os(), arch())?;
                    if asset_strs.is_empty() {
                        warn!("no asset configured for slsa verification of {tv}");
                        return Ok(());
                    }
                    match self.github_release_asset(&slsa_pkg, v, asset_strs).await {
                        Ok((url, _)) => {
                            let asset_filename = get_filename_from_url(&url);
                            let path = tv.download_path().join(asset_filename);
                            HTTP.download_file(&url, &path, Some(ctx.pr.as_ref()))
                                .await?;
                            path
                        }
                        Err(e) => {
                            warn!("no asset found for slsa verification of {tv}: {e}");
                            return Ok(());
                        }
                    }
                }
                "http" => {
                    let url = slsa.url(pkg, v, os(), arch())?;
                    let path = tv.download_path().join(get_filename_from_url(&url));
                    HTTP.download_file(&url, &path, Some(ctx.pr.as_ref()))
                        .await?;
                    path
                }
                t => {
                    warn!("unsupported slsa type: {t}");
                    return Ok(());
                }
            };

            let artifact_path = tv.download_path().join(filename);

            // Use native sigstore-verification crate for SLSA verification
            // Default to SLSA level 1 (sops provides level 1, newer tools provide level 2+)
            let min_level = 1u8;

            match sigstore_verification::verify_slsa_provenance(
                &artifact_path,
                &provenance_path,
                min_level,
            )
            .await
            {
                Ok(true) => {
                    ctx.pr
                        .set_message(format!("✓ SLSA provenance verified (level {})", min_level));
                    debug!(
                        "SLSA provenance verified successfully for {tv} at level {}",
                        min_level
                    );
                }
                Ok(false) => {
                    return Err(eyre!("SLSA provenance verification failed for {tv}"));
                }
                Err(e) => {
                    // Use proper error type matching instead of string matching
                    match &e {
                        sigstore_verification::AttestationError::NoAttestations => {
                            // SLSA verification was explicitly configured but attestations are missing
                            // This should be treated as a security failure, not a warning
                            return Err(eyre!(
                                "SLSA verification failed for {tv}: Package configuration requires SLSA provenance but no attestations found"
                            ));
                        }
                        _ => {
                            return Err(eyre!("SLSA verification error for {tv}: {e}"));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn verify_github_attestations(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        pkg: &AquaPackage,
        _v: &str,
        filename: &str,
    ) -> Result<()> {
        // Check if attestations are enabled via global and aqua-specific settings
        let settings = Settings::get();
        if !settings.github_attestations || !settings.aqua.github_attestations {
            debug!("GitHub attestations verification disabled");
            return Ok(());
        }

        if let Some(github_attestations) = &pkg.github_artifact_attestations {
            if github_attestations.enabled == Some(false) {
                debug!("GitHub attestations verification is disabled for {tv}");
                return Ok(());
            }

            ctx.pr.set_message("verify GitHub attestations".to_string());

            let artifact_path = tv.download_path().join(filename);

            // Get expected workflow from registry
            let signer_workflow = pkg
                .github_artifact_attestations
                .as_ref()
                .and_then(|att| att.signer_workflow.clone());

            match sigstore_verification::verify_github_attestation(
                &artifact_path,
                &pkg.repo_owner,
                &pkg.repo_name,
                env::GITHUB_TOKEN.as_deref(),
                signer_workflow.as_deref(),
            )
            .await
            {
                Ok(true) => {
                    ctx.pr
                        .set_message("✓ GitHub attestations verified".to_string());
                    debug!("GitHub attestations verified successfully for {tv}");
                }
                Ok(false) => {
                    return Err(eyre!(
                        "GitHub attestations verification returned false for {tv}"
                    ));
                }
                Err(sigstore_verification::AttestationError::NoAttestations) => {
                    return Err(eyre!(
                        "No GitHub attestations found for {tv}, but attestations are expected per aqua registry configuration"
                    ));
                }
                Err(e) => {
                    return Err(eyre!(
                        "GitHub attestations verification failed for {tv}: {e}"
                    ));
                }
            }
        }

        Ok(())
    }

    async fn cosign_checksums(
        &self,
        ctx: &InstallContext,
        pkg: &AquaPackage,
        v: &str,
        tv: &ToolVersion,
        checksum_path: &Path,
        download_path: &Path,
    ) -> Result<()> {
        if !Settings::get().aqua.cosign {
            return Ok(());
        }
        if let Some(cosign) = pkg.checksum.as_ref().and_then(|c| c.cosign.as_ref()) {
            if cosign.enabled == Some(false) {
                debug!("cosign is disabled for {tv}");
                return Ok(());
            }

            ctx.pr
                .set_message("verify checksums with cosign".to_string());

            // Use native sigstore-verification crate
            if let Some(key) = &cosign.key {
                // Key-based verification
                let mut key_pkg = pkg.clone();
                (key_pkg.repo_owner, key_pkg.repo_name) =
                    resolve_repo_info(key.repo_owner.as_ref(), key.repo_name.as_ref(), pkg);
                let key_arg = match key.r#type.as_deref().unwrap_or_default() {
                    "github_release" => {
                        let asset_strs = key.asset_strs(pkg, v, os(), arch())?;
                        if asset_strs.is_empty() {
                            String::new()
                        } else {
                            self.github_release_asset(&key_pkg, v, asset_strs).await?.0
                        }
                    }
                    "http" => key.url(pkg, v, os(), arch())?,
                    t => {
                        warn!(
                            "unsupported cosign key type for {}/{}: {t}",
                            pkg.repo_owner, pkg.repo_name
                        );
                        String::new()
                    }
                };
                if !key_arg.is_empty() {
                    // Download or locate the public key
                    let key_path = self
                        .download_url_to_path(&key_arg, download_path, ctx)
                        .await?;

                    // Download signature if specified
                    let sig_path = if let Some(signature) = &cosign.signature {
                        let mut sig_pkg = pkg.clone();
                        (sig_pkg.repo_owner, sig_pkg.repo_name) = resolve_repo_info(
                            signature.repo_owner.as_ref(),
                            signature.repo_name.as_ref(),
                            pkg,
                        );
                        let sig_arg = match signature.r#type.as_deref().unwrap_or_default() {
                            "github_release" => {
                                let asset_strs = signature.asset_strs(pkg, v, os(), arch())?;
                                if asset_strs.is_empty() {
                                    String::new()
                                } else {
                                    self.github_release_asset(&sig_pkg, v, asset_strs).await?.0
                                }
                            }
                            "http" => signature.url(pkg, v, os(), arch())?,
                            t => {
                                warn!(
                                    "unsupported cosign signature type for {}/{}: {t}",
                                    pkg.repo_owner, pkg.repo_name
                                );
                                String::new()
                            }
                        };
                        if !sig_arg.is_empty() {
                            self.download_url_to_path(&sig_arg, download_path, ctx)
                                .await?
                        } else {
                            // Default signature path
                            checksum_path.with_extension("sig")
                        }
                    } else {
                        // Default signature path
                        checksum_path.with_extension("sig")
                    };

                    // Verify with key
                    match sigstore_verification::verify_cosign_signature_with_key(
                        checksum_path,
                        &sig_path,
                        &key_path,
                    )
                    .await
                    {
                        Ok(true) => {
                            ctx.pr
                                .set_message("✓ Cosign signature verified with key".to_string());
                            debug!("Cosign signature verified successfully with key for {tv}");
                        }
                        Ok(false) => {
                            return Err(eyre!("Cosign signature verification failed for {tv}"));
                        }
                        Err(e) => {
                            return Err(eyre!("Cosign verification error for {tv}: {e}"));
                        }
                    }
                }
            } else if let Some(bundle) = &cosign.bundle {
                // Bundle-based keyless verification
                let mut bundle_pkg = pkg.clone();
                (bundle_pkg.repo_owner, bundle_pkg.repo_name) =
                    resolve_repo_info(bundle.repo_owner.as_ref(), bundle.repo_name.as_ref(), pkg);
                let bundle_arg = match bundle.r#type.as_deref().unwrap_or_default() {
                    "github_release" => {
                        let asset_strs = bundle.asset_strs(pkg, v, os(), arch())?;
                        if asset_strs.is_empty() {
                            String::new()
                        } else {
                            self.github_release_asset(&bundle_pkg, v, asset_strs)
                                .await?
                                .0
                        }
                    }
                    "http" => bundle.url(pkg, v, os(), arch())?,
                    t => {
                        warn!(
                            "unsupported cosign bundle type for {}/{}: {t}",
                            pkg.repo_owner, pkg.repo_name
                        );
                        String::new()
                    }
                };
                if !bundle_arg.is_empty() {
                    let bundle_path = self
                        .download_url_to_path(&bundle_arg, download_path, ctx)
                        .await?;

                    // Verify with bundle (keyless)
                    match sigstore_verification::verify_cosign_signature(
                        checksum_path,
                        &bundle_path,
                    )
                    .await
                    {
                        Ok(true) => {
                            ctx.pr
                                .set_message("✓ Cosign bundle verified (keyless)".to_string());
                            debug!("Cosign bundle verified successfully for {tv}");
                        }
                        Ok(false) => {
                            return Err(eyre!("Cosign bundle verification failed for {tv}"));
                        }
                        Err(e) => {
                            return Err(eyre!("Cosign bundle verification error for {tv}: {e}"));
                        }
                    }
                }
            }
        }
        Ok(())
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
            .map(|path| {
                if cfg!(windows) && pkg.complete_windows_ext {
                    path.with_extension("exe")
                } else {
                    path
                }
            })
            .collect();
        let first_bin_path = bin_paths
            .first()
            .expect("at least one bin path should exist");
        let tar_opts = TarOptions {
            format: format.parse().unwrap_or_default(),
            pr: Some(ctx.pr.as_ref()),
            strip_components: 0,
            ..Default::default()
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

        let srcs = self.srcs(pkg, tv)?;
        for (src, dst) in &srcs {
            if src != dst && src.exists() && !dst.exists() {
                if cfg!(windows) {
                    file::copy(src, dst)?;
                } else {
                    let src = PathBuf::from(".").join(src.file_name().unwrap().to_str().unwrap());
                    file::make_symlink(&src, dst)?;
                }
            }
        }

        if self.symlink_bins(tv) {
            self.create_symlink_bin_dir(tv, &srcs)?;
        }

        Ok(())
    }

    /// Creates a `.mise-bins` directory with symlinks only to the binaries defined in the aqua registry.
    /// This prevents bundled dependencies (like Python in aws-cli) from being exposed on PATH.
    fn create_symlink_bin_dir(&self, tv: &ToolVersion, srcs: &[(PathBuf, PathBuf)]) -> Result<()> {
        let symlink_dir = tv.install_path().join(".mise-bins");
        file::create_dir_all(&symlink_dir)?;

        for (_, dst) in srcs {
            if let Some(bin_name) = dst.file_name() {
                let symlink_path = symlink_dir.join(bin_name);
                if dst.exists() && !symlink_path.exists() {
                    file::make_symlink_or_copy(dst, &symlink_path)?;
                }
            }
        }
        Ok(())
    }

    fn symlink_bins(&self, tv: &ToolVersion) -> bool {
        tv.request
            .options()
            .get("symlink_bins")
            .is_some_and(|v| v == "true" || v == "1")
    }

    fn srcs(&self, pkg: &AquaPackage, tv: &ToolVersion) -> Result<Vec<(PathBuf, PathBuf)>> {
        let files: Vec<(PathBuf, PathBuf)> = pkg
            .files
            .iter()
            .map(|f| {
                let srcs = if let Some(prefix) = &pkg.version_prefix {
                    vec![f.src(pkg, &format!("{}{}", prefix, tv.version), os(), arch())?]
                } else {
                    vec![
                        f.src(pkg, &tv.version, os(), arch())?,
                        f.src(pkg, &format!("v{}", tv.version), os(), arch())?,
                    ]
                };
                Ok(srcs
                    .into_iter()
                    .flatten()
                    .map(|src| tv.install_path().join(src))
                    .map(|mut src| {
                        let mut dst = src.parent().unwrap().join(f.name.as_str());
                        if cfg!(windows) && pkg.complete_windows_ext {
                            src = src.with_extension("exe");
                            dst = dst.with_extension("exe");
                        }
                        (src, dst)
                    }))
            })
            .flatten_ok()
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .unique_by(|(src, _)| src.to_path_buf())
            .collect();
        Ok(files)
    }
}

async fn get_tags(pkg: &AquaPackage) -> Result<Vec<String>> {
    Ok(get_tags_with_created_at(pkg)
        .await?
        .into_iter()
        .map(|(tag, _)| tag)
        .collect())
}

/// Get tags with optional created_at timestamps.
/// Returns (tag_name, Option<created_at>) pairs.
async fn get_tags_with_created_at(pkg: &AquaPackage) -> Result<Vec<(String, Option<String>)>> {
    if let Some("github_tag") = pkg.version_source.as_deref() {
        // Tags don't have created_at timestamps
        let versions = github::list_tags(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
        return Ok(versions.into_iter().map(|v| (v, None)).collect());
    }
    let releases = github::list_releases(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
    if releases.is_empty() {
        // Fall back to tags (no timestamps)
        let versions = github::list_tags(&format!("{}/{}", pkg.repo_owner, pkg.repo_name)).await?;
        return Ok(versions.into_iter().map(|v| (v, None)).collect());
    }
    Ok(releases
        .into_iter()
        .map(|r| (r.tag_name, Some(r.created_at)))
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
        AquaPackageType::GoInstall => {
            bail!(
                "package type `go_install` is not supported in the aqua backend. Use the go backend instead{}.",
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
