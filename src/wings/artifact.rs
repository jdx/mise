//! Catalog resolve + OCI registry install path for mise-wings.

use std::{path::Path, time::Duration};

use eyre::{Context, Result, bail, ensure, eyre};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::backend::Backend;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::{get_filename_from_url, install_artifact};
use crate::file::TarFormat;
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::wildcard::Wildcard;
use crate::wings::client;

pub(crate) const MEDIA_TYPE_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub(crate) const MEDIA_TYPE_OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub(crate) const MISE_SOURCE_LAYER_MEDIA_TYPE: &str = "application/vnd.mise-wings.artifact.v1";
const OCI_TAR_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
const OCI_TAR_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const DOCKER_TAR_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";
const RESOLVE_PENDING_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const RESOLVE_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub async fn try_install<B: Backend + ?Sized>(
    backend: &B,
    ctx: &InstallContext,
    tv: &mut ToolVersion,
) -> Result<bool> {
    if !crate::config::Settings::get().wings.enabled {
        return Ok(false);
    }
    if !tv.request.options().wings_enabled() {
        return Ok(false);
    }

    let fallback_blocked = fallback_blocked(tv);
    let Some(token) = crate::wings::auth::session_token().await? else {
        return fallback_or_false(
            tv,
            fallback_blocked,
            "wings authentication is not available",
        );
    };

    let Some(source) = (match resolve_source(backend, tv).await {
        Ok(source) => source,
        Err(e) => {
            return fallback_or_error(tv, fallback_blocked, "wings source resolution failed", e);
        }
    }) else {
        return fallback_or_false(
            tv,
            fallback_blocked,
            "wings could not resolve an installable source",
        );
    };

    ctx.pr.set_message("wings resolve".into());
    let Some(artifact) = (match resolve_until_allowed(backend, ctx, tv, &source, &token).await {
        Ok(artifact) => artifact,
        Err(e) if e.downcast_ref::<WingsBlockedError>().is_some() => return Err(e),
        Err(e) => return fallback_or_error(tv, fallback_blocked, "wings resolve failed", e),
    }) else {
        return fallback_or_false(tv, fallback_blocked, "wings did not provide an artifact");
    };

    ctx.pr.set_message("wings pull oci".into());
    if let Err(e) = pull_and_install(ctx, tv, &artifact, &token).await {
        if fallback_blocked {
            return Err(e.wrap_err(format!(
                "wings install failed for {}, and wings.required blocks fallback to the normal installer",
                tv.style()
            )));
        }
        log::warn!("wings install failed; falling back to normal installer: {e:#}");
        backend.cleanup_install_dirs_on_error(tv);
        backend.create_install_dirs(tv)?;
        return Ok(false);
    }
    Ok(true)
}

pub fn fallback_blocked(tv: &ToolVersion) -> bool {
    if !crate::config::Settings::get().wings.enabled {
        return false;
    }
    let required = &crate::config::Settings::get().wings.required;
    fallback_required(tv, required.as_deref().unwrap_or(&[]))
}

fn fallback_required(tv: &ToolVersion, required: &[String]) -> bool {
    if !tv.request.options().wings_enabled() || required.is_empty() {
        return false;
    }

    let matcher = Wildcard::new(required);
    fallback_match_candidates(tv)
        .iter()
        .any(|candidate| matcher.match_any(candidate))
}

fn fallback_match_candidates(tv: &ToolVersion) -> Vec<String> {
    let full = tv.ba().full_without_opts();
    vec![tv.short().to_string(), full]
}

fn fallback_or_false(tv: &ToolVersion, fallback_blocked: bool, reason: &str) -> Result<bool> {
    if fallback_blocked {
        bail!(
            "{reason} for {}, and wings.required blocks fallback to the normal installer",
            tv.style()
        );
    }
    log::warn!("{reason}; falling back to normal installer");
    Ok(false)
}

fn fallback_or_error(
    tv: &ToolVersion,
    fallback_blocked: bool,
    context: &str,
    err: eyre::Report,
) -> Result<bool> {
    if fallback_blocked {
        return Err(err.wrap_err(format!(
            "{context} for {}, and wings.required blocks fallback to the normal installer",
            tv.style()
        )));
    }
    log::warn!("{context}; falling back to normal installer: {err:#}");
    Ok(false)
}

