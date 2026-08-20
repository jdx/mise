use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::{Result, bail, eyre};
use futures_util::TryStreamExt as _;
use log::warn;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderMap, HeaderValue, IF_MATCH,
    IF_NONE_MATCH,
};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use url::{Host, Url};

mod agent;
mod local;

pub use agent::{
    AGENT_PROTOCOL_VERSION, ActionPrediction, AgentRemoteCache, AgentRequest, AgentResponse,
    AgentStats, CacheAgent, RestoreStats,
};
pub use local::{LocalActionCache, LocalCas};

pub const PROTOCOL_VERSION: u8 = 1;
const PROTOCOL_HEADER: &str = "mise-cache-protocol";
const NAMESPACE_HEADER: &str = "mise-cache-namespace";
pub const ACTION_RESULT_MEDIA_TYPE: &str = "application/vnd.mise.cache-action-result.v1+json";
pub const DIRECTORY_MEDIA_TYPE: &str = "application/vnd.mise.cache-directory.v1+json";
pub const CLIENT_METADATA_MEDIA_TYPE: &str = "application/vnd.mise.cache-client-metadata.v1+json";
pub const TASK_ACTION_MANIFEST_MEDIA_TYPE: &str =
    "application/vnd.mise.cache-task-action-manifest.v1+json";
pub const BLOB_MEDIA_TYPE: &str = "application/octet-stream";
pub const BLOB_PACK_MEDIA_TYPE: &str = "application/vnd.mise.cache-blob-pack.v1";
const DIGEST_LIST_MEDIA_TYPE: &str = "application/vnd.mise.cache-digests.v1+json";
const BLOB_PACK_BLOBS_HEADER: &str = "mise-cache-pack-blobs";
const BLOB_PACK_BYTES_HEADER: &str = "mise-cache-pack-bytes";
const BLOB_PACK_MAGIC: &[u8; 8] = b"MISEPK01";
const BLOB_PACK_HEADER_BYTES: u64 = 1 + 32 + 8;
const MAX_STAGED_BLOB_PACK_BYTES: u64 = 256 * 1024 * 1024;
const MAX_STAGED_BLOB_PACK_ITEMS: usize = 2 * 1024;
const BLOB_PACK_TIMEOUT_BYTES_PER_UNIT: u64 = MAX_STAGED_BLOB_PACK_BYTES / 4;
const BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT: usize = MAX_STAGED_BLOB_PACK_ITEMS / 4;

/// Serialize a protocol object using the JSON Canonicalization Scheme.
///
/// Action digests are computed from these bytes, so callers must not use
/// serde's struct field order as part of the wire contract.
pub fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(serde_json_canonicalizer::to_vec(value)?)
}

#[derive(
    Debug,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    strum::EnumString,
    strum::Display,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum RemoteCacheMode {
    #[default]
    ReadWrite,
    ReadOnly,
    WriteOnly,
}

impl RemoteCacheMode {
    pub fn reads(self) -> bool {
        matches!(self, Self::ReadWrite | Self::ReadOnly)
    }

    pub fn writes(self) -> bool {
        matches!(self, Self::ReadWrite | Self::WriteOnly)
    }
}

pub struct RemoteCacheConfig {
    pub base_url: Url,
    pub namespace: String,
    pub token: Option<String>,
    pub token_file: Option<PathBuf>,
    pub oidc_audience: Option<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub download_timeout: Duration,
    pub retries: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheDigest {
    pub algorithm: String,
    pub hash: String,
    pub size: u64,
}

impl CacheDigest {
    pub fn blake3(bytes: &[u8]) -> Self {
        Self {
            algorithm: "blake3".into(),
            hash: blake3::hash(bytes).to_hex().to_string(),
            size: bytes.len() as u64,
        }
    }

    /// Hash a file while counting the bytes read in the same streaming pass.
    pub fn blake3_file(path: &Path) -> Result<Self> {
        let (hash, size) = hash_file_blake3(path)?;
        Ok(Self {
            algorithm: "blake3".into(),
            hash,
            size,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.algorithm != "blake3" && self.algorithm != "sha256" {
            bail!("unsupported remote cache digest algorithm");
        }
        if self.hash.len() != 64
            || !self
                .hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("invalid remote cache digest");
        }
        Ok(())
    }

    pub fn matches_bytes(&self, bytes: &[u8]) -> Result<bool> {
        self.validate()?;
        if self.size != bytes.len() as u64 {
            return Ok(false);
        }
        let hash = match self.algorithm.as_str() {
            "blake3" => blake3::hash(bytes).to_hex().to_string(),
            "sha256" => hex::encode(sha2::Sha256::digest(bytes)),
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.hash == hash)
    }

    pub fn matches_file(&self, path: &Path) -> Result<bool> {
        self.validate()?;
        let (hash, size) = match self.algorithm.as_str() {
            "blake3" => hash_file_blake3(path)?,
            "sha256" => hash_file_sha256(path)?,
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.size == size && self.hash == hash)
    }
}

fn hash_file_blake3(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((hasher.finalize().to_hex().to_string(), size))
}

fn hash_file_sha256(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((hex::encode(hasher.finalize()), size))
}

/// A canonical action-result record referencing objects in the CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteActionResult {
    pub action: CacheDigest,
    #[serde(default)]
    pub metadata: Option<CacheDigest>,
    #[serde(default)]
    pub output_root: Option<CacheDigest>,
    pub version: u8,
}

/// A canonical directory object stored in the CAS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDirectory {
    pub directories: Vec<CacheDirectoryNode>,
    pub files: Vec<CacheFileNode>,
    pub symlinks: Vec<CacheSymlinkNode>,
    pub version: u8,
}

/// A child directory entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheDirectoryNode {
    pub digest: CacheDigest,
    pub mode: u32,
    pub name: String,
}

/// A file entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheFileNode {
    pub digest: CacheDigest,
    pub executable: bool,
    pub mode: u32,
    pub name: String,
}

/// A symbolic-link entry in a canonical cache directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheSymlinkNode {
    pub mode: u32,
    pub name: String,
    pub target: String,
}

/// Rust-specific action metadata stored alongside compiled outputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustcMetadata {
    pub version: u8,
    pub kind: String,
    pub stdout: CacheDigest,
    pub stderr: CacheDigest,
}

pub enum BlobSource {
    Bytes(Vec<u8>),
    File(tempfile::NamedTempFile),
    Path(PathBuf),
}

pub struct BlobUpload {
    pub digest: CacheDigest,
    pub source: BlobSource,
}

pub struct RemoteActionManifest {
    pub bytes: Vec<u8>,
    pub etag: String,
}

/// A verified set of remote CAS objects downloaded through blob-pack streams.
pub struct RemoteBlobPack {
    _directory: tempfile::TempDir,
    pub blobs: Vec<(CacheDigest, PathBuf)>,
    pub requests: u64,
    pub requested: Vec<CacheDigest>,
    pub blob_count: u64,
    pub payload_bytes: u64,
    pub framed_bytes: u64,
}

