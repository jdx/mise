use crate::file;
use crate::task::task_cache::{
    CACHE_FORMAT_VERSION, CacheManifest, TaskCacheOutput, calculate_artifact_checksum,
    canonical_json,
};
use async_trait::async_trait;
use eyre::{Result, bail, eyre};
use jdx_tar::{Archive, Builder, EntryType, Header};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use url::Url;

const REMOTE_CACHE_PROTOCOL_VERSION: u8 = 1;
const REMOTE_CACHE_PROTOCOL_HEADER: &str = "Mise-Cache-Protocol";
const REMOTE_CACHE_NAMESPACE_HEADER: &str = "Mise-Cache-Namespace";
const REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE: &str =
    "application/vnd.mise.cache-action-result.v1+json";
const REMOTE_CACHE_DIRECTORY_MEDIA_TYPE: &str = "application/vnd.mise.cache-directory.v1+json";
const REMOTE_CACHE_CLIENT_METADATA_MEDIA_TYPE: &str =
    "application/vnd.mise.cache-client-metadata.v1+json";
const REMOTE_CACHE_BLOB_MEDIA_TYPE: &str = "application/octet-stream";

/// Version of the cache-store contract. This is independent of the artifact
/// manifest format so stores and transports can evolve without changing keys.
pub(crate) const TASK_CACHE_STORE_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct CacheDigest {
    algorithm: String,
    hash: String,
    size: u64,
}

impl CacheDigest {
    fn blake3(bytes: &[u8]) -> Self {
        Self {
            algorithm: "blake3".into(),
            hash: blake3::hash(bytes).to_hex().to_string(),
            size: bytes.len() as u64,
        }
    }