#[derive(Clone)]
struct SourceInfo {
    url: String,
    checksum: Option<String>,
}

async fn resolve_source<B: Backend + ?Sized>(
    backend: &B,
    tv: &mut ToolVersion,
) -> Result<Option<SourceInfo>> {
    let target = PlatformTarget::from_current();
    let platform_key = target.to_key();

    if let Some(info) = tv.lock_platforms.get(&platform_key)
        && let Some(source) = source_from_platform_info(info)
    {
        return Ok(Some(source));
    }

    let info = backend.resolve_lock_info(tv, &target).await?;
    if let Some(source) = source_from_platform_info(&info) {
        tv.lock_platforms.insert(platform_key, info);
        return Ok(Some(source));
    }

    Ok(None)
}

fn source_from_platform_info(info: &PlatformInfo) -> Option<SourceInfo> {
    let url = info.url.as_ref().or(info.url_api.as_ref())?;
    Some(SourceInfo {
        url: url.clone(),
        checksum: info.checksum.clone(),
    })
}

#[derive(Serialize)]
struct ResolveRequest {
    backend: String,
    tool: String,
    requested_version: String,
    resolved_version: String,
    os: String,
    arch: String,
    libc: Option<String>,
    source: ResolveSource,
    context: PolicyContext,
}

#[derive(Serialize)]
struct ResolveSource {
    url: String,
    checksum: Option<String>,
    published_at: Option<String>,
}

