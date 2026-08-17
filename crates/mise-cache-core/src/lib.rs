use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::{Result, bail, eyre};
use log::warn;
use reqwest::StatusCode;
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, ETAG, HeaderValue, IF_MATCH, IF_NONE_MATCH,
};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
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

    #[test]
    fn bearer_authorization_headers_are_sensitive() {
        let header = authorization_header(Some(" test-token ")).unwrap().unwrap();
        assert_eq!(header, "Bearer test-token");
        assert!(header.is_sensitive());
        assert!(authorization_header(Some(" ")).unwrap().is_none());
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