    fn validate(&self) -> Result<()> {
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

    fn matches_bytes(&self, bytes: &[u8]) -> Result<bool> {
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

    fn matches_file(&self, path: &Path) -> Result<bool> {
        self.validate()?;
        if self.size != fs::metadata(path)?.len() {
            return Ok(false);
        }
        let hash = match self.algorithm.as_str() {
            "blake3" => crate::hash::file_hash_blake3(path, None)?,
            "sha256" => crate::hash::file_hash_sha256(path, None)?,
            _ => unreachable!("digest algorithm was validated"),
        };
        Ok(self.hash == hash)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteActionResultEnvelope {
    result: RemoteActionResult,
    #[serde(default)]
    signatures: Vec<RemoteSignature>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteActionResult {
    action: CacheDigest,
    #[serde(default)]
    metadata: Option<CacheDigest>,
    #[serde(default)]
    output_root: Option<CacheDigest>,
    version: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteSignature {
    algorithm: String,
    key_id: String,
    signature: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteClientMetadata {
    execution_duration_ns: u64,
    output: Vec<TaskCacheOutput>,
    restored_bytes: u64,
    roots: Vec<String>,
    task_identity: String,
    version: u8,
}

impl RemoteClientMetadata {
    fn from_manifest(manifest: &CacheManifest) -> Self {
        Self {
            execution_duration_ns: manifest.execution_duration_ns,
            output: manifest.output.clone(),
            restored_bytes: manifest.restored_bytes,
            roots: manifest
                .roots
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            task_identity: manifest.task_identity.clone(),
            version: 1,
        }
    }

    fn into_manifest(self, key: &str) -> Result<CacheManifest> {
        if self.version != 1 {
            bail!("unsupported remote cache client metadata version");
        }
        let roots = self.roots.into_iter().map(PathBuf::from).collect();
        Ok(CacheManifest {
            format: CACHE_FORMAT_VERSION,
            key: key.to_string(),
            task_identity: self.task_identity,
            artifact_checksum: None,
            roots,
            output: self.output,
            restored_bytes: self.restored_bytes,
            execution_duration_ns: self.execution_duration_ns,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteDirectory {
    directories: Vec<RemoteDirectoryNode>,
    files: Vec<RemoteFileNode>,
    symlinks: Vec<RemoteSymlinkNode>,
    version: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteDirectoryNode {
    digest: CacheDigest,
    mode: u32,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteFileNode {
    digest: CacheDigest,
    executable: bool,
    mode: u32,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RemoteSymlinkNode {
    mode: u32,
    name: String,
    target: String,
}

enum CasBlobSource {
    Bytes(Vec<u8>),
    File(tempfile::NamedTempFile),
}

struct CasBlobUpload {
    digest: CacheDigest,
    source: CasBlobSource,
}

enum ArchiveNode {
    Directory {
        mode: u32,
    },
    File {
        digest: CacheDigest,
        executable: bool,
        mode: u32,
        file: tempfile::NamedTempFile,
    },
    Symlink {
        mode: u32,
        target: PathBuf,
    },
}

pub(crate) struct TaskCacheStoreEntry {
    pub(crate) manifest: Vec<u8>,
    pub(crate) artifact: Option<TaskCacheStoreArtifact>,
}

pub(crate) struct TaskCacheStoreArtifact {
    path: PathBuf,
    _temporary: Option<tempfile::TempPath>,
}

impl TaskCacheStoreArtifact {
    fn stored(path: PathBuf) -> Self {
        Self {
            path,
            _temporary: None,
        }
    }

    fn temporary(file: tempfile::NamedTempFile) -> Self {
        let path = file.path().to_path_buf();
        Self {
            path,
            _temporary: Some(file.into_temp_path()),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) struct TaskCacheStoreWrite {
    artifact_path: PathBuf,
    manifest_path: PathBuf,
}

impl TaskCacheStoreWrite {
    pub(crate) fn artifact_path(&self) -> &Path {
        &self.artifact_path
    }
}

impl Drop for TaskCacheStoreWrite {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.artifact_path);
        let _ = fs::remove_file(&self.manifest_path);
    }
}

/// Storage boundary for versioned task-cache entries.
///
/// Artifacts are materialized as files so implementations can stream large
/// payloads without buffering them in memory. A returned artifact path must
/// remain valid until the next mutating operation for the same key.
#[async_trait]
pub(crate) trait TaskCacheStore: Send + Sync {
    fn version(&self) -> u8;
    async fn get(&self, key: &str, action_size: u64) -> Result<Option<TaskCacheStoreEntry>>;
    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite>;
    async fn commit(
        &self,
        key: &str,
        action: &[u8],
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()>;
    async fn remove(&self, key: &str) -> Result<()>;
    async fn remove_local(&self, key: &str, _action_size: u64) -> Result<()> {
        self.remove(key).await
    }
    fn touch(&self, key: &str);
}

pub(crate) struct CompositeTaskCacheStore {
    local: Arc<dyn TaskCacheStore>,
    remote: Arc<dyn TaskCacheStore>,
}

impl CompositeTaskCacheStore {
    pub(crate) fn new(
        local: Arc<dyn TaskCacheStore>,
        remote: Arc<dyn TaskCacheStore>,
    ) -> Result<Self> {
        if local.version() != remote.version() {
            bail!(
                "task cache store version mismatch: local version {}, remote version {}",
                local.version(),
                remote.version()
            );
        }
        Ok(Self { local, remote })
    }

    fn copy_artifact(source: Option<&Path>, write: &TaskCacheStoreWrite) -> Result<bool> {
        let Some(source) = source else {
            return Ok(false);
        };
        fs::copy(source, write.artifact_path())?;
        Ok(true)
    }

    async fn promote(
        &self,
        key: &str,
        action_size: u64,
        entry: &TaskCacheStoreEntry,
    ) -> Result<TaskCacheStoreEntry> {
        // A remote lookup is an access, so promotion intentionally starts a
        // fresh local inactivity window for task.cache_max_age.
        let write = self.local.begin_write(key)?;
        let has_artifact = Self::copy_artifact(
            entry.artifact.as_ref().map(TaskCacheStoreArtifact::path),
            &write,
        )?;
        self.local
            .commit(key, &[], &write, &entry.manifest, has_artifact)
            .await?;
        self.local
            .get(key, action_size)
            .await?
            .ok_or_else(|| eyre::eyre!("promoted task cache entry disappeared"))
    }
}

#[async_trait]
impl TaskCacheStore for CompositeTaskCacheStore {
    fn version(&self) -> u8 {
        self.local.version()
    }

    async fn get(&self, key: &str, action_size: u64) -> Result<Option<TaskCacheStoreEntry>> {
        if let Some(entry) = self.local.get(key, action_size).await? {
            return Ok(Some(entry));
        }
        let entry = match self.remote.get(key, action_size).await {
            Ok(Some(entry)) => entry,
            Ok(None) => return Ok(None),
            Err(err) => {
                warn!("remote task cache lookup failed for {key}; using local cache only: {err}");
                return Ok(None);
            }
        };
        match self.promote(key, action_size, &entry).await {
            Ok(entry) => Ok(Some(entry)),
            Err(err) => {
                warn!("failed to promote remote task cache entry {key} locally: {err}");
                Ok(Some(entry))
            }
        }
    }

    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
        self.local.begin_write(key)
    }

    async fn commit(
        &self,
        key: &str,
        action: &[u8],
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        self.local
            .commit(key, action, write, manifest, has_artifact)
            .await?;
        let mirror: Result<()> = async {
            let entry = self
                .local
                .get(key, action.len() as u64)
                .await?
                .ok_or_else(|| eyre::eyre!("published local task cache entry disappeared"))?;
            let remote_write = self.remote.begin_write(key)?;
            let remote_has_artifact = Self::copy_artifact(
                entry.artifact.as_ref().map(TaskCacheStoreArtifact::path),
                &remote_write,
            )?;
            self.remote
                .commit(
                    key,
                    action,
                    &remote_write,
                    &entry.manifest,
                    remote_has_artifact,
                )
                .await
        }
        .await;
        if let Err(err) = mirror {
            warn!("failed to mirror task cache entry {key} remotely: {err}");
        }
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        // Delete remotely first so a failed remote delete cannot allow the
        // entry to be promoted back after its local copy was removed.
        self.remote.remove(key).await?;
        self.local.remove(key).await
    }

    async fn remove_local(&self, key: &str, action_size: u64) -> Result<()> {
        let Err(remove_err) = self.local.remove_local(key, action_size).await else {
            return Ok(());
        };
        let Some(entry) = self.remote.get(key, action_size).await? else {
            return Err(remove_err);
        };
        self.promote(key, action_size, &entry)
            .await
            .map(|_| ())
            .map_err(|promote_err| {
                eyre::eyre!(
                    "failed to remove expired local cache entry: {remove_err}; \
                 failed to refresh it from remote: {promote_err}"
                )
            })
    }

    fn touch(&self, key: &str) {
        self.local.touch(key);
        self.remote.touch(key);
    }
}

pub(crate) fn compose_task_cache_stores(
    local: Arc<dyn TaskCacheStore>,
    remote: Option<(Url, String, PathBuf)>,
) -> Result<Arc<dyn TaskCacheStore>> {
    match remote {
        Some((base_url, namespace, staging_dir)) => {
            let remote = Arc::new(HttpTaskCacheStore {
                base_url: normalized_base_url(base_url),
                namespace,
                staging_dir,
                client: reqwest::Client::new(),
            });
            Ok(Arc::new(CompositeTaskCacheStore::new(local, remote)?))
        }
        None => Ok(local),
    }
}

struct HttpTaskCacheStore {
    base_url: Url,
    namespace: String,
    staging_dir: PathBuf,
    client: reqwest::Client,
}

impl HttpTaskCacheStore {
    fn action_result_endpoint(&self, key: &str, action_size: u64) -> Result<Url> {
        validate_remote_key(key)?;
        Ok(self.base_url.join(&format!(
            "v{REMOTE_CACHE_PROTOCOL_VERSION}/action-results/blake3/{key}/{action_size}"
        ))?)
    }

    fn blob_endpoint(&self, digest: &CacheDigest) -> Result<Url> {
        digest.validate()?;
        Ok(self.base_url.join(&format!(
            "v{REMOTE_CACHE_PROTOCOL_VERSION}/blobs/{}/{}/{}",
            digest.algorithm, digest.hash, digest.size
        ))?)
    }

    fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        media_type: &'static str,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .header(REMOTE_CACHE_PROTOCOL_HEADER, "1")
            .header(REMOTE_CACHE_NAMESPACE_HEADER, &self.namespace)
            .header(ACCEPT, media_type)
    }

    async fn get_blob(&self, digest: &CacheDigest, media_type: &'static str) -> Result<Vec<u8>> {
        digest.validate()?;
        let response = self
            .request(
                reqwest::Method::GET,
                self.blob_endpoint(digest)?,
                media_type,
            )
            .send()
            .await?
            .error_for_status()?;
        let bytes = response.bytes().await?.to_vec();
        if !digest.matches_bytes(&bytes)? {
            bail!("remote cache blob failed digest verification");
        }
        Ok(bytes)
    }

    async fn get_blob_file(&self, digest: &CacheDigest) -> Result<tempfile::NamedTempFile> {
        let mut response = self
            .request(
                reqwest::Method::GET,
                self.blob_endpoint(digest)?,
                REMOTE_CACHE_BLOB_MEDIA_TYPE,
            )
            .send()
            .await?;
        response.error_for_status_ref()?;
        file::create_dir_all(&self.staging_dir)?;
        let temporary = tempfile::NamedTempFile::new_in(&self.staging_dir)?;
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
    }

    async fn put_blob(&self, upload: &CasBlobUpload) -> Result<()> {
        let (length, body) = match &upload.source {
            CasBlobSource::Bytes(bytes) => (bytes.len() as u64, reqwest::Body::from(bytes.clone())),
            CasBlobSource::File(file) => {
                let file = tokio::fs::File::open(file.path()).await?;
                let length = file.metadata().await?.len();
                let stream = tokio_util::io::ReaderStream::new(file);
                (length, reqwest::Body::wrap_stream(stream))
            }
        };
        let response = self
            .request(
                reqwest::Method::PUT,
                self.blob_endpoint(&upload.digest)?,
                REMOTE_CACHE_BLOB_MEDIA_TYPE,
            )
            .header(CONTENT_TYPE, REMOTE_CACHE_BLOB_MEDIA_TYPE)
            .header(CONTENT_LENGTH, length)
            .header(IF_NONE_MATCH, "*")
            .body(body)
            .send()
            .await?;
        if response.status() != StatusCode::PRECONDITION_FAILED {
            response.error_for_status()?;
        }
        Ok(())
    }

    async fn put_action_result(
        &self,
        key: &str,
        action_size: u64,
        result: &RemoteActionResultEnvelope,
    ) -> Result<()> {
        let response = self
            .request(
                reqwest::Method::PUT,
                self.action_result_endpoint(key, action_size)?,
                REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE,
            )
            .header(CONTENT_TYPE, REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE)
            .header(IF_NONE_MATCH, "*")
            .body(serde_json::to_vec(result)?)
            .send()
            .await?;
        if response.status() != StatusCode::PRECONDITION_FAILED {
            response.error_for_status()?;
        }
        Ok(())
    }
}

#[async_trait]
impl TaskCacheStore for HttpTaskCacheStore {
    fn version(&self) -> u8 {
        TASK_CACHE_STORE_VERSION
    }

    async fn get(&self, key: &str, action_size: u64) -> Result<Option<TaskCacheStoreEntry>> {
        let response = self
            .request(
                reqwest::Method::GET,
                self.action_result_endpoint(key, action_size)?,
                REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE,
            )
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let envelope: RemoteActionResultEnvelope = response.error_for_status()?.json().await?;
        if envelope.result.version != 1
            || envelope.result.action.algorithm != "blake3"
            || envelope.result.action.hash != key
            || envelope.result.action.size != action_size
        {
            bail!("remote action result does not match requested action");
        }
        let metadata = envelope
            .result
            .metadata
            .as_ref()
            .ok_or_else(|| eyre!("remote action result is missing client metadata"))?;
        let metadata_bytes = self
            .get_blob(metadata, REMOTE_CACHE_CLIENT_METADATA_MEDIA_TYPE)
            .await?;
        let metadata: RemoteClientMetadata = serde_json::from_slice(&metadata_bytes)?;
        if canonical_json(&serde_json::to_value(&metadata)?)? != metadata_bytes {
            bail!("remote cache client metadata is not canonical JSON");
        }
        let mut manifest = metadata.into_manifest(key)?;
        for root in &manifest.roots {
            validate_cache_path(root)?;
        }
        let artifact = match &envelope.result.output_root {
            Some(root) => {
                let temporary = materialize_remote_tree(self, root).await?;
                Some(TaskCacheStoreArtifact::temporary(temporary))
            }
            None if manifest.roots.is_empty() => None,
            None => bail!("remote action result is missing its output root"),
        };
        manifest.artifact_checksum = Some(calculate_artifact_checksum(
            &manifest,
            artifact.as_ref().map(TaskCacheStoreArtifact::path),
        )?);
        Ok(Some(TaskCacheStoreEntry {
            manifest: serde_json::to_vec(&manifest)?,
            artifact,
        }))
    }

    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
        validate_remote_key(key)?;
        file::create_dir_all(&self.staging_dir)?;
        let nonce = crate::rand::random_string(8);
        Ok(TaskCacheStoreWrite {
            artifact_path: self.staging_dir.join(format!("{key}.part-{nonce}.tar.zst")),
            manifest_path: self.staging_dir.join(format!("{key}.part-{nonce}.json")),
        })
    }

    async fn commit(
        &self,
        key: &str,
        action: &[u8],
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        validate_remote_key(key)?;
        let action_digest = CacheDigest::blake3(action);
        if action_digest.hash != key {
            bail!("remote cache action bytes do not match cache key");
        }
        let manifest: CacheManifest = serde_json::from_slice(manifest)?;
        if manifest.key != key {
            bail!("local task cache manifest does not match remote action");
        }
        let metadata = canonical_json(&serde_json::to_value(
            RemoteClientMetadata::from_manifest(&manifest),
        )?)?;
        let mut uploads = vec![
            CasBlobUpload {
                digest: action_digest.clone(),
                source: CasBlobSource::Bytes(action.to_vec()),
            },
            CasBlobUpload {
                digest: CacheDigest::blake3(&metadata),
                source: CasBlobSource::Bytes(metadata),
            },
        ];
        let metadata = uploads[1].digest.clone();
        let output_root = if has_artifact {
            let (root, mut artifact_uploads) =
                archive_to_cas(write.artifact_path(), &self.staging_dir)?;
            uploads.append(&mut artifact_uploads);
            Some(root)
        } else {
            None
        };
        let mut published = BTreeSet::new();
        for upload in &uploads {
            if published.insert(upload.digest.clone()) {
                self.put_blob(upload).await?;
            }
        }
        self.put_action_result(
            key,
            action.len() as u64,
            &RemoteActionResultEnvelope {
                result: RemoteActionResult {
                    action: action_digest,
                    metadata: Some(metadata),
                    output_root,
                    version: 1,
                },
                signatures: Vec::new(),
            },
        )
        .await
    }

    async fn remove(&self, _key: &str) -> Result<()> {
        // Ordinary cache writers intentionally have no remote-delete authority.
        Ok(())
    }

    fn touch(&self, _key: &str) {}
}

fn validate_cache_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("remote cache path must be relative");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("remote cache path escapes its output root");
    }
    Ok(())
}

fn cache_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| eyre!("remote cache paths must be valid UTF-8"))?;
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\0']) {
        bail!("invalid remote cache path component");
    }
    Ok(name.to_string())
}

fn validate_cache_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        bail!("invalid remote cache path component");
    }
    Ok(())
}

fn validate_cache_symlink_target(path: &Path, target: &Path) -> Result<()> {
    if target.is_absolute() {
        bail!("remote cache symlink target must be relative");
    }
    let resolved = path.parent().unwrap_or(Path::new("")).join(target);
    let mut depth = 0_i64;
    for component in resolved.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => depth -= 1,
            Component::CurDir => {}
            _ => bail!("remote cache symlink target is unsafe"),
        }
        if depth < 0 {
            bail!("remote cache symlink target escapes its output root");
        }
    }
    Ok(())
}

fn archive_to_cas(path: &Path, staging_dir: &Path) -> Result<(CacheDigest, Vec<CasBlobUpload>)> {
    file::create_dir_all(staging_dir)?;
    let decoder = zstd::Decoder::new(File::open(path)?)?;
    let mut archive = Archive::new(decoder);
    let mut nodes = BTreeMap::<PathBuf, ArchiveNode>::new();
    nodes.insert(PathBuf::new(), ArchiveNode::Directory { mode: 0o755 });

    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?.into_owned();
        validate_cache_path(&entry_path)?;
        let mode = entry.header().mode();
        let entry_type = entry.entry_type();
        let node = if entry_type == EntryType::Directory {
            ArchiveNode::Directory { mode }
        } else if entry_type == EntryType::File {
            let mut temporary = tempfile::NamedTempFile::new_in(staging_dir)?;
            let mut hasher = blake3::Hasher::new();
            let mut size = 0_u64;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = entry.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                temporary.write_all(&buffer[..read])?;
                hasher.update(&buffer[..read]);
                size = size.saturating_add(read as u64);
            }
            temporary.flush()?;
            ArchiveNode::File {
                digest: CacheDigest {
                    algorithm: "blake3".into(),
                    hash: hasher.finalize().to_hex().to_string(),
                    size,
                },
                executable: mode & 0o111 != 0,
                mode,
                file: temporary,
            }
        } else if entry_type == EntryType::Symlink {
            let target = entry
                .header()
                .link_name()
                .ok_or_else(|| eyre!("remote cache symlink is missing its target"))?
                .into_owned();
            validate_cache_symlink_target(&entry_path, &target)?;
            ArchiveNode::Symlink { mode, target }
        } else {
            bail!("unsupported task cache archive entry type");
        };
        if nodes.insert(entry_path.clone(), node).is_some() {
            bail!("task cache archive contains duplicate paths");
        }
        let mut parent = entry_path.parent();
        while let Some(path) = parent {
            nodes
                .entry(path.to_path_buf())
                .or_insert(ArchiveNode::Directory { mode: 0o755 });
            parent = path.parent();
        }
    }

    fn build_directory(
        path: &Path,
        nodes: &BTreeMap<PathBuf, ArchiveNode>,
        directory_uploads: &mut Vec<CasBlobUpload>,
    ) -> Result<CacheDigest> {
        let mut directories = Vec::new();
        let mut files = Vec::new();
        let mut symlinks = Vec::new();
        for (entry_path, node) in nodes {
            if entry_path.as_os_str().is_empty() || entry_path.parent() != Some(path) {
                continue;
            }
            let name = cache_name(entry_path)?;
            match node {
                ArchiveNode::Directory { mode } => directories.push(RemoteDirectoryNode {
                    digest: build_directory(entry_path, nodes, directory_uploads)?,
                    mode: *mode,
                    name,
                }),
                ArchiveNode::File {
                    digest,
                    executable,
                    mode,
                    ..
                } => files.push(RemoteFileNode {
                    digest: digest.clone(),
                    executable: *executable,
                    mode: *mode,
                    name,
                }),
                ArchiveNode::Symlink { mode, target } => symlinks.push(RemoteSymlinkNode {
                    mode: *mode,
                    name,
                    target: target
                        .to_str()
                        .ok_or_else(|| eyre!("remote cache symlink target must be valid UTF-8"))?
                        .to_string(),
                }),
            }
        }
        let directory = serde_json::to_value(RemoteDirectory {
            directories,
            files,
            symlinks,
            version: 1,
        })?;
        let bytes = canonical_json(&directory)?;
        let digest = CacheDigest::blake3(&bytes);
        directory_uploads.push(CasBlobUpload {
            digest: digest.clone(),
            source: CasBlobSource::Bytes(bytes),
        });
        Ok(digest)
    }

    let mut uploads = Vec::new();
    let root = build_directory(Path::new(""), &nodes, &mut uploads)?;
    for node in nodes.into_values() {
        if let ArchiveNode::File { digest, file, .. } = node {
            uploads.push(CasBlobUpload {
                digest,
                source: CasBlobSource::File(file),
            });
        }
    }
    Ok((root, uploads))
}

enum RestoredNode {
    Directory {
        mode: u32,
    },
    File {
        digest: CacheDigest,
        executable: bool,
        mode: u32,
    },
    Symlink {
        mode: u32,
        target: PathBuf,
    },
}

async fn materialize_remote_tree(
    store: &HttpTaskCacheStore,
    root: &CacheDigest,
) -> Result<tempfile::NamedTempFile> {
    let mut pending = vec![(PathBuf::new(), root.clone(), BTreeSet::new())];
    let mut nodes = BTreeMap::<PathBuf, RestoredNode>::new();
    while let Some((path, digest, mut ancestors)) = pending.pop() {
        if !ancestors.insert(digest.clone()) {
            bail!("remote cache directory graph contains a cycle");
        }
        let bytes = store
            .get_blob(&digest, REMOTE_CACHE_DIRECTORY_MEDIA_TYPE)
            .await?;
        let directory: RemoteDirectory = serde_json::from_slice(&bytes)?;
        if canonical_json(&serde_json::to_value(&directory)?)? != bytes {
            bail!("remote cache directory is not canonical JSON");
        }
        if directory.version != 1 {
            bail!("unsupported remote cache directory version");
        }
        let mut names = BTreeSet::new();
        for directory in directory.directories {
            validate_cache_name(&directory.name)?;
            if !names.insert(directory.name.clone()) {
                bail!("remote cache directory contains duplicate names");
            }
            let child = path.join(&directory.name);
            validate_cache_path(&child)?;
            nodes.insert(
                child.clone(),
                RestoredNode::Directory {
                    mode: directory.mode,
                },
            );
            pending.push((child, directory.digest, ancestors.clone()));
        }
        for file in directory.files {
            validate_cache_name(&file.name)?;
            if !names.insert(file.name.clone()) {
                bail!("remote cache directory contains duplicate names");
            }
            let child = path.join(&file.name);
            validate_cache_path(&child)?;
            nodes.insert(
                child,
                RestoredNode::File {
                    digest: file.digest,
                    executable: file.executable,
                    mode: file.mode,
                },
            );
        }
        for symlink in directory.symlinks {
            validate_cache_name(&symlink.name)?;
            if !names.insert(symlink.name.clone()) {
                bail!("remote cache directory contains duplicate names");
            }
            let child = path.join(&symlink.name);
            validate_cache_path(&child)?;
            let target = PathBuf::from(symlink.target);
            validate_cache_symlink_target(&child, &target)?;
            nodes.insert(
                child,
                RestoredNode::Symlink {
                    mode: symlink.mode,
                    target,
                },
            );
        }
    }

    file::create_dir_all(&store.staging_dir)?;
    let mut downloaded = BTreeMap::new();
    for (path, node) in &nodes {
        if let RestoredNode::File { digest, .. } = node {
            downloaded.insert(path.clone(), store.get_blob_file(digest).await?);
        }
    }

    let archive_file = tempfile::NamedTempFile::new_in(&store.staging_dir)?;
    let encoder = zstd::Encoder::new(archive_file.reopen()?, 0)?;
    let mut archive = Builder::new(encoder);
    for (path, node) in nodes {
        let (entry_type, mode) = match &node {
            RestoredNode::Directory { mode } => (EntryType::Directory, *mode),
            RestoredNode::File {
                executable, mode, ..
            } => {
                let mode = if *executable {
                    *mode | 0o111
                } else {
                    *mode & !0o111
                };
                (EntryType::File, mode)
            }
            RestoredNode::Symlink { mode, .. } => (EntryType::Symlink, *mode),
        };
        let mut header = Header::new_gnu(entry_type);
        header.set_mode(mode);
        header.set_mtime(0);
        match node {
            RestoredNode::Directory { .. } => {
                header.set_size(0);
                archive.append_data(&mut header, path, std::io::empty())?;
            }
            RestoredNode::File { digest, .. } => {
                header.set_size(digest.size);
                let file = downloaded
                    .get(&path)
                    .ok_or_else(|| eyre!("remote cache file was not downloaded"))?;
                archive.append_data(&mut header, path, File::open(file.path())?)?;
            }
            RestoredNode::Symlink { target, .. } => {
                header.set_size(0);
                archive.append_link(&mut header, path, target)?;
            }
        }
    }
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(archive_file)
}

fn normalized_base_url(mut url: Url) -> Url {
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url
}

fn validate_remote_key(key: &str) -> Result<()> {
    if key.len() != 64
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("invalid remote task cache key");
    }
    Ok(())
}