#[derive(Serialize)]
struct PolicyContext {
    repository: Option<String>,
    teams: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
enum ResolveResponse {
    Allow {
        artifact: Artifact,
    },
    Blocked {
        reason: String,
        #[serde(default)]
        job: Option<PendingJob>,
    },
    Pending {
        job: PendingJob,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct Artifact {
    transport: String,
    #[serde(rename = "ref")]
    ref_: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct PendingJob {
    id: String,
    status: String,
    #[serde(rename = "progressPercent")]
    progress_percent: u8,
    message: String,
}

async fn resolve_until_allowed<B: Backend + ?Sized>(
    backend: &B,
    ctx: &InstallContext,
    tv: &ToolVersion,
    source: &SourceInfo,
    token: &str,
) -> Result<Option<Artifact>> {
    let target = PlatformTarget::from_current();
    let body = ResolveRequest {
        backend: backend.get_type().to_string(),
        tool: backend.tool_name(),
        requested_version: tv.request.version(),
        resolved_version: tv.version.clone(),
        os: target.os_name().to_string(),
        arch: target.arch_name().to_string(),
        libc: target.libc().map(normalize_libc),
        source: ResolveSource {
            url: source.url.clone(),
            checksum: source.checksum.clone(),
            published_at: None,
        },
        context: PolicyContext {
            repository: None,
            teams: vec![],
        },
    };

    let url = format!("https://api.{}/v1/catalog/resolve", crate::wings::host());
    let headers = bearer_headers(token)?;
    let deadline = tokio::time::Instant::now() + RESOLVE_PENDING_TIMEOUT;
    loop {
        if crate::ui::ctrlc::is_cancelled() {
            bail!("wings resolve cancelled by user");
        }
        if tokio::time::Instant::now() >= deadline {
            log::warn!(
                "wings resolve timed out after {}s waiting for artifact build; the server-side job may still be running",
                RESOLVE_PENDING_TIMEOUT.as_secs()
            );
            return Ok(None);
        }
        // Re-posting the same resolve request is the poll contract: the
        // server deduplicates pending/running jobs by
        // (org, backend, tool, version, platform) and returns the existing
        // job/progress until the catalog row is available.
        let response: ResolveResponse = match client::post_json(&url, &body, &headers).await {
            Ok(response) => response,
            Err(e) => return Err(e.wrap_err("wings resolver request failed")),
        };
        match response {
            ResolveResponse::Allow { artifact } => {
                ensure!(
                    artifact.transport == "oci",
                    "wings resolver returned unsupported transport {}",
                    artifact.transport
                );
                return Ok(Some(artifact));
            }
            ResolveResponse::Blocked { reason, job } => {
                if let Some(job) = job {
                    ctx.pr.set_message(format!(
                        "wings blocked {}% {}",
                        job.progress_percent,
                        progress_message(&job)
                    ));
                }
                return Err(WingsBlockedError {
                    tool: tv.style().to_string(),
                    reason,
                }
                .into());
            }
            ResolveResponse::Pending { job } => {
                ctx.pr.set_message(format!(
                    "wings build {}% {}",
                    job.progress_percent,
                    progress_message(&job)
                ));
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(remaining.min(RESOLVE_POLL_INTERVAL)).await;
            }
        }
    }
}

fn normalize_libc(libc: &str) -> String {
    match libc {
        "gnu" => "glibc".to_string(),
        other => other.to_string(),
    }
}

fn progress_message(job: &PendingJob) -> String {
    if job.message.is_empty() {
        format!("{} {}", job.id, job.status)
    } else {
        job.message.clone()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("wings blocked {tool}: {reason}")]
struct WingsBlockedError {
    tool: String,
    reason: String,
}

async fn pull_and_install(
    ctx: &InstallContext,
    tv: &ToolVersion,
    artifact: &Artifact,
    token: &str,
) -> Result<()> {
    let reference = WingsReference::parse(&artifact.ref_)?;
    let expected_registry = format!("registry.{}", crate::wings::host());
    ensure!(
        reference.registry == expected_registry,
        "wings resolver returned registry {}; expected {}",
        reference.registry,
        expected_registry
    );
    let manifest_url = format!(
        "https://{}/v2/{}/manifests/{}",
        reference.registry, reference.repository, artifact.digest
    );
    let headers = registry_headers(token, &[MEDIA_TYPE_OCI_MANIFEST])?;
    let manifest_bytes = crate::http::HTTP
        .get_bytes_with_headers(&manifest_url, &headers)
        .await
        .wrap_err_with(|| format!("fetching wings OCI manifest {}", artifact.ref_))?;
    let manifest_bytes = manifest_bytes.as_ref();
    ensure_digest(manifest_bytes, &artifact.digest, "manifest")?;
    let manifest: WingsManifest =
        serde_json::from_slice(manifest_bytes).wrap_err("decoding wings OCI manifest")?;
    ensure!(
        manifest.layers.len() == 1,
        "wings artifact {} has {} layers; expected 1",
        artifact.ref_,
        manifest.layers.len()
    );
    let layer = &manifest.layers[0];
    let blob_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.registry, reference.repository, layer.digest
    );
    let filename = filename_for_layer(&manifest, layer)?;
    let layer_path = tv.download_path().join(filename);

    ctx.pr.set_message("wings download".into());
    let blob_headers = registry_headers(token, &[])?;
    crate::http::HTTP
        .download_file_with_headers(&blob_url, &layer_path, &blob_headers, Some(ctx.pr.as_ref()))
        .await
        .wrap_err_with(|| format!("fetching wings OCI layer {}", layer.digest))?;
    ensure_file_digest(&layer_path, &layer.digest, "layer")?;

    ctx.pr.next_operation();
    ctx.pr.set_message("wings install".into());
    let opts = tv.request.options();
    match layer.media_type.as_str() {
        OCI_TAR_GZIP_LAYER_MEDIA_TYPE
        | DOCKER_TAR_GZIP_LAYER_MEDIA_TYPE
        | OCI_TAR_LAYER_MEDIA_TYPE
        | MISE_SOURCE_LAYER_MEDIA_TYPE => {
            install_artifact(tv, &layer_path, &opts, Some(ctx.pr.as_ref()))?;
        }
        other => bail!("wings artifact layer media type {other} is not installable"),
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WingsManifest {
    layers: Vec<WingsDescriptor>,
    #[serde(default)]
    annotations: indexmap::IndexMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct WingsDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
}

fn filename_for_layer(manifest: &WingsManifest, layer: &WingsDescriptor) -> Result<String> {
    if let Some(filename) = manifest
        .annotations
        .get("dev.mise-wings.source.url")
        .map(|url| get_filename_from_url(url))
        .filter(|name| !name.is_empty())
    {
        ensure_installable_source_filename(&filename)?;
        return Ok(filename);
    }

    ensure!(
        layer.media_type != MISE_SOURCE_LAYER_MEDIA_TYPE,
        "wings source artifact layer is missing dev.mise-wings.source.url annotation"
    );

    Ok(filename_for_layer_media_type(&layer.media_type).to_string())
}

fn ensure_installable_source_filename(filename: &str) -> Result<()> {
    ensure!(
        filename != "." && filename != ".." && !filename.contains('/') && !filename.contains('\\'),
        "wings source artifact filename {filename:?} must not contain path separators or parent directory segments"
    );
    let format = TarFormat::from_file_name(filename);
    let has_extension = Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| !ext.is_empty());
    ensure!(
        format != TarFormat::Raw || !has_extension,
        "wings source artifact filename {filename:?} has unsupported archive extension; expected tar, tar.gz, tar.xz, tar.bz2, tar.zst, zip, 7z, gz, xz, bz2, zst, or an extensionless raw binary"
    );
    Ok(())
}

fn filename_for_layer_media_type(media_type: &str) -> &'static str {
    match media_type {
        OCI_TAR_GZIP_LAYER_MEDIA_TYPE | DOCKER_TAR_GZIP_LAYER_MEDIA_TYPE => "artifact.tar.gz",
        OCI_TAR_LAYER_MEDIA_TYPE => "artifact.tar",
        _ => "artifact",
    }
}

pub(crate) fn registry_headers(token: &str, accept: &[&str]) -> Result<HeaderMap> {
    let mut headers = bearer_headers(token)?;
    if !accept.is_empty() {
        headers.insert(ACCEPT, HeaderValue::from_str(&accept.join(", "))?);
    }
    Ok(headers)
}

fn bearer_headers(token: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}"))
            .wrap_err("wings token contains invalid header characters")?,
    );
    Ok(headers)
}

pub(crate) fn ensure_digest(bytes: &[u8], expected: &str, label: &str) -> Result<()> {
    let Some(expected) = expected.strip_prefix("sha256:") else {
        bail!("wings {label} digest is not sha256: {expected}");
    };
    let actual = hex_sha256(bytes);
    ensure!(
        actual == expected,
        "wings {label} digest mismatch: expected sha256:{expected}, got sha256:{actual}"
    );
    Ok(())
}

fn ensure_file_digest(path: &std::path::Path, expected: &str, label: &str) -> Result<()> {
    let Some(expected) = expected.strip_prefix("sha256:") else {
        bail!("wings {label} digest is not sha256: {expected}");
    };
    let actual = crate::hash::file_hash_sha256(path, None)?;
    ensure!(
        actual == expected,
        "wings {label} digest mismatch: expected sha256:{expected}, got sha256:{actual}"
    );
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WingsReference {
    pub(crate) registry: String,
    pub(crate) repository: String,
}

impl WingsReference {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        let (without_digest, _) = s.split_once('@').unwrap_or((s, ""));
        let without_tag = match without_digest.rsplit_once(':') {
            Some((name, tag)) if !tag.contains('/') => name,
            _ => without_digest,
        };
        let url = Url::parse(&format!("oci://{without_tag}"))
            .map_err(|e| eyre!("invalid wings OCI ref {s}: {e}"))?;
        let host = url
            .host_str()
            .ok_or_else(|| eyre!("invalid wings OCI ref {s}: missing registry"))?
            .to_string();
        let registry = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        };
        let repository = url.path().trim_start_matches('/').to_string();
        ensure!(
            !repository.is_empty(),
            "invalid wings OCI ref {s}: missing repository"
        );
        Ok(Self {
            registry,
            repository,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::BackendArg;
    use crate::toolset::{ToolRequest, ToolSource, ToolVersion, ToolVersionOptions};
    use std::sync::Arc;

    #[test]
    fn parses_tagged_wings_reference() {
        let r = WingsReference::parse("registry.example.com/acme/node:20").unwrap();
        assert_eq!(
            r,
            WingsReference {
                registry: "registry.example.com".into(),
                repository: "acme/node".into(),
            }
        );
    }

    #[test]
    fn parses_digest_wings_reference() {
        let r = WingsReference::parse("registry.example.com/acme/node@sha256:abc").unwrap();
        assert_eq!(r.registry, "registry.example.com");
        assert_eq!(r.repository, "acme/node");
    }

    #[test]
    fn parses_registry_port_in_wings_reference() {
        let r = WingsReference::parse("registry.example.com:5000/acme/node:1.0.0").unwrap();
        assert_eq!(r.registry, "registry.example.com:5000");
        assert_eq!(r.repository, "acme/node");
    }

    #[test]
    fn normalizes_gnu_libc_for_resolver() {
        assert_eq!(normalize_libc("gnu"), "glibc");
        assert_eq!(normalize_libc("musl"), "musl");
    }

    #[test]
    fn fallback_required_matches_tool_and_backend_patterns() {
        let tv = tool_version("node", Some("github:nodejs/node"));

        assert!(fallback_required(&tv, &["node".into()]));
        assert!(fallback_required(&tv, &["github:*".into()]));
        assert!(fallback_required(&tv, &["*".into()]));
        assert!(!fallback_required(&tv, &["github:?".into()]));
        assert!(!fallback_required(&tv, &["npm:*".into()]));
    }

    #[test]
    fn fallback_required_respects_tool_opt_out() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("wings".into(), toml::Value::Boolean(false));
        let tv = tool_version_with_options("node", Some("github:nodejs/node"), opts);

        assert!(!fallback_required(&tv, &["*".into()]));
    }

    #[test]
    fn source_layer_requires_source_url_for_filename() {
        let manifest = WingsManifest {
            layers: vec![],
            annotations: indexmap::IndexMap::new(),
        };
        let layer = WingsDescriptor {
            media_type: MISE_SOURCE_LAYER_MEDIA_TYPE.into(),
            digest: "sha256:abc".into(),
        };
        let err = filename_for_layer(&manifest, &layer).unwrap_err();
        assert!(
            err.to_string()
                .contains("missing dev.mise-wings.source.url annotation")
        );
    }

    #[test]
    fn source_layer_rejects_unknown_archive_extension() {
        let mut annotations = indexmap::IndexMap::new();
        annotations.insert(
            "dev.mise-wings.source.url".into(),
            "https://example.com/tool.deb".into(),
        );
        let manifest = WingsManifest {
            layers: vec![],
            annotations,
        };
        let layer = WingsDescriptor {
            media_type: MISE_SOURCE_LAYER_MEDIA_TYPE.into(),
            digest: "sha256:abc".into(),
        };

        let err = filename_for_layer(&manifest, &layer).unwrap_err();
        assert!(err.to_string().contains("unsupported archive extension"));
    }

    #[test]
    fn source_layer_rejects_path_like_source_filename() {
        let err = ensure_installable_source_filename("../tool.tar.gz").unwrap_err();
        assert!(err.to_string().contains("must not contain path separators"));

        let err = ensure_installable_source_filename("nested/tool.tar.gz").unwrap_err();
        assert!(err.to_string().contains("must not contain path separators"));

        let err = ensure_installable_source_filename("..").unwrap_err();
        assert!(err.to_string().contains("parent directory segments"));
    }

    #[test]
    fn source_layer_allows_dotdot_substring_in_filename() {
        ensure_installable_source_filename("node-v20..1.tar.gz").unwrap();
    }

    #[test]
    fn source_layer_allows_extensionless_raw_binary() {
        let mut annotations = indexmap::IndexMap::new();
        annotations.insert(
            "dev.mise-wings.source.url".into(),
            "https://example.com/tool".into(),
        );
        let manifest = WingsManifest {
            layers: vec![],
            annotations,
        };
        let layer = WingsDescriptor {
            media_type: MISE_SOURCE_LAYER_MEDIA_TYPE.into(),
            digest: "sha256:abc".into(),
        };

        assert_eq!(filename_for_layer(&manifest, &layer).unwrap(), "tool");
    }

    fn tool_version(short: &str, full: Option<&str>) -> ToolVersion {
        tool_version_with_options(short, full, ToolVersionOptions::default())
    }

    fn tool_version_with_options(
        short: &str,
        full: Option<&str>,
        options: ToolVersionOptions,
    ) -> ToolVersion {
        let backend = Arc::new(BackendArg::new(
            short.to_string(),
            full.map(ToString::to_string),
        ));
        let request = ToolRequest::Version {
            backend,
            version: "1.0.0".into(),
            options,
            source: ToolSource::Unknown,
        };
        ToolVersion::new(request, "1.0.0".into())
    }
}
