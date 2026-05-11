//! Catalog resolve + OCI registry install path for mise-wings.

use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use eyre::{Context, Result, bail, ensure, eyre};
use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::backend::Backend;
use crate::backend::platform_target::PlatformTarget;
use crate::backend::static_helpers::get_filename_from_url;
use crate::file::{self, TarFormat};
use crate::install_context::InstallContext;
use crate::lockfile::PlatformInfo;
use crate::toolset::ToolVersion;
use crate::wildcard::Wildcard;
use crate::wings::client;

pub(crate) const MEDIA_TYPE_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub(crate) const MEDIA_TYPE_OCI_IMAGE_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub(crate) const MOCITO_TOOL_ARTIFACT_TYPE: &str = "application/vnd.mise.tool.v1";
pub(crate) const MOCITO_TOOL_CONFIG_MEDIA_TYPE: &str = "application/vnd.mise.tool.config.v1+json";
const OCI_TAR_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar";
const OCI_TAR_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+gzip";
const OCI_TAR_ZSTD_LAYER_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+zstd";
const DOCKER_TAR_GZIP_LAYER_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";
const ZIP_LAYER_MEDIA_TYPE: &str = "application/zip";
const BINARY_LAYER_MEDIA_TYPE: &str = "application/octet-stream";
const MOCITO_BIN_DIR: &str = ".mise-bins";
const MOCITO_ENV_FILE: &str = ".mise-mocito-env.json";
const RESOLVE_PENDING_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const RESOLVE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const RESOLVE_TRANSIENT_RETRIES: u32 = 5;

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
            "wings authentication is not available; run `mise wings login` or configure GitHub Actions OIDC",
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
    let settings = crate::config::Settings::get();
    if !settings.wings.enabled {
        return false;
    }
    let required = &settings.wings.required;
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