struct DownloadedBlobPack {
    directory: tempfile::TempDir,
    blobs: Vec<(CacheDigest, PathBuf)>,
    metadata: BlobPackResponseStats,
}

#[derive(Debug, Clone, Copy, Default)]
struct BlobPackResponseMetadata {
    content_length: Option<u64>,
    blob_count: Option<u64>,
    payload_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct BlobPackResponseStats {
    blob_count: u64,
    payload_bytes: u64,
    framed_bytes: u64,
}

impl BlobPackResponseMetadata {
    fn from_headers(headers: &HeaderMap) -> Result<Self> {
        Ok(Self {
            content_length: optional_u64_header(headers, CONTENT_LENGTH.as_str())?,
            blob_count: optional_u64_header(headers, BLOB_PACK_BLOBS_HEADER)?,
            payload_bytes: optional_u64_header(headers, BLOB_PACK_BYTES_HEADER)?,
        })
    }

    fn validate(self, decoded: BlobPackResponseStats) -> Result<BlobPackResponseStats> {
        if let Some(content_length) = self.content_length
            && content_length != decoded.framed_bytes
        {
            bail!(
                "remote cache blob pack content length metadata mismatch: expected {}, decoded {}",
                content_length,
                decoded.framed_bytes
            );
        }
        if let Some(blob_count) = self.blob_count
            && blob_count != decoded.blob_count
        {
            bail!(
                "remote cache blob pack blob count metadata mismatch: expected {}, decoded {}",
                blob_count,
                decoded.blob_count
            );
        }
        if let Some(payload_bytes) = self.payload_bytes
            && payload_bytes != decoded.payload_bytes
        {
            bail!(
                "remote cache blob pack payload byte metadata mismatch: expected {}, decoded {}",
                payload_bytes,
                decoded.payload_bytes
            );
        }
        Ok(BlobPackResponseStats {
            blob_count: self.blob_count.unwrap_or(decoded.blob_count),
            payload_bytes: self.payload_bytes.unwrap_or(decoded.payload_bytes),
            framed_bytes: self.content_length.unwrap_or(decoded.framed_bytes),
        })
    }
}

fn optional_u64_header(headers: &HeaderMap, name: &str) -> Result<Option<u64>> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| eyre!("remote cache blob pack {name} header is not valid UTF-8"))?;
    let value = value
        .parse::<u64>()
        .map_err(|_| eyre!("remote cache blob pack {name} header is not an unsigned integer"))?;
    Ok(Some(value))
}

#[derive(Debug, Deserialize)]
struct RemoteCacheCapabilities {
    protocol: CapabilityProtocol,
    #[serde(default)]
    features: CapabilityFeatures,
    #[serde(default)]
    limits: CapabilityLimits,
}

#[derive(Debug, Deserialize)]
struct CapabilityProtocol {
    major: u8,
}

#[derive(Debug, Default, Deserialize)]
struct CapabilityFeatures {
    #[serde(default)]
    blob_packs: bool,
}

#[derive(Debug, Default, Deserialize)]
struct CapabilityLimits {
    #[serde(default)]
    max_batch_items: u64,
    #[serde(default)]
    max_pack_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
struct BlobPackLimits {
    max_items: usize,
    max_bytes: u64,
}

#[derive(Serialize)]
struct DigestList<'a> {
    digests: &'a [CacheDigest],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestPutOutcome {
    Stored,
    PreconditionFailed,
}

pub struct RemoteCacheClient {
    base_url: Url,
    namespace: String,
    client: reqwest::Client,
    credential: RemoteCacheCredential,
    download_timeout: Duration,
    retries: i64,
    capabilities: tokio::sync::OnceCell<Option<BlobPackLimits>>,
    blob_packs_disabled: AtomicBool,
}

