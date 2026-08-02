use crate::file;
use eyre::{Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Version of the cache-store contract. This is independent of the artifact
/// manifest format so stores and transports can evolve without changing keys.
pub(crate) const TASK_CACHE_STORE_VERSION: u8 = 1;

pub(crate) struct TaskCacheStoreEntry {
    pub(crate) manifest: Vec<u8>,
    pub(crate) artifact_path: Option<PathBuf>,
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
pub(crate) trait TaskCacheStore: Send + Sync {
    fn version(&self) -> u8;
    fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>>;
    fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite>;
    fn commit(
        &self,
        key: &str,
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()>;
    fn remove(&self, key: &str) -> Result<()>;
    fn remove_local(&self, key: &str) -> Result<()> {
        self.remove(key)
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

    fn promote(&self, key: &str, entry: &TaskCacheStoreEntry) -> Result<TaskCacheStoreEntry> {
        // A remote lookup is an access, so promotion intentionally starts a
        // fresh local inactivity window for task.cache_max_age.
        let write = self.local.begin_write(key)?;
        let has_artifact = Self::copy_artifact(entry.artifact_path.as_deref(), &write)?;
        self.local
            .commit(key, &write, &entry.manifest, has_artifact)?;
        self.local
            .get(key)?
            .ok_or_else(|| eyre::eyre!("promoted task cache entry disappeared"))
    }
}

impl TaskCacheStore for CompositeTaskCacheStore {
    fn version(&self) -> u8 {
        self.local.version()
    }

    fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
        if let Some(entry) = self.local.get(key)? {
            return Ok(Some(entry));
        }
        let Some(entry) = self.remote.get(key)? else {
            return Ok(None);
        };
        match self.promote(key, &entry) {
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

    fn commit(
        &self,
        key: &str,
        write: &TaskCacheStoreWrite,
        manifest: &[u8],
        has_artifact: bool,
    ) -> Result<()> {
        self.local.commit(key, write, manifest, has_artifact)?;
        let mirror = || -> Result<()> {
            let entry = self
                .local
                .get(key)?
                .ok_or_else(|| eyre::eyre!("published local task cache entry disappeared"))?;
            let remote_write = self.remote.begin_write(key)?;
            let remote_has_artifact =
                Self::copy_artifact(entry.artifact_path.as_deref(), &remote_write)?;
            self.remote
                .commit(key, &remote_write, &entry.manifest, remote_has_artifact)
        };
        if let Err(err) = mirror() {
            warn!("failed to mirror task cache entry {key} remotely: {err}");
        }
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<()> {
        // Delete remotely first so a failed remote delete cannot allow the
        // entry to be promoted back after its local copy was removed.
        self.remote.remove(key)?;
        self.local.remove(key)
    }

    fn remove_local(&self, key: &str) -> Result<()> {
        self.local.remove_local(key)
    }

    fn touch(&self, key: &str) {
        self.local.touch(key);
        self.remote.touch(key);
    }
}

pub(crate) fn compose_task_cache_stores(
    local: Arc<dyn TaskCacheStore>,
    remote: Option<Arc<dyn TaskCacheStore>>,
) -> Result<Arc<dyn TaskCacheStore>> {
    match remote {
        Some(remote) => Ok(Arc::new(CompositeTaskCacheStore::new(local, remote)?)),
        None => Ok(local),
    }
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

impl TaskCacheStore for LocalTaskCacheStore {
    fn version(&self) -> u8 {
        TASK_CACHE_STORE_VERSION
    }

    fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
        let (artifact_path, manifest_path) = self.paths(key);
        let manifest = match fs::read(&manifest_path) {
            Ok(manifest) => manifest,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        Ok(Some(TaskCacheStoreEntry {
            manifest,
            artifact_path: artifact_path.is_file().then_some(artifact_path),
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

    fn commit(
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

    fn remove(&self, key: &str) -> Result<()> {
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

    impl TaskCacheStore for FailingTaskCacheStore {
        fn version(&self) -> u8 {
            self.inner.version()
        }

        fn get(&self, key: &str) -> Result<Option<TaskCacheStoreEntry>> {
            self.inner.get(key)
        }

        fn begin_write(&self, key: &str) -> Result<TaskCacheStoreWrite> {
            self.inner.begin_write(key)
        }

        fn commit(
            &self,
            _key: &str,
            _write: &TaskCacheStoreWrite,
            _manifest: &[u8],
            _has_artifact: bool,
        ) -> Result<()> {
            bail!("remote commit failed")
        }

        fn remove(&self, _key: &str) -> Result<()> {
            bail!("remote remove failed")
        }

        fn touch(&self, key: &str) {
            self.inner.touch(key);
        }
    }

    #[test]
    fn local_store_round_trips_result_and_artifact_entries() {
        let root = tempfile::tempdir().unwrap();
        let store = LocalTaskCacheStore::new(root.path().to_path_buf());
        assert_eq!(store.version(), TASK_CACHE_STORE_VERSION);

        let write = store.begin_write("result").unwrap();
        store.commit("result", &write, b"manifest", false).unwrap();
        let entry = store.get("result").unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest");
        assert!(entry.artifact_path.is_none());

        let write = store.begin_write("artifact").unwrap();
        fs::write(write.artifact_path(), b"archive").unwrap();
        store
            .commit("artifact", &write, b"manifest-2", true)
            .unwrap();
        let entry = store.get("artifact").unwrap().unwrap();
        assert_eq!(entry.manifest, b"manifest-2");
        assert_eq!(fs::read(entry.artifact_path.unwrap()).unwrap(), b"archive");

        store.remove("artifact").unwrap();
        assert!(store.get("artifact").unwrap().is_none());
    }

    fn seed(store: &dyn TaskCacheStore, key: &str, manifest: &[u8], artifact: Option<&[u8]>) {
        let write = store.begin_write(key).unwrap();
        if let Some(artifact) = artifact {
            fs::write(write.artifact_path(), artifact).unwrap();
        }
        store
            .commit(key, &write, manifest, artifact.is_some())
            .unwrap();
    }

    #[test]
    fn composite_store_promotes_remote_hits_and_mirrors_writes() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(remote_root.path().to_path_buf()));
        seed(remote.as_ref(), "remote-hit", b"remote", Some(b"artifact"));
        let composite = CompositeTaskCacheStore::new(local.clone(), remote.clone()).unwrap();

        let hit = composite.get("remote-hit").unwrap().unwrap();
        assert_eq!(hit.manifest, b"remote");
        assert_eq!(fs::read(hit.artifact_path.unwrap()).unwrap(), b"artifact");
        assert!(local.get("remote-hit").unwrap().is_some());

        seed(&composite, "mirrored", b"manifest", Some(b"output"));
        assert_eq!(
            local.get("mirrored").unwrap().unwrap().manifest,
            b"manifest"
        );
        let mirrored = remote.get("mirrored").unwrap().unwrap();
        assert_eq!(mirrored.manifest, b"manifest");
        assert_eq!(
            fs::read(mirrored.artifact_path.unwrap()).unwrap(),
            b"output"
        );

        seed(&composite, "result-only", b"result", None);
        assert!(
            remote
                .get("result-only")
                .unwrap()
                .unwrap()
                .artifact_path
                .is_none()
        );
    }

    #[test]
    fn composite_store_keeps_local_entries_when_remote_operations_fail() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote_inner = LocalTaskCacheStore::new(remote_root.path().to_path_buf());
        seed(&remote_inner, "remove", b"remote", None);
        let remote: Arc<dyn TaskCacheStore> = Arc::new(FailingTaskCacheStore {
            inner: remote_inner,
        });
        let composite = CompositeTaskCacheStore::new(local.clone(), remote).unwrap();

        seed(&composite, "commit", b"local", None);
        assert_eq!(local.get("commit").unwrap().unwrap().manifest, b"local");

        seed(local.as_ref(), "remove", b"local", None);
        assert!(composite.remove("remove").is_err());
        assert!(local.get("remove").unwrap().is_some());
    }

    #[test]
    fn composite_store_local_removal_preserves_and_repromotes_remote_entry() {
        let local_root = tempfile::tempdir().unwrap();
        let remote_root = tempfile::tempdir().unwrap();
        let local: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(local_root.path().to_path_buf()));
        let remote: Arc<dyn TaskCacheStore> =
            Arc::new(LocalTaskCacheStore::new(remote_root.path().to_path_buf()));
        seed(local.as_ref(), "expired", b"local", None);
        seed(remote.as_ref(), "expired", b"remote", None);
        let composite = CompositeTaskCacheStore::new(local.clone(), remote.clone()).unwrap();

        composite.remove_local("expired").unwrap();

        assert!(local.get("expired").unwrap().is_none());
        assert!(remote.get("expired").unwrap().is_some());
        assert_eq!(
            composite.get("expired").unwrap().unwrap().manifest,
            b"remote"
        );
        assert!(local.get("expired").unwrap().is_some());
    }
}