#[derive(Debug, Clone)]
pub(crate) struct RebuildJob {
    pub id: String,
    pub status: String,
    pub progress_percent: u8,
    pub message: String,
    pub blocked_reason: Option<String>,
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

#[derive(Debug)]
struct ResolveTransientError(String);

impl std::fmt::Display for ResolveTransientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ResolveTransientError {}

#[derive(Debug, Deserialize)]
struct PendingJob {
    id: String,
    status: String,
    #[serde(rename = "progressPercent")]
    progress_percent: u8,
    message: String,
}

impl From<PendingJob> for RebuildJob {
    fn from(job: PendingJob) -> Self {
        Self {
            id: job.id,
            status: job.status,
            progress_percent: job.progress_percent,
            message: job.message,
            blocked_reason: None,
        }
    }
}

async fn resolve_until_allowed<B: Backend + ?Sized>(
    backend: &B,
    ctx: &InstallContext,
    tv: &ToolVersion,
    source: &SourceInfo,
    token: &str,
) -> Result<Option<Artifact>> {
    let body = resolve_request(backend, tv, source);
    let url = format!("https://api.{}/v1/catalog/resolve", crate::wings::host());
    let headers = bearer_headers(token)?;
    let deadline = tokio::time::Instant::now() + RESOLVE_PENDING_TIMEOUT;
    let mut transient_failures = 0;
    loop {
        if crate::ui::ctrlc::is_cancelled() {
            bail!("wings resolve cancelled by user");
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(eyre!(
                "wings resolve timed out after {}s waiting for artifact build; \
                 the server-side job may still be running",
                RESOLVE_PENDING_TIMEOUT.as_secs()
            ));
        }
        // Re-posting the same resolve request is the poll contract: the
        // server deduplicates pending/running jobs by
        // (org, backend, tool, version, platform) and returns the existing
        // job/progress until the catalog row is available.
        let response = match post_resolve(&url, &body, &headers).await {
            Ok(response) => {
                transient_failures = 0;
                response
            }
            Err(e) if e.downcast_ref::<ResolveTransientError>().is_some() => {
                transient_failures += 1;
                if transient_failures > RESOLVE_TRANSIENT_RETRIES {
                    return Err(e.wrap_err("wings resolver request failed"));
                }
                let delay = Duration::from_secs(2_u64.pow(transient_failures - 1))
                    .min(RESOLVE_POLL_INTERVAL);
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(remaining.min(delay)).await;
                continue;
            }
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

pub(crate) async fn rebuild<B: Backend + ?Sized>(
    backend: &B,
    tv: &mut ToolVersion,
) -> Result<RebuildJob> {
    let Some(token) = crate::wings::auth::session_token().await? else {
        bail!("wings authentication is not available; run `mise wings login`");
    };
    let Some(source) = resolve_source(backend, tv).await? else {
        bail!(
            "wings could not resolve an installable source for {}",
            tv.style()
        );
    };

    let body = resolve_request(backend, tv, &source);
    let url = format!("https://api.{}/v1/catalog/rebuild", crate::wings::host());
    let headers = bearer_headers(&token)?;
    match post_resolve(&url, &body, &headers)
        .await
        .wrap_err("wings rebuild request failed")?
    {
        ResolveResponse::Pending { job } => Ok(job.into()),
        ResolveResponse::Blocked { reason, job } => match job {
            Some(job) => Ok(RebuildJob {
                blocked_reason: Some(reason),
                ..job.into()
            }),
            None => Err(WingsBlockedError {
                tool: tv.style().to_string(),
                reason,
            }
            .into()),
        },
        ResolveResponse::Allow { .. } => {
            bail!("wings rebuild returned an existing artifact instead of a rebuild job")
        }
    }
}

fn resolve_request<B: Backend + ?Sized>(
    backend: &B,
    tv: &ToolVersion,
    source: &SourceInfo,
) -> ResolveRequest {
    let target = PlatformTarget::from_current();
    ResolveRequest {
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
    }
}

async fn post_resolve(
    url: &str,
    body: &ResolveRequest,
    headers: &HeaderMap,
) -> Result<ResolveResponse> {
    let resp = client::http_client()?
        .post(url)
        .headers(headers.clone())
        .json(body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() || e.is_connect() || e.is_request() {
                ResolveTransientError(format!("POST {url}: {e}")).into()
            } else {
                eyre!(e).wrap_err(format!("POST {url}"))
            }
        })?;
    let status = resp.status();
    if !status.is_success() {
        let response_body = resp.text().await.unwrap_or_default();
        if status.is_server_error() {
            return Err(ResolveTransientError(format!(
                "wings {url} returned {status}: {response_body}"
            ))
            .into());
        }
        bail!("wings {url} returned {status}: {response_body}");
    }
    resp.json()
        .await
        .wrap_err_with(|| format!("decoding {url} response body"))
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
    manifest.validate_mocito(&artifact.ref_)?;

    let config_url = format!(
        "https://{}/v2/{}/blobs/{}",
        reference.registry, reference.repository, manifest.config.digest
    );
    let blob_headers = registry_headers(token, &[])?;
    let config_bytes = crate::http::HTTP
        .get_bytes_with_headers(&config_url, &blob_headers)
        .await
        .wrap_err_with(|| format!("fetching wings MOCITO config {}", manifest.config.digest))?;
    let config_bytes = config_bytes.as_ref();
    manifest
        .config
        .ensure_bytes(config_bytes, "config")
        .wrap_err("verifying wings MOCITO config descriptor")?;
    let config: MocitoConfig =
        serde_json::from_slice(config_bytes).wrap_err("decoding wings MOCITO config")?;
    config.validate(tv)?;

    ctx.pr.set_message("wings download".into());
    let mut layers = Vec::with_capacity(manifest.layers.len());
    for (idx, layer) in manifest.layers.iter().enumerate() {
        let blob_url = format!(
            "https://{}/v2/{}/blobs/{}",
            reference.registry, reference.repository, layer.digest
        );
        let filename = filename_for_layer(&manifest, layer, idx)?;
        let layer_path = tv.download_path().join(filename);
        crate::http::HTTP
            .download_file_with_headers(
                &blob_url,
                &layer_path,
                &blob_headers,
                Some(ctx.pr.as_ref()),
            )
            .await
            .wrap_err_with(|| format!("fetching wings OCI layer {}", layer.digest))?;
        layer
            .ensure_file(&layer_path, "layer")
            .wrap_err_with(|| format!("verifying wings OCI layer {}", layer.digest))?;
        layers.push((layer, layer_path));
    }

    ctx.pr.next_operation();
    ctx.pr.set_message("wings install".into());
    install_mocito_artifact(tv, &config, &layers, Some(ctx.pr.as_ref()))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct WingsManifest {
    #[serde(rename = "artifactType")]
    artifact_type: String,
    config: WingsDescriptor,
    layers: Vec<WingsDescriptor>,
    #[serde(default)]
    annotations: indexmap::IndexMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct WingsDescriptor {
    #[serde(rename = "mediaType")]
    media_type: String,
    digest: String,
    size: u64,
}

impl WingsManifest {
    fn validate_mocito(&self, artifact_ref: &str) -> Result<()> {
        ensure!(
            self.artifact_type == MOCITO_TOOL_ARTIFACT_TYPE,
            "wings artifact {artifact_ref} has artifactType {}; expected {}",
            self.artifact_type,
            MOCITO_TOOL_ARTIFACT_TYPE
        );
        ensure!(
            self.config.media_type == MOCITO_TOOL_CONFIG_MEDIA_TYPE,
            "wings artifact {artifact_ref} config has media type {}; expected {}",
            self.config.media_type,
            MOCITO_TOOL_CONFIG_MEDIA_TYPE
        );
        ensure!(
            !self.layers.is_empty(),
            "wings artifact {artifact_ref} has no payload layers"
        );
        for layer in &self.layers {
            ensure_installable_layer_media_type(&layer.media_type)?;
        }
        Ok(())
    }
}

impl WingsDescriptor {
    fn ensure_bytes(&self, bytes: &[u8], label: &str) -> Result<()> {
        ensure!(
            bytes.len() as u64 == self.size,
            "wings {label} size mismatch: expected {}, got {}",
            self.size,
            bytes.len()
        );
        ensure_digest(bytes, &self.digest, label)
    }

    fn ensure_file(&self, path: &Path, label: &str) -> Result<()> {
        let size = std::fs::metadata(path)?.len();
        ensure!(
            size == self.size,
            "wings {label} size mismatch: expected {}, got {}",
            self.size,
            size
        );
        ensure_file_digest(path, &self.digest, label)
    }
}

#[derive(Debug, Deserialize)]
struct MocitoConfig {
    #[serde(rename = "schemaVersion")]
    schema_version: u8,
    namespace: String,
    tool: String,
    version: String,
    platform: String,
    #[serde(default, rename = "stripComponents")]
    strip_components: usize,
    #[serde(default)]
    bin: Vec<String>,
    #[serde(default, rename = "binLinks")]
    bin_links: Vec<MocitoBinLink>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MocitoBinLink {
    name: String,
    src: Option<String>,
    link: Option<String>,
    #[serde(default)]
    hard: bool,
}

impl MocitoConfig {
    fn validate(&self, tv: &ToolVersion) -> Result<()> {
        ensure!(
            self.schema_version == 1,
            "wings MOCITO config schemaVersion {} is not supported",
            self.schema_version
        );
        ensure!(
            !self.namespace.trim().is_empty(),
            "wings MOCITO config namespace is empty"
        );
        ensure!(
            !self.tool.trim().is_empty(),
            "wings MOCITO config tool is empty"
        );
        ensure!(
            self.version == tv.version,
            "wings MOCITO config version {} does not match requested {}",
            self.version,
            tv.version
        );
        let expected_platform = PlatformTarget::from_current().to_key();
        ensure!(
            self.platform == expected_platform,
            "wings MOCITO config platform {} does not match current platform {}",
            self.platform,
            expected_platform
        );
        for bin in &self.bin {
            safe_relative_path(bin.trim_end_matches('/'), "bin")?;
        }
        for link in &self.bin_links {
            link.validate()?;
        }
        validate_mocito_env(&self.env)?;
        Ok(())
    }

    fn raw_binary_dest(&self) -> Result<PathBuf> {
        if let Some(bin) = self.bin.iter().find(|bin| !bin.ends_with('/')) {
            return safe_relative_path(bin, "bin");
        }
        if let Some(link) = self.bin_links.first() {
            return link.source_path();
        }
        bail!(
            "wings MOCITO raw binary payload requires a non-directory bin entry or binLinks source"
        )
    }
}

impl MocitoBinLink {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.trim().is_empty(),
            "wings MOCITO binLinks entry has empty name"
        );
        safe_relative_path(&self.name, "binLinks.name")?;
        if let Some(src) = &self.src {
            safe_relative_path(src, "binLinks.src")?;
        }
        if let Some(link) = &self.link {
            safe_relative_path(link, "binLinks.link")?;
        }
        Ok(())
    }

    fn source_path(&self) -> Result<PathBuf> {
        safe_relative_path(self.src.as_deref().unwrap_or(&self.name), "binLinks.src")
    }

    fn link_path(&self) -> Result<PathBuf> {
        match self.link.as_deref() {
            Some(link) => safe_relative_path(link, "binLinks.link"),
            None => Ok(PathBuf::from(MOCITO_BIN_DIR).join(&self.name)),
        }
    }
}

fn filename_for_layer(
    manifest: &WingsManifest,
    layer: &WingsDescriptor,
    layer_index: usize,
) -> Result<String> {
    if layer_index == 0
        && let Some(filename) = manifest
            .annotations
            .get("dev.mise.mocito.source.url")
            .map(|url| get_filename_from_url(url))
            .filter(|name| !name.is_empty())
    {
        ensure_installable_source_filename(&filename)?;
        return Ok(filename);
    }

    let Some(expected) = layer.digest.strip_prefix("sha256:") else {
        bail!("wings layer digest is not sha256: {}", layer.digest);
    };
    ensure!(
        expected.len() == 64 && expected.chars().all(|c| c.is_ascii_hexdigit()),
        "wings layer digest must be a 64-character sha256 hex digest: {}",
        layer.digest
    );
    Ok(format!(
        "artifact-{layer_index}-{}.{}",
        &expected[..12],
        extension_for_layer_media_type(&layer.media_type)?
    ))
}

fn ensure_installable_source_filename(filename: &str) -> Result<()> {
    ensure!(
        filename != "." && filename != ".." && !filename.contains('/') && !filename.contains('\\'),
        "wings source artifact filename {filename:?} must not contain path separators or parent directory segments"
    );
    let format = TarFormat::from_file_name(filename);
    ensure!(
        format != TarFormat::Raw || !has_unsupported_archive_extension(filename),
        "wings source artifact filename {filename:?} has unsupported archive extension; expected tar, tar.gz, tar.xz, tar.bz2, tar.zst, zip, 7z, gz, xz, bz2, zst, or a raw binary"
    );
    Ok(())
}

fn has_unsupported_archive_extension(filename: &str) -> bool {
    let Some(ext) = Path::new(filename).extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "apk" | "deb" | "dmg" | "exe" | "msi" | "pkg" | "rpm"
    )
}

fn install_mocito_artifact(
    tv: &ToolVersion,
    config: &MocitoConfig,
    layers: &[(&WingsDescriptor, PathBuf)],
    pr: Option<&dyn crate::ui::progress_report::SingleReport>,
) -> Result<()> {
    let install_path = tv.install_path();
    file::remove_all(&install_path)?;
    file::create_dir_all(&install_path)?;

    for (layer, path) in layers {
        install_mocito_layer(tv, config, layer, path, pr)?;
    }
    validate_mocito_bins(&install_path, config)?;
    create_mocito_bin_links(&install_path, &config.bin_links)?;
    write_mocito_env_file(&install_path, &config.env)?;
    Ok(())
}

pub(crate) fn installed_env(tv: &ToolVersion) -> Result<BTreeMap<String, String>> {
    read_mocito_env_file(&tv.install_path())
}

fn write_mocito_env_file(install_path: &Path, env: &BTreeMap<String, String>) -> Result<()> {
    if env.is_empty() {
        return Ok(());
    }
    validate_mocito_env(env)?;
    let path = install_path.join(MOCITO_ENV_FILE);
    file::write(&path, serde_json::to_string_pretty(env)?)?;
    Ok(())
}

fn read_mocito_env_file(install_path: &Path) -> Result<BTreeMap<String, String>> {
    let path = install_path.join(MOCITO_ENV_FILE);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let env: BTreeMap<String, String> = serde_json::from_slice(&file::read(&path)?)
        .wrap_err_with(|| format!("decoding wings MOCITO env file {}", path.display()))?;
    validate_mocito_env(&env)?;
    Ok(env)
}

fn validate_mocito_env(env: &BTreeMap<String, String>) -> Result<()> {
    for (key, value) in env {
        ensure!(
            !key.trim().is_empty() && !key.contains('=') && !key.contains('\0'),
            "wings MOCITO config env key {key:?} is invalid"
        );
        ensure!(
            !value.contains('\0'),
            "wings MOCITO config env value for {key:?} contains a NUL byte"
        );
    }
    Ok(())
}

fn install_mocito_layer(
    tv: &ToolVersion,
    config: &MocitoConfig,
    layer: &WingsDescriptor,
    layer_path: &Path,
    pr: Option<&dyn crate::ui::progress_report::SingleReport>,
) -> Result<()> {
    let install_path = tv.install_path();
    match layer_format(&layer.media_type)? {
        TarFormat::Raw => {
            let dest = install_path.join(config.raw_binary_dest()?);
            if let Some(parent) = dest.parent() {
                file::create_dir_all(parent)?;
            }
            file::copy(layer_path, &dest)?;
            file::make_executable(&dest)?;
        }
        format => {
            let layer_dir = tv.download_path().join(format!(
                ".mocito-layer-{}",
                layer
                    .digest
                    .trim_start_matches("sha256:")
                    .chars()
                    .take(12)
                    .collect::<String>()
            ));
            file::remove_all(&layer_dir)?;
            file::create_dir_all(&layer_dir)?;
            let opts = file::TarOptions {
                strip_components: config.strip_components,
                pr,
                ..file::TarOptions::new(format)
            };
            file::untar(layer_path, &layer_dir, &opts)?;
            merge_mocito_layer(&layer_dir, &install_path)?;
            file::remove_all(&layer_dir)?;
        }
    }
    Ok(())
}

fn merge_mocito_layer(layer_dir: &Path, install_path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(layer_dir)? {
        let entry = entry?;
        let src = entry.path();
        let dst = install_path.join(entry.file_name());
        merge_mocito_entry(&src, &dst)?;
    }
    Ok(())
}

fn merge_mocito_entry(src: &Path, dst: &Path) -> Result<()> {
    let src_meta = std::fs::symlink_metadata(src)?;
    if src_meta.is_dir()
        && dst
            .symlink_metadata()
            .is_ok_and(|dst_meta| dst_meta.is_dir())
    {
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            merge_mocito_entry(&entry.path(), &dst.join(entry.file_name()))?;
        }
        std::fs::remove_dir(src).wrap_err_with(|| {
            format!(
                "failed to remove merged wings MOCITO layer directory {}",
                src.display()
            )
        })?;
        return Ok(());
    }

    if dst.exists() || dst.is_symlink() {
        file::remove_all(dst)?;
    } else if let Some(parent) = dst.parent() {
        file::create_dir_all(parent)?;
    }
    std::fs::rename(src, dst).wrap_err_with(|| {
        format!(
            "failed to move wings MOCITO layer entry {} to {}",
            src.display(),
            dst.display()
        )
    })?;
    Ok(())
}

fn validate_mocito_bins(install_path: &Path, config: &MocitoConfig) -> Result<()> {
    for entry in &config.bin {
        let is_dir_entry = entry.ends_with('/');
        let path = install_path.join(safe_relative_path(entry.trim_end_matches('/'), "bin")?);
        if is_dir_entry {
            ensure!(
                path.is_dir(),
                "wings MOCITO bin directory {} does not exist after install",
                path.display()
            );
        } else {
            ensure!(
                path.is_file(),
                "wings MOCITO bin file {} does not exist after install",
                path.display()
            );
            file::make_executable(&path)?;
        }
    }
    Ok(())
}

fn create_mocito_bin_links(install_path: &Path, links: &[MocitoBinLink]) -> Result<()> {
    if links.is_empty() {
        return Ok(());
    }
    let symlink_dir = install_path.join(MOCITO_BIN_DIR);
    file::create_dir_all(&symlink_dir)?;

    for link in links {
        let src = install_path.join(link.source_path()?);
        ensure!(
            src.is_file(),
            "wings MOCITO binLinks source {} does not exist after install",
            src.display()
        );
        file::make_executable(&src)?;
        let dst = install_path.join(link.link_path()?);
        if let Some(parent) = dst.parent() {
            file::create_dir_all(parent)?;
        }
        if link.hard {
            if dst.exists() || dst.is_symlink() {
                file::remove_file(&dst)?;
            }
            std::fs::hard_link(&src, &dst).wrap_err_with(|| {
                format!("failed to hard link {} to {}", src.display(), dst.display())
            })?;
        } else {
            file::make_symlink_or_copy(&src, &dst)?;
        }
    }
    Ok(())
}

fn layer_format(media_type: &str) -> Result<TarFormat> {
    match media_type {
        OCI_TAR_GZIP_LAYER_MEDIA_TYPE | DOCKER_TAR_GZIP_LAYER_MEDIA_TYPE => Ok(TarFormat::TarGz),
        OCI_TAR_LAYER_MEDIA_TYPE => Ok(TarFormat::Tar),
        OCI_TAR_ZSTD_LAYER_MEDIA_TYPE => Ok(TarFormat::TarZst),
        ZIP_LAYER_MEDIA_TYPE => Ok(TarFormat::Zip),
        BINARY_LAYER_MEDIA_TYPE => Ok(TarFormat::Raw),
        other => bail!("wings artifact layer media type {other} is not installable"),
    }
}

fn ensure_installable_layer_media_type(media_type: &str) -> Result<()> {
    layer_format(media_type).map(|_| ())
}

fn extension_for_layer_media_type(media_type: &str) -> Result<&'static str> {
    match layer_format(media_type)? {
        TarFormat::TarGz => Ok("tar.gz"),
        TarFormat::Tar => Ok("tar"),
        TarFormat::TarZst => Ok("tar.zst"),
        TarFormat::Zip => Ok("zip"),
        TarFormat::Raw => Ok("bin"),
        other => bail!("wings artifact layer media type for {other:?} is not supported"),
    }
}

fn safe_relative_path(value: &str, field: &str) -> Result<PathBuf> {
    ensure!(
        !value.trim().is_empty(),
        "wings MOCITO config {field} path is empty"
    );
    let path = Path::new(value);
    ensure!(
        !path.is_absolute(),
        "wings MOCITO config {field} path {value:?} must be relative"
    );
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => bail!(
                "wings MOCITO config {field} path {value:?} must not escape the install directory"
            ),
        }
    }
    ensure!(
        !normalized.as_os_str().is_empty(),
        "wings MOCITO config {field} path is empty"
    );
    Ok(normalized)
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
    fn fallback_required_auth_error_is_actionable() {
        let tv = tool_version("node", Some("github:nodejs/node"));
        let err = fallback_or_false(
            &tv,
            true,
            "wings authentication is not available; run `mise wings login` or configure GitHub Actions OIDC",
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("mise wings login"));
        assert!(message.contains("GitHub Actions OIDC"));
        assert!(message.contains("wings.required blocks fallback"));
    }

    #[test]
    fn source_layer_rejects_unknown_archive_extension() {
        let mut annotations = indexmap::IndexMap::new();
        annotations.insert(
            "dev.mise.mocito.source.url".into(),
            "https://example.com/tool.deb".into(),
        );
        let manifest = manifest_with_annotations(annotations);
        let layer = layer(OCI_TAR_GZIP_LAYER_MEDIA_TYPE);

        let err = filename_for_layer(&manifest, &layer, 0).unwrap_err();
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
            "dev.mise.mocito.source.url".into(),
            "https://example.com/tool".into(),
        );
        let manifest = manifest_with_annotations(annotations);
        let layer = layer(BINARY_LAYER_MEDIA_TYPE);

        assert_eq!(filename_for_layer(&manifest, &layer, 0).unwrap(), "tool");
    }

    #[test]
    fn source_layer_allows_dotted_raw_binary_filename() {
        let mut annotations = indexmap::IndexMap::new();
        annotations.insert(
            "dev.mise.mocito.source.url".into(),
            "https://example.com/gh_2.40.0_linux_amd64".into(),
        );
        let manifest = manifest_with_annotations(annotations);
        let layer = layer(BINARY_LAYER_MEDIA_TYPE);

        assert_eq!(
            filename_for_layer(&manifest, &layer, 0).unwrap(),
            "gh_2.40.0_linux_amd64"
        );
    }

    #[test]
    fn layer_without_source_url_gets_stable_filename() {
        let manifest = manifest_with_annotations(indexmap::IndexMap::new());
        let layer = layer(OCI_TAR_GZIP_LAYER_MEDIA_TYPE);

        assert_eq!(
            filename_for_layer(&manifest, &layer, 1).unwrap(),
            "artifact-1-abcdef012345.tar.gz"
        );
    }

    #[test]
    fn layer_without_source_url_rejects_short_digest_without_panic() {
        let manifest = manifest_with_annotations(indexmap::IndexMap::new());
        let mut layer = layer(OCI_TAR_GZIP_LAYER_MEDIA_TYPE);
        layer.digest = "sha256:abc".into();

        let err = filename_for_layer(&manifest, &layer, 1).unwrap_err();
        assert!(err.to_string().contains("64-character sha256"));
    }

    #[test]
    fn mocito_config_rejects_unsafe_paths() {
        let mut config = mocito_config();
        config.bin = vec!["../bin/tool".into()];
        let err = config
            .validate(&tool_version("foo", Some("github:foo/bar")))
            .unwrap_err();

        assert!(err.to_string().contains("must not escape"));
    }

    #[test]
    fn mocito_env_round_trips_from_install_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = BTreeMap::new();
        env.insert("CC".into(), "clang".into());
        env.insert("MISE_ADD_PATH".into(), "libexec/bin".into());

        write_mocito_env_file(dir.path(), &env).unwrap();

        assert_eq!(read_mocito_env_file(dir.path()).unwrap(), env);
    }

    #[test]
    fn mocito_env_file_revalidates_on_read() {
        let dir = tempfile::tempdir().unwrap();
        file::write(dir.path().join(MOCITO_ENV_FILE), r#"{"BAD=KEY":"value"}"#).unwrap();

        let err = read_mocito_env_file(dir.path()).unwrap_err();

        assert!(err.to_string().contains("env key"));
    }

    #[test]
    fn mocito_layer_merge_preserves_sibling_directory_contents() {
        let dir = tempfile::tempdir().unwrap();
        let install_path = dir.path().join("install");
        let layer0 = dir.path().join("layer0");
        let layer1 = dir.path().join("layer1");
        file::create_dir_all(layer0.join("bin")).unwrap();
        file::create_dir_all(layer1.join("bin")).unwrap();
        file::write(layer0.join("bin").join("alpha"), "alpha").unwrap();
        file::write(layer0.join("bin").join("shared"), "old").unwrap();
        file::write(layer1.join("bin").join("beta"), "beta").unwrap();
        file::write(layer1.join("bin").join("shared"), "new").unwrap();

        merge_mocito_layer(&layer0, &install_path).unwrap();
        merge_mocito_layer(&layer1, &install_path).unwrap();

        assert_eq!(
            file::read_to_string(install_path.join("bin").join("alpha")).unwrap(),
            "alpha"
        );
        assert_eq!(
            file::read_to_string(install_path.join("bin").join("beta")).unwrap(),
            "beta"
        );
        assert_eq!(
            file::read_to_string(install_path.join("bin").join("shared")).unwrap(),
            "new"
        );
    }

    fn manifest_with_annotations(annotations: indexmap::IndexMap<String, String>) -> WingsManifest {
        WingsManifest {
            artifact_type: MOCITO_TOOL_ARTIFACT_TYPE.into(),
            config: WingsDescriptor {
                media_type: MOCITO_TOOL_CONFIG_MEDIA_TYPE.into(),
                digest: "sha256:config".into(),
                size: 1,
            },
            layers: vec![],
            annotations,
        }
    }

    fn layer(media_type: &str) -> WingsDescriptor {
        WingsDescriptor {
            media_type: media_type.into(),
            digest: "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
                .into(),
            size: 1,
        }
    }

    fn mocito_config() -> MocitoConfig {
        MocitoConfig {
            schema_version: 1,
            namespace: "github".into(),
            tool: "foo/bar".into(),
            version: "1.0.0".into(),
            platform: PlatformTarget::from_current().to_key(),
            strip_components: 0,
            bin: vec![],
            bin_links: vec![],
            env: BTreeMap::new(),
        }
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