impl RemoteCacheClient {
    pub fn new(config: RemoteCacheConfig) -> Result<Self> {
        let authenticated = config
            .token
            .as_deref()
            .is_some_and(|token| !token.trim().is_empty())
            || config.token_file.is_some()
            || config
                .oidc_audience
                .as_deref()
                .is_some_and(|audience| !audience.trim().is_empty());
        validate_remote_url(&config.base_url, authenticated)?;
        let client = reqwest::Client::builder()
            .connect_timeout(config.connect_timeout)
            .read_timeout(config.read_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let credential = remote_credential(&config, client.clone())?;
        Ok(Self {
            base_url: normalized_base_url(config.base_url),
            namespace: config.namespace,
            client,
            credential,
            download_timeout: config.download_timeout,
            retries: config.retries,
            capabilities: tokio::sync::OnceCell::new(),
            blob_packs_disabled: AtomicBool::new(false),
        })
    }

    fn action_result_endpoint(&self, action: &CacheDigest) -> Result<Url> {
        action.validate()?;
        if action.algorithm != "blake3" {
            bail!("remote cache action keys must use blake3");
        }
        Ok(self.base_url.join(&format!(
            "v{PROTOCOL_VERSION}/action-results/{}/{}/{}",
            action.algorithm, action.hash, action.size
        ))?)
    }

    fn blob_endpoint(&self, digest: &CacheDigest) -> Result<Url> {
        digest.validate()?;
        Ok(self.base_url.join(&format!(
            "v{PROTOCOL_VERSION}/blobs/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        ))?)
    }

    fn action_manifest_endpoint(&self, key: &CacheDigest) -> Result<Url> {
        key.validate()?;
        if key.algorithm != "blake3" {
            bail!("remote action manifest keys must use blake3");
        }
        Ok(self.base_url.join(&format!(
            "v{PROTOCOL_VERSION}/action-manifests/{}/{}/{}",
            key.algorithm, key.hash, key.size
        ))?)
    }

    fn capabilities_endpoint(&self) -> Result<Url> {
        Ok(self
            .base_url
            .join(&format!("v{PROTOCOL_VERSION}/capabilities"))?)
    }

    fn blob_pack_endpoint(&self) -> Result<Url> {
        Ok(self
            .base_url
            .join(&format!("v{PROTOCOL_VERSION}/blobs:pack"))?)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        media_type: &'static str,
    ) -> Result<reqwest::RequestBuilder> {
        let request = self
            .client
            .request(method, url)
            .header(PROTOCOL_HEADER, u16::from(PROTOCOL_VERSION))
            .header(NAMESPACE_HEADER, &self.namespace)
            .header(ACCEPT, media_type);
        if let Some(authorization) = self.credential.authorization().await? {
            Ok(request.header(AUTHORIZATION, authorization))
        } else {
            Ok(request)
        }
    }

    async fn blob_pack_limits(&self) -> Result<Option<BlobPackLimits>> {
        self.capabilities
            .get_or_try_init(|| async {
                let url = self.capabilities_endpoint()?;
                let response = self
                    .request(reqwest::Method::GET, url, "application/json")
                    .await?
                    .send()
                    .await?;
                if matches!(
                    response.status(),
                    StatusCode::NOT_FOUND
                        | StatusCode::METHOD_NOT_ALLOWED
                        | StatusCode::NOT_IMPLEMENTED
                ) {
                    return Ok(None);
                }
                let capabilities: RemoteCacheCapabilities =
                    response.error_for_status()?.json().await?;
                if capabilities.protocol.major != PROTOCOL_VERSION {
                    bail!(
                        "remote cache capability protocol {} is incompatible with client protocol {PROTOCOL_VERSION}",
                        capabilities.protocol.major
                    );
                }
                if !capabilities.features.blob_packs {
                    return Ok(None);
                }
                let max_items = usize::try_from(capabilities.limits.max_batch_items)
                    .ok()
                    .filter(|limit| *limit > 0)
                    .ok_or_else(|| {
                        eyre!("remote cache blob packs require a positive max_batch_items limit")
                    })?;
                if capabilities.limits.max_pack_bytes == 0 {
                    bail!("remote cache blob packs require a positive max_pack_bytes limit");
                }
                Ok(Some(BlobPackLimits {
                    max_items: max_items.min(MAX_STAGED_BLOB_PACK_ITEMS),
                    max_bytes: capabilities
                        .limits
                        .max_pack_bytes
                        .min(MAX_STAGED_BLOB_PACK_BYTES),
                }))
            })
            .await
            .copied()
    }

    /// Download verified CAS objects using the server's negotiated blob-pack extension.
    ///
    /// `None` means the server does not support blob packs. Objects omitted by a
    /// supported server are absent from `blobs`, so callers can retry them through
    /// the ordinary single-blob endpoint.
    pub async fn get_blob_pack(
        &self,
        digests: &[CacheDigest],
        staging_dir: &Path,
    ) -> Result<Option<RemoteBlobPack>> {
        if digests.is_empty() || self.blob_packs_disabled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Some(limits) = self.blob_pack_limits().await? else {
            return Ok(None);
        };
        fs::create_dir_all(staging_dir)?;
        let chunk = blob_pack_chunk(digests, limits)?;
        if chunk.is_empty() {
            return Ok(Some(RemoteBlobPack {
                _directory: tempfile::tempdir_in(staging_dir)?,
                blobs: Vec::new(),
                requests: 0,
                requested: Vec::new(),
                blob_count: 0,
                payload_bytes: 0,
                framed_bytes: BLOB_PACK_MAGIC.len() as u64,
            }));
        }
        match self.download_blob_pack_chunk(&chunk, staging_dir).await? {
            Some(pack) => Ok(Some(RemoteBlobPack {
                _directory: pack.directory,
                blobs: pack.blobs,
                requests: 1,
                requested: chunk,
                blob_count: pack.metadata.blob_count,
                payload_bytes: pack.metadata.payload_bytes,
                framed_bytes: pack.metadata.framed_bytes,
            })),
            None => {
                self.blob_packs_disabled.store(true, Ordering::Relaxed);
                Ok(None)
            }
        }
    }

    async fn download_blob_pack_chunk(
        &self,
        digests: &[CacheDigest],
        staging_dir: &Path,
    ) -> Result<Option<DownloadedBlobPack>> {
        let url = self.blob_pack_endpoint()?;
        let body = serde_json::to_vec(&DigestList { digests })?;
        let download_timeout = blob_pack_download_timeout(self.download_timeout, digests);
        let download = retry_async("POST", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::POST, url.clone(), BLOB_PACK_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, DIGEST_LIST_MEDIA_TYPE)
                .body(body.clone())
                .send()
                .await?;
            if matches!(
                response.status(),
                StatusCode::NOT_FOUND
                    | StatusCode::METHOD_NOT_ALLOWED
                    | StatusCode::NOT_IMPLEMENTED
            ) {
                return Ok(None);
            }
            let response = response.error_for_status()?;
            let media_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .map(str::trim);
            if media_type != Some(BLOB_PACK_MEDIA_TYPE) {
                bail!("remote cache blob pack has an invalid content type");
            }
            Ok(Some(
                decode_blob_pack(response, digests, staging_dir).await?,
            ))
        });
        tokio::time::timeout(download_timeout, download)
            .await
            .map_err(|_| eyre!("remote cache blob pack download timed out for {url}"))?
    }

    pub async fn get_action_result(
        &self,
        action: &CacheDigest,
    ) -> Result<Option<RemoteActionResult>> {
        let url = self.action_result_endpoint(action)?;
        let result = retry_async("GET", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::GET, url.clone(), ACTION_RESULT_MEDIA_TYPE)
                .await?
                .send()
                .await?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            Ok(Some(
                response
                    .error_for_status()?
                    .json::<RemoteActionResult>()
                    .await?,
            ))
        })
        .await?;
        if let Some(result) = &result
            && (result.version != 1 || result.action != *action)
        {
            bail!("remote action result does not match requested action");
        }
        Ok(result)
    }