pub(crate) struct LocalTaskCacheStore {
    root: PathBuf,
}

impl LocalTaskCacheStore {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn paths(&self, key: &str) -> (PathBuf, PathBuf) {
        (
            self.root.join(format!("{key}.tar.zst")),
            self.root.join(format!("{key}.json")),
        )
    }
}

#[async_trait]
impl TaskCacheStore for LocalTaskCacheStore {
    fn version(&self) -> u8 {
        TASK_CACHE_STORE_VERSION
    }

    async fn get(&self, key: &str, _action_size: u64) -> Result<Option<TaskCacheStoreEntry>> {
        let (artifact_path, manifest_path) = self.paths(key);
        let manifest = match fs::read(&manifest_path) {
            Ok(manifest) => manifest,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        Ok(Some(TaskCacheStoreEntry {
            manifest,
            artifact: artifact_path
                .is_file()
                .then(|| TaskCacheStoreArtifact::stored(artifact_path)),
        }))
    }

    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
        file::create_dir_all(&self.root)?;
        let nonce = crate::rand::random_string(8);
        Ok(TaskCacheStoreWrite {
            artifact_path: self.root.join(format!("{key}.part-{nonce}.tar.zst")),
            manifest_path: self.root.join(format!("{key}.part-{nonce}.json")),
        })
    }

