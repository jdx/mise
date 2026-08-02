use crate::file;
use eyre::Result;
use std::fs;
use std::path::{Path, PathBuf};

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
    fn touch(&self, key: &str);
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
}