    pub async fn put_action_result(&self, result: &RemoteActionResult) -> Result<()> {
        let url = self.action_result_endpoint(&result.action)?;
        let body = serde_json::to_vec(result)?;
        retry_async("PUT", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::PUT, url.clone(), ACTION_RESULT_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, ACTION_RESULT_MEDIA_TYPE)
                .header(IF_NONE_MATCH, "*")
                .body(body.clone())
                .send()
                .await?;
            if response.status() != StatusCode::PRECONDITION_FAILED {
                response.error_for_status()?;
            }
            Ok(())
        })
        .await
    }

    pub async fn get_action_manifest(
        &self,
        key: &CacheDigest,
    ) -> Result<Option<RemoteActionManifest>> {
        let url = self.action_manifest_endpoint(key)?;
        retry_async("GET", &url, self.retries, || async {
            let response = self
                .request(
                    reqwest::Method::GET,
                    url.clone(),
                    TASK_ACTION_MANIFEST_MEDIA_TYPE,
                )
                .await?
                .send()
                .await?;
            if response.status() == StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let response = response.error_for_status()?;
            let etag = parse_strong_etag(response.headers().get(ETAG))?;
            let bytes = response.bytes().await?.to_vec();
            if blake3::hash(&bytes).to_hex().as_str() != etag {
                bail!("remote action manifest ETag does not match its body");
            }
            Ok(Some(RemoteActionManifest { bytes, etag }))
        })
        .await
    }

    pub async fn put_action_manifest(
        &self,
        key: &CacheDigest,
        bytes: &[u8],
        expected_etag: Option<&str>,
    ) -> Result<ManifestPutOutcome> {
        let url = self.action_manifest_endpoint(key)?;
        let body = bytes.to_vec();
        let expected_etag = expected_etag.map(quoted_etag).transpose()?;
        retry_async("PUT", &url, self.retries, || async {
            let mut request = self
                .request(
                    reqwest::Method::PUT,
                    url.clone(),
                    TASK_ACTION_MANIFEST_MEDIA_TYPE,
                )
                .await?
                .header(CONTENT_TYPE, TASK_ACTION_MANIFEST_MEDIA_TYPE)
                .body(body.clone());
            request = if let Some(etag) = &expected_etag {
                request.header(IF_MATCH, etag)
            } else {
                request.header(IF_NONE_MATCH, "*")
            };
            let response = request.send().await?;
            if response.status() == StatusCode::PRECONDITION_FAILED {
                return Ok(ManifestPutOutcome::PreconditionFailed);
            }
            response.error_for_status()?;
            Ok(ManifestPutOutcome::Stored)
        })
        .await
    }

    pub async fn get_blob(
        &self,
        digest: &CacheDigest,
        media_type: &'static str,
    ) -> Result<Vec<u8>> {
        digest.validate()?;
        let url = self.blob_endpoint(digest)?;
        retry_async("GET", &url, self.retries, || async {
            let response = self
                .request(reqwest::Method::GET, url.clone(), media_type)
                .await?
                .send()
                .await?
                .error_for_status()?;
            let bytes = response.bytes().await?.to_vec();
            if !digest.matches_bytes(&bytes)? {
                bail!("remote cache blob failed digest verification");
            }
            Ok(bytes)
        })
        .await
    }

    pub async fn get_blob_file(
        &self,
        digest: &CacheDigest,
        staging_dir: &Path,
    ) -> Result<tempfile::NamedTempFile> {
        let url = self.blob_endpoint(digest)?;
        let download = retry_async("GET", &url, self.retries, || async {
            let mut response = self
                .request(reqwest::Method::GET, url.clone(), BLOB_MEDIA_TYPE)
                .await?
                .send()
                .await?;
            response.error_for_status_ref()?;
            fs::create_dir_all(staging_dir)?;
            let temporary = tempfile::NamedTempFile::new_in(staging_dir)?;
            let mut output = tokio::fs::File::from_std(temporary.reopen()?);
            while let Some(chunk) = response.chunk().await? {
                output.write_all(&chunk).await?;
            }
            output.flush().await?;
            drop(output);
            if !digest.matches_file(temporary.path())? {
                bail!("remote cache blob failed digest verification");
            }
            Ok(temporary)
        });
        tokio::time::timeout(self.download_timeout, download)
            .await
            .map_err(|_| eyre!("remote cache blob download timed out for {url}"))?
    }

    pub async fn put_blob(&self, upload: &BlobUpload) -> Result<()> {
        let url = self.blob_endpoint(&upload.digest)?;
        retry_async("PUT", &url, self.retries, || async {
            let (length, body) = match &upload.source {
                BlobSource::Bytes(bytes) => {
                    (bytes.len() as u64, reqwest::Body::from(bytes.clone()))
                }
                BlobSource::File(file) => {
                    let file = tokio::fs::File::open(file.path()).await?;
                    let length = file.metadata().await?.len();
                    let stream = tokio_util::io::ReaderStream::new(file);
                    (length, reqwest::Body::wrap_stream(stream))
                }
                BlobSource::Path(path) => {
                    let file = tokio::fs::File::open(path).await?;
                    let length = file.metadata().await?.len();
                    let stream = tokio_util::io::ReaderStream::new(file);
                    (length, reqwest::Body::wrap_stream(stream))
                }
            };
            let response = self
                .request(reqwest::Method::PUT, url.clone(), BLOB_MEDIA_TYPE)
                .await?
                .header(CONTENT_TYPE, BLOB_MEDIA_TYPE)
                .header(CONTENT_LENGTH, length)
                .header(IF_NONE_MATCH, "*")
                .body(body)
                .send()
                .await?;
            if response.status() != StatusCode::PRECONDITION_FAILED {
                response.error_for_status()?;
            }
            Ok(())
        })
        .await
    }
}

fn blob_pack_chunk(digests: &[CacheDigest], limits: BlobPackLimits) -> Result<Vec<CacheDigest>> {
    let mut seen = BTreeSet::new();
    let mut chunk = Vec::new();
    let mut chunk_bytes = 0_u64;
    for digest in digests {
        digest.validate()?;
        if !seen.insert(digest.clone()) || digest.size > limits.max_bytes {
            continue;
        }
        if chunk.len() == limits.max_items
            || chunk_bytes.saturating_add(digest.size) > limits.max_bytes
        {
            break;
        }
        chunk_bytes = chunk_bytes.saturating_add(digest.size);
        chunk.push(digest.clone());
    }
    Ok(chunk)
}

fn blob_pack_download_timeout(base: Duration, digests: &[CacheDigest]) -> Duration {
    let bytes = digests
        .iter()
        .fold(0_u64, |total, digest| total.saturating_add(digest.size));
    let byte_units = bytes.div_ceil(BLOB_PACK_TIMEOUT_BYTES_PER_UNIT);
    let item_units = digests.len().div_ceil(BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT);
    let item_units = u64::try_from(item_units).unwrap_or(u64::MAX);
    let multiplier = byte_units.max(item_units).max(1);
    base.saturating_mul(u32::try_from(multiplier).unwrap_or(u32::MAX))
}

async fn decode_blob_pack(
    response: reqwest::Response,
    requested: &[CacheDigest],
    staging_dir: &Path,
) -> Result<DownloadedBlobPack> {
    let metadata = BlobPackResponseMetadata::from_headers(response.headers())?;
    let requested = requested.iter().cloned().collect::<BTreeSet<_>>();
    let stream = response.bytes_stream().map_err(std::io::Error::other);
    let mut reader = tokio_util::io::StreamReader::new(stream);
    let mut magic = [0_u8; BLOB_PACK_MAGIC.len()];
    reader.read_exact(&mut magic).await?;
    if &magic != BLOB_PACK_MAGIC {
        bail!("remote cache blob pack has invalid magic");
    }

    let directory = tempfile::tempdir_in(staging_dir)?;
    let mut seen = BTreeSet::new();
    let mut blobs = Vec::new();
    let mut payload_bytes = 0_u64;
    let mut framed_bytes = BLOB_PACK_MAGIC.len() as u64;
    loop {
        let mut algorithm = [0_u8; 1];
        if reader.read(&mut algorithm).await? == 0 {
            break;
        }
        let (algorithm, mut hasher) = match algorithm[0] {
            1 => (
                "blake3",
                BlobPackHasher::Blake3(Box::new(blake3::Hasher::new())),
            ),
            2 => ("sha256", BlobPackHasher::Sha256(sha2::Sha256::new())),
            _ => bail!("remote cache blob pack has an invalid digest algorithm"),
        };
        let mut hash = [0_u8; 32];
        reader.read_exact(&mut hash).await?;
        let mut size = [0_u8; 8];
        reader.read_exact(&mut size).await?;
        let digest = CacheDigest {
            algorithm: algorithm.into(),
            hash: hex::encode(hash),
            size: u64::from_be_bytes(size),
        };
        if !requested.contains(&digest) {
            bail!("remote cache blob pack returned an unrequested digest");
        }
        if !seen.insert(digest.clone()) {
            bail!("remote cache blob pack returned a duplicate digest");
        }
        framed_bytes = framed_bytes
            .checked_add(BLOB_PACK_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(digest.size))
            .ok_or_else(|| eyre!("remote cache blob pack is too large"))?;
        payload_bytes = payload_bytes
            .checked_add(digest.size)
            .ok_or_else(|| eyre!("remote cache blob pack payload is too large"))?;

        let path = directory.path().join(blobs.len().to_string());
        let mut output = tokio::fs::File::create(&path).await?;
        let mut remaining = digest.size;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let limit = usize::try_from(remaining.min(buffer.len() as u64)).unwrap();
            let count = reader.read(&mut buffer[..limit]).await?;
            if count == 0 {
                bail!("remote cache blob pack ended before a blob was complete");
            }
            output.write_all(&buffer[..count]).await?;
            hasher.update(&buffer[..count]);
            remaining -= count as u64;
        }
        output.flush().await?;
        drop(output);
        if !hasher.matches(&digest.hash) {
            bail!("remote cache blob pack failed digest verification");
        }
        blobs.push((digest, path));
    }
    let blob_count = blobs.len().try_into().unwrap_or(u64::MAX);
    let metadata = metadata.validate(BlobPackResponseStats {
        blob_count,
        payload_bytes,
        framed_bytes,
    })?;
    Ok(DownloadedBlobPack {
        directory,
        blobs,
        metadata,
    })
}

