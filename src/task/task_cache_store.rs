use crate::file;
use async_trait::async_trait;
use eyre::{Result, bail};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use url::Url;

const REMOTE_CACHE_PROTOCOL_VERSION: u8 = 1;
const REMOTE_CACHE_PROTOCOL_HEADER: &str = "Mise-Cache-Protocol";
const REMOTE_CACHE_NAMESPACE_HEADER: &str = "Mise-Cache-Namespace";
const REMOTE_CACHE_MANIFEST_MEDIA_TYPE: &str = "application/vnd.mise.task-cache-manifest.v2+json";
const REMOTE_CACHE_ARTIFACT_MEDIA_TYPE: &str = "application/vnd.mise.task-cache-artifact.v1+zstd";

/// Version of the cache-store contract. This is independent of the artifact
/// manifest format so stores and transports can evolve without changing keys.
pub(crate) const TASK_CACHE_STORE_VERSION: u8 = 1;

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
    async fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>>;
    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite>;
    async fn commit(
        &self,
        key: &str,
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()>;
    async fn remove(&self, key: &str) -> Result<()>;
    async fn remove_local(&self, key: &str) -> Result<()> {
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

    async fn promote(&self, key: &str, entry: &TaskCacheStoreEntry) -> Result<TaskCacheStoreEntry> {
        // A remote lookup is an access, so promotion intentionally starts a
        // fresh local inactivity window for task.cache_max_age.
        let write = self.local.begin_write(key)?;
        let has_artifact = Self::copy_artifact(
            entry.artifact.as_ref().map(TaskCacheStoreArtifact::path),
            &write,
        )?;
        self.local
            .commit(key, &write, &entry.manifest, has_artifact)
            .await?;
        self.local
            .get(key)
            .await?
            .ok_or_else(|| eyre::eyre!("promoted task cache entry disappeared"))
    }
}

#[async_trait]
impl TaskCacheStore for CompositeTaskCacheStore {
    fn version(&self) -> u8 {
        self.local.version()
    }

    async fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
        if let Some(entry) = self.local.get(key).await? {
            return Ok(Some(entry));
        }
        let entry = match self.remote.get(key).await {
            Ok(Some(entry)) => entry,
            Ok(None) => return Ok(None),
            Err(err) => {
                warn!("remote task cache lookup failed for {key}; using local cache only: {err}");
                return Ok(None);
            }
        };
        match self.promote(key, &entry).await {
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
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        self.local
            .commit(key, write, manifest, has_artifact)
            .await?;
        let mirror: Result<()> = async {
            let entry = self
                .local
                .get(key)
                .await?
                .ok_or_else(|| eyre::eyre!("published local task cache entry disappeared"))?;
            let remote_write = self.remote.begin_write(key)?;
            let remote_has_artifact = Self::copy_artifact(
                entry.artifact.as_ref().map(TaskCacheStoreArtifact::path),
                &remote_write,
            )?;
            self.remote
                .commit(key, &remote_write, &entry.manifest, remote_has_artifact)
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

    async fn remove_local(&self, key: &str) -> Result<()> {
        let Err(remove_err) = self.local.remove_local(key).await else {
            return Ok(());
        };
        let Some(entry) = self.remote.get(key).await? else {
            return Err(remove_err);
        };
        self.promote(key, &entry)
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
    fn endpoint(&self, key: &str, artifact: bool) -> Result<Url> {
        validate_remote_key(key)?;
        let suffix = if artifact { "/artifact" } else { "" };
        Ok(self.base_url.join(&format!(
            "v{REMOTE_CACHE_PROTOCOL_VERSION}/cache/{key}{suffix}"
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

    async fn put_artifact(&self, key: &str, path: &Path) -> Result<()> {
        let file = tokio::fs::File::open(path).await?;
        let length = file.metadata().await?.len();
        let stream = tokio_util::io::ReaderStream::new(file);
        let response = self
            .request(
                reqwest::Method::PUT,
                self.endpoint(key, true)?,
                REMOTE_CACHE_ARTIFACT_MEDIA_TYPE,
            )
            .header(CONTENT_TYPE, REMOTE_CACHE_ARTIFACT_MEDIA_TYPE)
            .header(CONTENT_LENGTH, length)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await?;
        response.error_for_status()?;
        Ok(())
    }

    async fn put_manifest(&self, key: &str, manifest: &[u8]) -> Result<()> {
        let response = self
            .request(
                reqwest::Method::PUT,
                self.endpoint(key, false)?,
                REMOTE_CACHE_MANIFEST_MEDIA_TYPE,
            )
            .header(CONTENT_TYPE, REMOTE_CACHE_MANIFEST_MEDIA_TYPE)
            .body(manifest.to_vec())
            .send()
            .await?;
        response.error_for_status()?;
        Ok(())
    }

    async fn get_artifact(&self, key: &str) -> Result<Option<TaskCacheStoreArtifact>> {
        let mut response = self
            .request(
                reqwest::Method::GET,
                self.endpoint(key, true)?,
                REMOTE_CACHE_ARTIFACT_MEDIA_TYPE,
            )
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        response.error_for_status_ref()?;
        file::create_dir_all(&self.staging_dir)?;
        let temporary = tempfile::NamedTempFile::new_in(&self.staging_dir)?;
        let mut output = tokio::fs::File::from_std(temporary.reopen()?);
        while let Some(chunk) = response.chunk().await? {
            output.write_all(&chunk).await?;
        }
        output.flush().await?;
        drop(output);
        Ok(Some(TaskCacheStoreArtifact::temporary(temporary)))
    }

    async fn delete_object(&self, key: &str, artifact: bool) -> Result<()> {
        let media_type = if artifact {
            REMOTE_CACHE_ARTIFACT_MEDIA_TYPE
        } else {
            REMOTE_CACHE_MANIFEST_MEDIA_TYPE
        };
        let response = self
            .request(
                reqwest::Method::DELETE,
                self.endpoint(key, artifact)?,
                media_type,
            )
            .send()
            .await?;
        if response.status() != StatusCode::NOT_FOUND {
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

    async fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
        let response = self
            .request(
                reqwest::Method::GET,
                self.endpoint(key, false)?,
                REMOTE_CACHE_MANIFEST_MEDIA_TYPE,
            )
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let manifest = response.error_for_status()?.bytes().await?.to_vec();
        #[derive(serde::Deserialize)]
        struct ManifestRoots {
            roots: Vec<PathBuf>,
        }
        let has_artifact = match serde_json::from_slice::<ManifestRoots>(&manifest) {
            Ok(manifest) => !manifest.roots.is_empty(),
            Err(err) => {
                warn!("ignoring malformed remote task cache manifest for {key}: {err}");
                return Ok(None);
            }
        };
        let artifact = if has_artifact {
            let Some(artifact) = self.get_artifact(key).await? else {
                return Ok(None);
            };
            Some(artifact)
        } else {
            None
        };
        Ok(Some(TaskCacheStoreEntry { manifest, artifact }))
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
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        if has_artifact {
            self.put_artifact(key, write.artifact_path()).await?;
        }
        self.put_manifest(key, manifest).await
    }

    async fn remove(&self, key: &str) -> Result<()> {
        self.delete_object(key, false).await?;
        self.delete_object(key, true).await
    }

    fn touch(&self, _key: &str) {}
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

    async fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
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

        async fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
            self.inner.get(key).await
        }

        fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
            self.inner.begin_write(key)
        }

        async fn commit(
            &self,
            key: &str,
            write: &TaskCacheStoreWrite,
            manifest: &[u8],
            has_artifact: bool,
        ) -> Result<()> {
            self.inner.commit(key, write, manifest, has_artifact).await
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

        async fn get(&self, _key: &str) -> Result<Option<TaskCacheStoreEntry>> {
            bail!("remote get failed")
        }

        fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
            self.inner.begin_write(key)
        }

        async fn commit(
            &self,
            _key: &str,
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
            .commit("result", &write, b"manifest", false)
            .await
            .unwrap();
        let entry = store.get("result").await.unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest");
        assert!(entry.artifact.is_none());

        let write = store.begin_write("artifact").unwrap();
        fs::write(write.artifact_path(), b"archive").unwrap();
        store
            .commit("artifact", &write, b"manifest-2", true)
            .await
            .unwrap();
        let entry = store.get("artifact").await.unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest-2");
        assert_eq!(
            fs::read(entry.artifact.unwrap().path()).unwrap(),
            b"archive"
        );

        store.remove("artifact").await.unwrap();
        assert!(store.get("artifact").await.unwrap().is_none());
    }

    async fn seed(store: &dyn TaskCacheStore, key: &str, manifest: &[u8], artifact: Option<&[u8]>) {
        let write = store.begin_write(key).unwrap();
        if let Some(artifact) = artifact {
            fs::write(write.artifact_path(), artifact).unwrap();
        }
        store
            .commit(key, &write, manifest, artifact.is_some())
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

        let hit = composite.get("remote-hit").await.unwrap().unwrap();
        assert_eq!(hit.manifest, b"remote");
        assert_eq!(fs::read(hit.artifact.unwrap().path()).unwrap(), b"artifact");
        assert!(local.get("remote-hit").await.unwrap().is_some());

        seed(&composite, "mirrored", b"manifest", Some(b"output")).await;
        assert_eq!(
            local.get("mirrored").await.unwrap().unwrap().manifest,
            b"manifest"
        );
        let mirrored = remote.get("mirrored").await.unwrap().unwrap();
        assert_eq!(mirrored.manifest, b"manifest");
        assert_eq!(
            fs::read(mirrored.artifact.unwrap().path()).unwrap(),
            b"output"
        );

        seed(&composite, "result-only", b"result", None).await;
        assert!(
            remote
                .get("result-only")
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

        assert!(composite.get("unavailable").await.unwrap().is_none());

        seed(&composite, "commit", b"local", None).await;
        assert_eq!(
            local.get("commit").await.unwrap().unwrap().manifest,
            b"local"
        );

        seed(local.as_ref(), "remove", b"local", None).await;
        assert!(composite.remove("remove").await.is_err());
        assert!(local.get("remove").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn http_store_streams_artifact_downloads_to_temporary_files() {
        let mut server = mockito::Server::new_async().await;
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let artifact = "artifact-data\n".repeat(16 * 1024);
        let manifest = format!(r#"{{"format":2,"key":"{key}","roots":["dist"]}}"#);
        let manifest_mock = server
            .mock("GET", format!("/v1/cache/{key}").as_str())
            .match_header("mise-cache-protocol", "1")
            .match_header("mise-cache-namespace", "test-namespace")
            .with_status(200)
            .with_body(manifest.as_bytes())
            .expect(1)
            .create_async()
            .await;
        let artifact_mock = server
            .mock("GET", format!("/v1/cache/{key}/artifact").as_str())
            .match_header("accept", REMOTE_CACHE_ARTIFACT_MEDIA_TYPE)
            .with_status(200)
            .with_body(artifact.as_bytes())
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

        let entry = store.get(key).await.unwrap().unwrap();
        assert_eq!(entry.manifest, manifest.as_bytes());
        let artifact_path = entry.artifact.as_ref().unwrap().path().to_path_buf();
        assert_eq!(fs::read(&artifact_path).unwrap(), artifact.as_bytes());
        drop(entry);
        assert!(!artifact_path.exists());
        manifest_mock.assert_async().await;
        artifact_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_store_streams_artifact_before_publishing_manifest() {
        let mut server = mockito::Server::new_async().await;
        let key = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let artifact = "upload-data\n".repeat(16 * 1024);
        let manifest = br#"{"format":2,"roots":["dist"]}"#;
        let artifact_length = artifact.len().to_string();
        let artifact_mock = server
            .mock("PUT", format!("/v1/cache/{key}/artifact").as_str())
            .match_header("content-type", REMOTE_CACHE_ARTIFACT_MEDIA_TYPE)
            .match_header("content-length", artifact_length.as_str())
            .match_body(artifact.as_str())
            .with_status(201)
            .expect(1)
            .create_async()
            .await;
        let manifest_mock = server
            .mock("PUT", format!("/v1/cache/{key}").as_str())
            .match_header("content-type", REMOTE_CACHE_MANIFEST_MEDIA_TYPE)
            .match_body(manifest.to_vec())
            .with_status(201)
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
        fs::write(write.artifact_path(), artifact.as_bytes()).unwrap();

        store.commit(key, &write, manifest, true).await.unwrap();
        artifact_mock.assert_async().await;
        manifest_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_store_treats_missing_required_artifact_as_a_miss() {
        let mut server = mockito::Server::new_async().await;
        let key = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
        let manifest = format!(r#"{{"format":2,"key":"{key}","roots":["dist"]}}"#);
        let manifest_mock = server
            .mock("GET", format!("/v1/cache/{key}").as_str())
            .with_status(200)
            .with_body(manifest)
            .expect(1)
            .create_async()
            .await;
        let artifact_mock = server
            .mock("GET", format!("/v1/cache/{key}/artifact").as_str())
            .with_status(404)
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

        assert!(store.get(key).await.unwrap().is_none());
        manifest_mock.assert_async().await;
        artifact_mock.assert_async().await;
    }

    #[tokio::test]
    async fn http_store_treats_malformed_manifest_as_a_miss() {
        let mut server = mockito::Server::new_async().await;
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let manifest_mock = server
            .mock("GET", format!("/v1/cache/{key}").as_str())
            .with_status(200)
            .with_body("not-json")
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

        assert!(store.get(key).await.unwrap().is_none());
        manifest_mock.assert_async().await;
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

        composite.remove_local("expired").await.unwrap();

        assert!(local.get("expired").await.unwrap().is_none());
        assert!(remote.get("expired").await.unwrap().is_some());
        assert_eq!(
            composite.get("expired").await.unwrap().unwrap().manifest,
            b"remote"
        );
        assert!(local.get("expired").await.unwrap().is_some());
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

        composite.remove_local("expired").await.unwrap();

        assert_eq!(
            local.get("expired").await.unwrap().unwrap().manifest,
            b"fresh-remote"
        );
    }
}