    async fn commit(
        &self,
        key: &str,
        _action: &[u8],
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        let (artifact_path, manifest_path) = self.paths(key);
        fs::write(&write.manifest_path, manifest)?;
        file::rename(&write.manifest_path, &manifest_path)?;
        if has_artifact {
            file::rename(&write.artifact_path, &artifact_path)?;
        } else {
            remove_file(&artifact_path)?;
        }
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<()> {
        let (artifact_path, manifest_path) = self.paths(key);
        remove_file(&artifact_path)?;
        remove_file(&manifest_path)
    }

    fn touch(&self, key: &str) {
        let (artifact_path, manifest_path) = self.paths(key);
        for (kind, path) in [("archive", artifact_path), ("manifest", manifest_path)] {
            if path.is_file()
                && let Err(err) = file::touch_file(&path)
            {
                warn!("failed to update task cache {kind} access time: {err}");
            }
        }
    }
}

fn remove_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    struct FailingTaskCacheStore {
        inner: LocalTaskCacheStore,
    }

    struct RemoveFailingTaskCacheStore {
        inner: LocalTaskCacheStore,
    }

    #[async_trait]
    impl TaskCacheStore for RemoveFailingTaskCacheStore {
        fn version(&self) -> u8 {
            self.inner.version()
        }