enum BlobPackHasher {
    Blake3(Box<blake3::Hasher>),
    Sha256(sha2::Sha256),
}

impl BlobPackHasher {
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Blake3(hasher) => {
                hasher.update(bytes);
            }
            Self::Sha256(hasher) => {
                hasher.update(bytes);
            }
        }
    }

    fn matches(self, expected: &str) -> bool {
        match self {
            Self::Blake3(hasher) => hasher.finalize().to_hex().as_str() == expected,
            Self::Sha256(hasher) => hex::encode(hasher.finalize()) == expected,
        }
    }
}

fn parse_strong_etag(value: Option<&HeaderValue>) -> Result<String> {
    let value = value
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| eyre!("remote action manifest response is missing an ETag"))?;
    let etag = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| is_lower_hex_digest(value))
        .ok_or_else(|| eyre!("remote action manifest response has an invalid ETag"))?;
    Ok(etag.to_owned())
}

fn quoted_etag(etag: &str) -> Result<HeaderValue> {
    if !is_lower_hex_digest(etag) {
        bail!("invalid remote action manifest ETag");
    }
    Ok(HeaderValue::from_str(&format!("\"{etag}\""))?)
}

fn is_lower_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone)]
enum RemoteCacheCredential {
    None,
    Static(HeaderValue),
    File(PathBuf),
    GithubActions(Arc<GithubActionsOidcCredential>),
}

struct GithubActionsOidcCredential {
    audience: String,
    request_url: Url,
    request_token: HeaderValue,
    client: reqwest::Client,
    retries: i64,
    cached: tokio::sync::Mutex<Option<CachedOidcToken>>,
}

struct CachedOidcToken {
    authorization: HeaderValue,
    expires_at: u64,
}

#[derive(Deserialize)]
struct GithubActionsOidcResponse {
    value: String,
}

#[derive(Deserialize)]
struct JwtExpiry {
    exp: u64,
}

fn remote_credential(
    config: &RemoteCacheConfig,
    client: reqwest::Client,
) -> Result<RemoteCacheCredential> {
    if let Some(authorization) = authorization_header(config.token.as_deref())? {
        return Ok(RemoteCacheCredential::Static(authorization));
    }
    if let Some(path) = &config.token_file {
        return Ok(RemoteCacheCredential::File(path.clone()));
    }
    let Some(audience) = config
        .oidc_audience
        .as_deref()
        .map(str::trim)
        .filter(|audience| !audience.is_empty())
    else {
        return Ok(RemoteCacheCredential::None);
    };
    Ok(RemoteCacheCredential::GithubActions(Arc::new(
        GithubActionsOidcCredential::from_env(audience, client, config.retries)?,
    )))
}

fn authorization_header(token: Option<&str>) -> Result<Option<HeaderValue>> {
    let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) else {
        return Ok(None);
    };
    let mut value = HeaderValue::from_str(&format!("Bearer {token}"))?;
    value.set_sensitive(true);
    Ok(Some(value))
}

impl RemoteCacheCredential {
    async fn authorization(&self) -> Result<Option<HeaderValue>> {
        match self {
            Self::None => Ok(None),
            Self::Static(value) => Ok(Some(value.clone())),
            Self::File(path) => {
                let token = tokio::fs::read_to_string(path).await.map_err(|err| {
                    eyre!(
                        "failed to read remote cache token file {}: {err}",
                        path.display()
                    )
                })?;
                authorization_header(Some(&token))?
                    .ok_or_else(|| eyre!("remote cache token file {} is empty", path.display()))
                    .map(Some)
            }
            Self::GithubActions(credential) => credential.authorization().await.map(Some),
        }
    }
}

impl GithubActionsOidcCredential {
    fn from_env(audience: &str, client: reqwest::Client, retries: i64) -> Result<Self> {
        let request_url = std::env::var("ACTIONS_ID_TOKEN_REQUEST_URL").map_err(|_| {
            eyre!(
                "remote cache OIDC audience requires GitHub Actions OIDC; \
                 grant `id-token: write` or set MISE_TASK_CACHE_REMOTE_TOKEN"
            )
        })?;
        let request_token = std::env::var("ACTIONS_ID_TOKEN_REQUEST_TOKEN").map_err(|_| {
            eyre!(
                "remote cache OIDC audience requires GitHub Actions OIDC; \
                 ACTIONS_ID_TOKEN_REQUEST_TOKEN is missing"
            )
        })?;
        let request_url: Url = request_url
            .parse()
            .map_err(|err| eyre!("invalid GitHub Actions OIDC request URL: {err}"))?;
        Self::new(audience, request_url, &request_token, client, retries)
    }

    fn new(
        audience: &str,
        mut request_url: Url,
        request_token: &str,
        client: reqwest::Client,
        retries: i64,
    ) -> Result<Self> {
        validate_oidc_request_url(&request_url)?;
        let query = request_url
            .query_pairs()
            .filter(|(key, _)| key != "audience")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        request_url.set_query(None);
        request_url
            .query_pairs_mut()
            .extend_pairs(query)
            .append_pair("audience", audience);
        let request_token = authorization_header(Some(request_token))?
            .ok_or_else(|| eyre!("GitHub Actions OIDC request token is empty"))?;
        Ok(Self {
            audience: audience.to_string(),
            request_url,
            request_token,
            client,
            retries,
            cached: tokio::sync::Mutex::new(None),
        })
    }

    async fn authorization(&self) -> Result<HeaderValue> {
        const REFRESH_LEEWAY_SECONDS: u64 = 60;
        let mut cached = self.cached.lock().await;
        let now = unix_timestamp()?;
        if let Some(token) = cached.as_ref()
            && token.expires_at > now.saturating_add(REFRESH_LEEWAY_SECONDS)
        {
            return Ok(token.authorization.clone());
        }
        let response: GithubActionsOidcResponse =
            retry_async("GET", &self.request_url, self.retries, || async {
                Ok(self
                    .client
                    .get(self.request_url.clone())
                    .header(AUTHORIZATION, self.request_token.clone())
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?)
            })
            .await
            .map_err(|err| {
                eyre!(
                    "failed to acquire GitHub Actions OIDC token for audience {:?}: {err}",
                    self.audience
                )
            })?;
        let expires_at = jwt_expiry(&response.value)?;
        if expires_at <= now.saturating_add(REFRESH_LEEWAY_SECONDS) {
            bail!("GitHub Actions OIDC token expires too soon");
        }
        let authorization = authorization_header(Some(&response.value))?
            .ok_or_else(|| eyre!("GitHub Actions returned an empty OIDC token"))?;
        *cached = Some(CachedOidcToken {
            authorization: authorization.clone(),
            expires_at,
        });
        Ok(authorization)
    }
}

fn jwt_expiry(token: &str) -> Result<u64> {
    let payload = token
        .split('.')
        .nth(1)
        .ok_or_else(|| eyre!("GitHub Actions returned a malformed OIDC token"))?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| eyre!("GitHub Actions returned a malformed OIDC token"))?;
    let claims: JwtExpiry = serde_json::from_slice(&payload)
        .map_err(|_| eyre!("GitHub Actions OIDC token is missing a valid expiry"))?;
    Ok(claims.exp)
}

fn unix_timestamp() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| eyre!("system clock is before the Unix epoch: {err}"))?
        .as_secs())
}

fn validate_oidc_request_url(url: &Url) -> Result<()> {
    if url.scheme() == "https"
        || url.scheme() == "http"
            && url.host().is_some_and(|host| match host {
                Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
                Host::Ipv4(address) => address.is_loopback(),
                Host::Ipv6(address) => address.is_loopback(),
            })
    {
        Ok(())
    } else {
        bail!("GitHub Actions OIDC request URL must use HTTPS")
    }
}

fn validate_remote_url(base_url: &Url, authenticated: bool) -> Result<()> {
    if base_url.scheme() == "https" {
        return Ok(());
    }
    if base_url.scheme() != "http" {
        bail!("remote cache URL must use HTTPS");
    }
    let is_loopback = base_url.host().is_some_and(|host| match host {
        Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(address) => address.is_loopback(),
        Host::Ipv6(address) => address.is_loopback(),
    });
    if !is_loopback && authenticated {
        bail!("remote cache URL must use HTTPS except for loopback development servers");
    }
    if !is_loopback {
        warn!(
            "using an unauthenticated remote build cache over plain HTTP; cache traffic can be read \
             or modified in transit"
        );
    }
    Ok(())
}

fn normalized_base_url(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    url
}

fn retry_delays(retries: i64) -> impl Iterator<Item = Duration> {
    [200u64, 1_000, 4_000, 15_000]
        .into_iter()
        .chain(std::iter::repeat(15_000))
        .map(Duration::from_millis)
        .map(|duration| {
            let factor = 0.5 + rand::random::<f64>() * 0.5;
            Duration::from_secs_f64(duration.as_secs_f64() * factor)
        })
        .take(retries.max(0) as usize)
}

/// hyper-util exposes DNS failures in the error chain as a `dns error` source,
/// but reqwest intentionally erases the concrete connector type. Match that
/// stable connector error label rather than platform-specific resolver text.
fn is_dns_error(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source.to_string() == "dns error" {
            return true;
        }
        current = source.source();
    }
    false
}

fn is_transient(error: &eyre::Report) -> bool {
    // An unavailable hostname is a deterministic configuration error. reqwest
    // categorizes it as a connect error, but retrying only delays the diagnosis.
    if is_dns_error(error.as_ref()) {
        return false;
    }
    error.chain().any(|source| {
        let Some(error) = source.downcast_ref::<reqwest::Error>() else {
            return false;
        };
        if error.is_timeout() || error.is_connect() || error.is_body() {
            return true;
        }
        error.status().is_some_and(|status| {
            let status = status.as_u16();
            status == 408 || status == 429 || (500..600).contains(&status)
        })
    })
}