        async fn get(&self, key: &str, action_size: u64) -> Result<Option<TaskCacheStoreEntry>> {
            self.inner.get(key, action_size).await
        }

        fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
            self.inner.begin_write(key)
        }

        async fn commit(
            &self,
            key: &str,
            action: &[u8],
            write: &TaskCacheStoreWrite,
            manifest: &[u8],
            has_artifact: bool,
        ) -> Result<()> {
            self.inner
                .commit(key, action, write, manifest, has_artifact)
                .await
        }

        async fn remove(&self, _key: &str) -> Result<()> {
            bail!("local remove failed")
        }

        fn touch(&self, key: &str) {
            self.inner.touch(key);
        }
    }

    #[async_trait]
    impl TaskCacheStore for FailingTaskCacheStore {
        fn version(&self) -> u8 {
            self.inner.version()
        }

        async fn get(&self, _key: &str, _action_size: u64) -> Result<Option<TaskCacheStoreEntry>> {
            bail!("remote get failed")
        }

        fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
            self.inner.begin_write(key)
        }

        async fn commit(
            &self,
            _key: &str,
            _action: &[u8],
            _write: &TaskCacheStoreWrite,
            _manifest: &[u8],
            _has_artifact: bool,
        ) -> Result<()> {
            bail!("remote commit failed")
        }

        async fn remove(&self, _key: &str) -> Result<()> {
            bail!("remote remove failed")
        }

        fn touch(&self, key: &str) {
            self.inner.touch(key);
        }
    }

    #[tokio::test]
    async fn local_store_round_trips_result_and_artifact_entries() {
        let root = tempfile::tempdir().unwrap();
        let store = LocalTaskCacheStore::new(root.path().to_path_buf());
        assert_eq!(store.version(), TASK_CACHE_STORE_VERSION);

        let write = store.begin_write("result").unwrap();
        store
            .commit("result", b"action", &write, b"manifest", false)
            .await
            .unwrap();
        let entry = store.get("result", 6).await.unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest");
        assert!(entry.artifact.is_none());

        let write = store.begin_write("artifact").unwrap();
        fs::write(write.artifact_path(), b"archive").unwrap();
        store
            .commit("artifact", b"action", &write, b"manifest-2", true)
            .await
            .unwrap();
        let entry = store.get("artifact", 6).await.unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest-2");
        assert_eq!(
            fs::read(entry.artifact.unwrap().path()).unwrap(),
            b"archive"
        );

        store.remove("artifact").await.unwrap();
        assert!(store.get("artifact", 6).await.unwrap().is_none());
    }

    async fn seed(store: &dyn TaskCacheStore, key: &str, manifest: &[u8], artifact: Option<&[u8]>) {
        let write = store.begin_write(key).unwrap();
        if let Some(artifact) = artifact {
            fs::write(write.artifact_path(), artifact).unwrap();
        }
        store
            .commit(key, b"action", &write, manifest, artifact.is_some())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn composite_store_promotes_remote_hits_and_mirrors_writes() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(remote_root.path().to_path_buf()));
        seed(remote.as_ref(), "remote-hit", b"remote", Some(b"artifact")).await;
        let composite = CompositeTaskCacheStore::new(local.clone(), remote.clone()).unwrap();

        let hit = composite.get("remote-hit", 6).await.unwrap().unwrap();
        assert_eq!(hit.manifest, b"remote");
        assert_eq!(fs::read(hit.artifact.unwrap().path()).unwrap(), b"artifact");
        assert!(local.get("remote-hit", 6).await.unwrap().is_some());

        seed(&composite, "mirrored", b"manifest", Some(b"output")).await;
        assert_eq!(
            local.get("mirrored", 6).await.unwrap().unwrap().manifest,
            b"manifest"
        );
        let mirrored = remote.get("mirrored", 6).await.unwrap().unwrap();
        assert_eq!(mirrored.manifest, b"manifest");
        assert_eq!(
            fs::read(mirrored.artifact.unwrap().path()).unwrap(),
            b"output"
        );

        seed(&composite, "result-only", b"result", None).await;
        assert!(
            remote
                .get("result-only", 6)
                .await
                .unwrap()
                .unwrap()
                .artifact
                .is_none()
        );
    }

    #[tokio::test]
    async fn composite_store_keeps_local_entries_when_remote_operations_fail() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote_inner = LocalTaskCacheStore::new(remote_root.path().to_path_buf());
        seed(&remote_inner, "remove", b"remote", None).await;
        let remote: Arc<dyn TaskCacheStore> = Arc::new(FailingTaskCacheStore {
            inner: remote_inner,
        });
        let composite = CompositeTaskCacheStore::new(local.clone(), remote).unwrap();

        assert!(composite.get("unavailable", 6).await.unwrap().is_none());

        seed(&composite, "commit", b"local", None).await;
        assert_eq!(
            local.get("commit", 6).await.unwrap().unwrap().manifest,
            b"local"
        );

        seed(local.as_ref(), "remove", b"local", None).await;
        assert!(composite.remove("remove").await.is_err());
        assert!(local.get("remove", 6).await.unwrap().is_some());
    }

    #[test]
    fn archive_is_split_into_directory_and_file_cas_objects() {
        let staging = tempfile::tempdir().unwrap();
        let archive_path = staging.path().join("output.tar.zst");
        let encoder = zstd::Encoder::new(File::create(&archive_path).unwrap(), 0).unwrap();
        let mut archive = Builder::new(encoder);
        let mut directory_header = Header::new_gnu(EntryType::Directory);
        directory_header.set_mode(0o755);
        directory_header.set_size(0);
        archive
            .append_data(&mut directory_header, "dist", std::io::empty())
            .unwrap();
        let mut file_header = Header::new_gnu(EntryType::File);
        file_header.set_mode(0o755);
        file_header.set_size(5);
        archive
            .append_data(&mut file_header, "dist/app", b"hello".as_slice())
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();

        let (root, uploads) = archive_to_cas(&archive_path, staging.path()).unwrap();
        let root_bytes = uploads
            .iter()
            .find_map(|upload| {
                (upload.digest == root).then(|| match &upload.source {
                    CasBlobSource::Bytes(bytes) => bytes.as_slice(),
                    CasBlobSource::File(_) => panic!("directory object must be in memory"),
                })
            })
            .unwrap();
        let root_directory: RemoteDirectory = serde_json::from_slice(root_bytes).unwrap();
        assert_eq!(root_directory.directories.len(), 1);
        assert_eq!(root_directory.directories[0].name, "dist");
        let dist_digest = &root_directory.directories[0].digest;
        let dist_bytes = uploads
            .iter()
            .find_map(|upload| {
                (&upload.digest == dist_digest).then(|| match &upload.source {
                    CasBlobSource::Bytes(bytes) => bytes.as_slice(),
                    CasBlobSource::File(_) => panic!("directory object must be in memory"),
                })
            })
            .unwrap();
        let dist: RemoteDirectory = serde_json::from_slice(dist_bytes).unwrap();
        assert_eq!(dist.files.len(), 1);
        assert_eq!(dist.files[0].name, "app");
        assert_eq!(dist.files[0].digest, CacheDigest::blake3(b"hello"));
        assert!(dist.files[0].executable);
    }

    #[test]
    fn cache_symlink_targets_remain_within_output_root() {
        assert!(
            validate_cache_symlink_target(Path::new("dist/link"), Path::new("../artifact")).is_ok()
        );
        assert!(
            validate_cache_symlink_target(Path::new("dist/link"), Path::new("../../outside"))
                .is_err()
        );
        assert!(validate_cache_symlink_target(Path::new("link"), Path::new("..")).is_err());
        assert!(validate_cache_symlink_target(Path::new("link"), Path::new("/outside")).is_err());
    }

    #[tokio::test]
    async fn http_store_publishes_cas_before_action_result_and_reads_it_back() {
        let mut server = mockito::Server::new_async().await;
        let action = br#"{"task":"build"}"#;
        let action_digest = CacheDigest::blake3(action);
        let key = action_digest.hash.as_str();
        let manifest = format!(
            r#"{{"format":2,"key":"{key}","task_identity":"build","artifact_checksum":null,"roots":[],"output":[],"restored_bytes":0,"execution_duration_ns":1}}"#
        );
        let local_manifest: CacheManifest = serde_json::from_str(&manifest).unwrap();
        let metadata = canonical_json(
            &serde_json::to_value(RemoteClientMetadata::from_manifest(&local_manifest)).unwrap(),
        )
        .unwrap();
        let metadata_digest = CacheDigest::blake3(&metadata);
        let envelope = RemoteActionResultEnvelope {
            result: RemoteActionResult {
                action: action_digest.clone(),
                metadata: Some(metadata_digest.clone()),
                output_root: None,
                version: 1,
            },
            signatures: Vec::new(),
        };
        let action_path = format!(
            "/v1/blobs/blake3/{}/{}",
            action_digest.hash, action_digest.size
        );
        let metadata_path = format!(
            "/v1/blobs/blake3/{}/{}",
            metadata_digest.hash, metadata_digest.size
        );
        let result_path = format!(
            "/v1/action-results/blake3/{}/{}",
            action_digest.hash, action_digest.size
        );
        let action_put = server
            .mock("PUT", action_path.as_str())
            .match_header("mise-cache-protocol", "1")
            .match_header("mise-cache-namespace", "test-namespace")
            .match_header("if-none-match", "*")
            .match_body(action.to_vec())
            .with_status(201)
            .expect(1)
            .create_async()
            .await;
        let metadata_put = server
            .mock("PUT", metadata_path.as_str())
            .match_header("if-none-match", "*")
            .match_body(metadata.clone())
            .with_status(201)
            .expect(1)
            .create_async()
            .await;
        let result_body = serde_json::to_vec(&envelope).unwrap();
        let result_put = server
            .mock("PUT", result_path.as_str())
            .match_header("content-type", REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE)
            .match_header("if-none-match", "*")
            .match_body(result_body.clone())
            .with_status(201)
            .expect(1)
            .create_async()
            .await;
        let result_get = server
            .mock("GET", result_path.as_str())
            .match_header("accept", REMOTE_CACHE_ACTION_RESULT_MEDIA_TYPE)
            .with_status(200)
            .with_body(result_body)
            .expect(1)
            .create_async()
            .await;
        let metadata_get = server
            .mock("GET", metadata_path.as_str())
            .with_status(200)
            .with_body(metadata)
            .expect(1)
            .create_async()
            .await;
        let staging = tempfile::tempdir().unwrap();
        let store = HttpTaskCacheStore {
            base_url: normalized_base_url(server.url().parse().unwrap()),
            namespace: "test-namespace".into(),
            staging_dir: staging.path().to_path_buf(),
            client: reqwest::Client::new(),
        };
        let write = store.begin_write(key).unwrap();

        store
            .commit(key, action, &write, manifest.as_bytes(), false)
            .await
            .unwrap();
        let entry = store.get(key, action.len() as u64).await.unwrap().unwrap();
        let restored: CacheManifest = serde_json::from_slice(&entry.manifest).unwrap();
        assert_eq!(restored.key, key);
        assert!(restored.roots.is_empty());
        assert!(restored.artifact_checksum.is_some());
        assert!(entry.artifact.is_none());
        action_put.assert_async().await;
        metadata_put.assert_async().await;
        result_put.assert_async().await;
        result_get.assert_async().await;
        metadata_get.assert_async().await;
    }

    #[tokio::test]
    async fn composite_store_local_removal_preserves_and_repromotes_remote_entry() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(remote_root.path().to_path_buf()));
        seed(local.as_ref(), "expired", b"local", None).await;
        seed(remote.as_ref(), "expired", b"remote", None).await;
        let composite = CompositeTaskCacheStore::new(local.clone(), remote.clone()).unwrap();

        composite.remove_local("expired", 6).await.unwrap();

        assert!(local.get("expired", 6).await.unwrap().is_none());
        assert!(remote.get("expired", 6).await.unwrap().is_some());
        assert_eq!(
            composite.get("expired", 6).await.unwrap().unwrap().manifest,
            b"remote"
        );
        assert!(local.get("expired", 6).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn composite_store_repairs_failed_local_removal_from_remote() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local_inner = LocalTaskCacheStore::new(local_root.path().to_path_buf());
        seed(&local_inner, "expired", b"stale-local", None).await;
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(RemoveFailingTaskCacheStore { inner: local_inner });
        let remote: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(remote_root.path().to_path_buf()));
        seed(remote.as_ref(), "expired", b"fresh-remote", None).await;
        let composite = CompositeTaskCacheStore::new(local.clone(), remote).unwrap();

        composite.remove_local("expired", 6).await.unwrap();

        assert_eq!(
            local.get("expired", 6).await.unwrap().unwrap().manifest,
            b"fresh-remote"
        );
    }
}