async fn retry_async<F, Fut, T>(verb: &str, url: &Url, retries: i64, mut operation: F) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut delays = retry_delays(retries);
    let mut attempt = 1;
    loop {
        let started_at = Instant::now();
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if is_transient(&error) => {
                let Some(delay) = delays.next() else {
                    return Err(error);
                };
                warn!(
                    "HTTP {verb} {url} attempt {attempt} failed after {:?} (transient): {error}; retrying in {delay:?}",
                    started_at.elapsed()
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_json_uses_jcs_key_and_number_encoding() {
        let value = serde_json::json!({"z": 1.0e30, "a": {"d": true, "c": null}});
        assert_eq!(
            canonical_json(&value).unwrap(),
            br#"{"a":{"c":null,"d":true},"z":1e+30}"#
        );
    }

    #[test]
    fn dns_errors_are_not_transient() {
        #[derive(Debug)]
        struct DnsError;

        impl std::fmt::Display for DnsError {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("dns error")
            }
        }

        impl std::error::Error for DnsError {}

        let error = eyre::Report::new(DnsError);
        assert!(is_dns_error(error.as_ref()));
        assert!(!is_transient(&error));
    }

    #[test]
    fn cache_digest_verifies_its_declared_algorithm() {
        let bytes = b"remote cache blob";
        let sha256 = CacheDigest {
            algorithm: "sha256".into(),
            hash: hex::encode(sha2::Sha256::digest(bytes)),
            size: bytes.len() as u64,
        };
        assert!(sha256.matches_bytes(bytes).unwrap());
        assert!(!sha256.matches_bytes(b"different").unwrap());

        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), bytes).unwrap();
        assert!(sha256.matches_file(file.path()).unwrap());
        assert_eq!(
            CacheDigest::blake3_file(file.path()).unwrap().size,
            bytes.len() as u64
        );
        assert!(
            CacheDigest::blake3_file(file.path())
                .unwrap()
                .matches_bytes(bytes)
                .unwrap()
        );
    }

    #[test]
    fn action_result_keys_require_blake3() {
        let client = RemoteCacheClient::new(RemoteCacheConfig {
            base_url: "http://127.0.0.1:1".parse().unwrap(),
            namespace: "test".into(),
            token: None,
            token_file: None,
            oidc_audience: None,
            connect_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(1),
            download_timeout: Duration::from_secs(1),
            retries: 0,
        })
        .unwrap();
        let action = CacheDigest {
            algorithm: "sha256".into(),
            hash: "0".repeat(64),
            size: 0,
        };

        assert!(
            client
                .action_result_endpoint(&action)
                .unwrap_err()
                .to_string()
                .contains("must use blake3")
        );
    }

    #[tokio::test]
    async fn downloads_negotiated_blob_packs_and_omits_missing_objects() {
        let mut server = mockito::Server::new_async().await;
        let first_bytes = b"first packed blob";
        let second_bytes = b"second packed blob";
        let first = CacheDigest::blake3(first_bytes);
        let second = CacheDigest::blake3(second_bytes);
        let missing = CacheDigest::blake3(b"missing packed blob");
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .match_header(PROTOCOL_HEADER, "1")
            .match_header(AUTHORIZATION.as_str(), "Bearer test-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1024}
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let packed = encode_blob_pack(&[
            (&first, first_bytes.as_slice()),
            (&second, second_bytes.as_slice()),
        ]);
        let packed_len = packed.len().to_string();
        let packed_blobs = 2.to_string();
        let packed_payload_bytes = (first.size + second.size).to_string();
        let request = server
            .mock("POST", "/v1/blobs:pack")
            .match_header(PROTOCOL_HEADER, "1")
            .match_header(NAMESPACE_HEADER, "test")
            .match_header("content-type", DIGEST_LIST_MEDIA_TYPE)
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_header("content-length", &packed_len)
            .with_header(BLOB_PACK_BLOBS_HEADER, &packed_blobs)
            .with_header(BLOB_PACK_BYTES_HEADER, &packed_payload_bytes)
            .with_body(packed)
            .expect(1)
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let pack = client
            .get_blob_pack(
                &[first.clone(), missing, second.clone(), first.clone()],
                staging.path(),
            )
            .await
            .unwrap()
            .unwrap();

        assert_eq!(pack.requests, 1);
        assert_eq!(pack.blob_count, 2);
        assert_eq!(pack.payload_bytes, first.size + second.size);
        assert_eq!(
            pack.framed_bytes,
            BLOB_PACK_MAGIC.len() as u64 + 2 * BLOB_PACK_HEADER_BYTES + first.size + second.size
        );
        assert_eq!(pack.blobs.len(), 2);
        assert_eq!(fs::read(&pack.blobs[0].1).unwrap(), first_bytes);
        assert_eq!(fs::read(&pack.blobs[1].1).unwrap(), second_bytes);
        capabilities.assert_async().await;
        request.assert_async().await;
    }

    #[tokio::test]
    async fn rejects_mismatched_blob_pack_metadata() {
        let mut server = mockito::Server::new_async().await;
        let contents = b"packed blob";
        let digest = CacheDigest::blake3(contents);
        mock_blob_pack_capabilities(&mut server).await;
        server
            .mock("POST", "/v1/blobs:pack")
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_header(BLOB_PACK_BLOBS_HEADER, "2")
            .with_body(encode_blob_pack(&[(&digest, contents.as_slice())]))
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let error = client
            .get_blob_pack(&[digest], staging.path())
            .await
            .err()
            .unwrap();

        assert!(error.to_string().contains("blob count metadata mismatch"));
    }

    #[tokio::test]
    async fn rejects_malformed_blob_pack_metadata() {
        let mut server = mockito::Server::new_async().await;
        let contents = b"packed blob";
        let digest = CacheDigest::blake3(contents);
        mock_blob_pack_capabilities(&mut server).await;
        server
            .mock("POST", "/v1/blobs:pack")
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_header(BLOB_PACK_BYTES_HEADER, "not-a-number")
            .with_body(encode_blob_pack(&[(&digest, contents.as_slice())]))
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let error = client
            .get_blob_pack(&[digest], staging.path())
            .await
            .err()
            .unwrap();

        assert!(error.to_string().contains("not an unsigned integer"));
    }

    #[tokio::test]
    async fn rejects_unrequested_blob_pack_frames() {
        let mut server = mockito::Server::new_async().await;
        let requested = CacheDigest::blake3(b"requested");
        let injected_bytes = b"not requested";
        let injected = CacheDigest::blake3(injected_bytes);
        server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1024}
                })
                .to_string(),
            )
            .create_async()
            .await;
        server
            .mock("POST", "/v1/blobs:pack")
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_body(encode_blob_pack(&[(&injected, injected_bytes.as_slice())]))
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let error = client
            .get_blob_pack(&[requested], staging.path())
            .await
            .err()
            .unwrap();

        assert!(error.to_string().contains("unrequested digest"));
    }

    #[tokio::test]
    async fn falls_back_when_blob_packs_are_not_advertised() {
        let mut server = mockito::Server::new_async().await;
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        assert!(
            client
                .get_blob_pack(&[CacheDigest::blake3(b"blob")], staging.path())
                .await
                .unwrap()
                .is_none()
        );
        capabilities.assert_async().await;
    }

    #[tokio::test]
    async fn disables_blob_packs_when_the_advertised_endpoint_is_unavailable() {
        let mut server = mockito::Server::new_async().await;
        let capabilities = server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1024}
                })
                .to_string(),
            )
            .expect(1)
            .create_async()
            .await;
        let request = server
            .mock("POST", "/v1/blobs:pack")
            .with_status(404)
            .expect(1)
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();
        let digest = CacheDigest::blake3(b"blob");

        assert!(
            client
                .get_blob_pack(std::slice::from_ref(&digest), staging.path())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            client
                .get_blob_pack(&[digest], staging.path())
                .await
                .unwrap()
                .is_none()
        );
        capabilities.assert_async().await;
        request.assert_async().await;
    }

    #[tokio::test]
    async fn rejects_truncated_blob_pack_frames() {
        let mut server = mockito::Server::new_async().await;
        let contents = b"complete blob";
        let digest = CacheDigest::blake3(contents);
        let mut pack = encode_blob_pack(&[(&digest, contents.as_slice())]);
        pack.truncate(pack.len() - 3);
        mock_blob_pack_capabilities(&mut server).await;
        server
            .mock("POST", "/v1/blobs:pack")
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_body(pack)
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let error = match client.get_blob_pack(&[digest], staging.path()).await {
            Err(error) => error,
            Ok(_) => panic!("truncated pack should be rejected"),
        };

        assert!(
            error
                .to_string()
                .contains("ended before a blob was complete")
        );
    }

    #[tokio::test]
    async fn rejects_blob_pack_frames_with_corrupt_content() {
        let mut server = mockito::Server::new_async().await;
        let digest = CacheDigest::blake3(b"expected");
        let corrupt = b"corrupt!";
        let pack = encode_blob_pack(&[(&digest, corrupt.as_slice())]);
        mock_blob_pack_capabilities(&mut server).await;
        server
            .mock("POST", "/v1/blobs:pack")
            .with_status(200)
            .with_header("content-type", BLOB_PACK_MEDIA_TYPE)
            .with_body(pack)
            .create_async()
            .await;
        let client = test_client(&server);
        let staging = tempfile::tempdir().unwrap();

        let error = match client.get_blob_pack(&[digest], staging.path()).await {
            Err(error) => error,
            Ok(_) => panic!("corrupt pack should be rejected"),
        };

        assert!(error.to_string().contains("failed digest verification"));
    }

    #[test]
    fn blob_pack_chunk_honors_item_and_byte_limits() {
        let first = CacheDigest::blake3(b"1234");
        let second = CacheDigest::blake3(b"5678");
        let oversized = CacheDigest::blake3(b"123456789");
        let chunk = blob_pack_chunk(
            &[first.clone(), second.clone(), first.clone(), oversized],
            BlobPackLimits {
                max_items: 10,
                max_bytes: 7,
            },
        )
        .unwrap();

        assert_eq!(chunk, vec![first]);

        let chunk = blob_pack_chunk(
            &[CacheDigest::blake3(b"a"), CacheDigest::blake3(b"b")],
            BlobPackLimits {
                max_items: 1,
                max_bytes: 100,
            },
        )
        .unwrap();
        assert_eq!(chunk.len(), 1);
    }

    #[test]
    fn blob_pack_timeout_scales_with_declared_work() {
        let base = Duration::from_secs(10);
        let small = CacheDigest::blake3(b"small");
        assert_eq!(blob_pack_download_timeout(base, &[small]), base);

        let large = CacheDigest {
            algorithm: "blake3".into(),
            hash: "0".repeat(64),
            size: MAX_STAGED_BLOB_PACK_BYTES,
        };
        assert_eq!(
            blob_pack_download_timeout(base, &[large]),
            base.saturating_mul(4)
        );

        let many = (0..=BLOB_PACK_TIMEOUT_ITEMS_PER_UNIT)
            .map(|index| CacheDigest::blake3(index.to_string().as_bytes()))
            .collect::<Vec<_>>();
        assert_eq!(
            blob_pack_download_timeout(base, &many),
            base.saturating_mul(2)
        );
    }

    #[test]
    fn bearer_authorization_headers_are_sensitive() {
        let header = authorization_header(Some(" test-token ")).unwrap().unwrap();
        assert_eq!(header, "Bearer test-token");
        assert!(header.is_sensitive());
        assert!(authorization_header(Some(" ")).unwrap().is_none());
    }

    fn test_client(server: &mockito::ServerGuard) -> RemoteCacheClient {
        RemoteCacheClient::new(RemoteCacheConfig {
            base_url: server.url().parse().unwrap(),
            namespace: "test".into(),
            token: Some("test-token".into()),
            token_file: None,
            oidc_audience: None,
            connect_timeout: Duration::from_secs(1),
            read_timeout: Duration::from_secs(1),
            download_timeout: Duration::from_secs(1),
            retries: 0,
        })
        .unwrap()
    }

    async fn mock_blob_pack_capabilities(server: &mut mockito::ServerGuard) {
        server
            .mock("GET", "/v1/capabilities")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "protocol":{"major":1},
                    "features":{"blob_packs":true},
                    "limits":{"max_batch_items":100,"max_pack_bytes":1024}
                })
                .to_string(),
            )
            .create_async()
            .await;
    }

    fn encode_blob_pack(entries: &[(&CacheDigest, &[u8])]) -> Vec<u8> {
        let mut pack = BLOB_PACK_MAGIC.to_vec();
        for (digest, contents) in entries {
            assert_eq!(digest.size, contents.len() as u64);
            pack.push(match digest.algorithm.as_str() {
                "blake3" => 1,
                "sha256" => 2,
                algorithm => panic!("unexpected test digest algorithm {algorithm}"),
            });
            pack.extend(hex::decode(&digest.hash).unwrap());
            pack.extend(digest.size.to_be_bytes());
            pack.extend_from_slice(contents);
        }
        pack
    }

    #[tokio::test]
    async fn token_file_credentials_are_reloaded() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cache-token");
        fs::write(&path, "first-token\n").unwrap();
        let credential = RemoteCacheCredential::File(path.clone());

        let first = credential.authorization().await.unwrap().unwrap();
        assert_eq!(first, "Bearer first-token");
        assert!(first.is_sensitive());

        fs::write(path, "rotated-token\n").unwrap();
        let rotated = credential.authorization().await.unwrap().unwrap();
        assert_eq!(rotated, "Bearer rotated-token");
    }

    #[tokio::test]
    async fn github_actions_oidc_tokens_are_acquired_and_cached() {
        let mut server = mockito::Server::new_async().await;
        let expires_at = unix_timestamp().unwrap() + 3600;
        let token = test_jwt(expires_at);
        let token_response = serde_json::json!({"value":token}).to_string();
        let request = server
            .mock("GET", "/oidc")
            .match_query(mockito::Matcher::UrlEncoded(
                "audience".into(),
                "https://cache.example.com".into(),
            ))
            .match_header("authorization", "Bearer request-secret")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(token_response)
            .expect(1)
            .create_async()
            .await;
        let credential = GithubActionsOidcCredential::new(
            "https://cache.example.com",
            format!("{}/oidc?api-version=1&audience=old", server.url())
                .parse()
                .unwrap(),
            "request-secret",
            reqwest::Client::new(),
            0,
        )
        .unwrap();
        assert_eq!(
            credential.request_url.query_pairs().collect::<Vec<_>>(),
            vec![
                ("api-version".into(), "1".into()),
                ("audience".into(), "https://cache.example.com".into()),
            ]
        );

        let first = credential.authorization().await.unwrap();
        let second = credential.authorization().await.unwrap();

        assert_eq!(first, format!("Bearer {token}"));
        assert_eq!(first, second);
        assert!(first.is_sensitive());
        request.assert_async().await;
    }

    #[test]
    fn oidc_request_urls_require_https_except_for_loopback() {
        validate_oidc_request_url(&"https://example.com/oidc".parse().unwrap()).unwrap();
        validate_oidc_request_url(&"http://127.0.0.1:3000/oidc".parse().unwrap()).unwrap();
        assert!(validate_oidc_request_url(&"http://example.com/oidc".parse().unwrap()).is_err());
    }

    fn test_jwt(expires_at: u64) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{}");
        let claims = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&serde_json::json!({"exp":expires_at})).unwrap());
        format!("{header}.{claims}.signature")
    }

    #[test]
    fn remote_urls_require_https_for_authenticated_requests() {
        for url in [
            "http://localhost:3000",
            "http://127.0.0.1:3000",
            "http://[::1]:3000",
            "https://cache.example.com",
        ] {
            validate_remote_url(&url.parse().unwrap(), true).unwrap();
        }
        let insecure: Url = "http://cache.example.com".parse().unwrap();
        assert!(validate_remote_url(&insecure, true).is_err());
        validate_remote_url(&insecure, false).unwrap();
        assert!(validate_remote_url(&"ftp://localhost/cache".parse().unwrap(), false).is_err());
    }
}
