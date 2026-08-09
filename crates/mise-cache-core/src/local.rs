use crate::{CacheDigest, RemoteActionResult, canonical_json};
use eyre::{Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A validated, content-addressed store on the local filesystem.
#[derive(Debug, Clone)]
pub struct LocalCas {
    root: PathBuf,
}

/// A local index from action digests to their referenced cache objects.
#[derive(Debug, Clone)]
pub struct LocalActionCache {
    root: PathBuf,
    cas: LocalCas,
}

impl LocalCas {
    /// Create a local content-addressed store beneath `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the root shared by this store and its action-result index.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve the storage path for a validated digest.
    pub fn path_for(&self, digest: &CacheDigest) -> Result<PathBuf> {
        digest.validate()?;
        Ok(self
            .root
            .join("cas/v1")
            .join(&digest.algorithm)
            .join(&digest.hash[..2])
            .join(format!("{}-{}", digest.hash, digest.size)))
    }

    /// Find and verify a stored object.
    pub fn find(&self, digest: &CacheDigest) -> Result<Option<PathBuf>> {
        let path = self.path_for(digest)?;
        if !path.exists() {
            return Ok(None);
        }
        if !digest.matches_file(&path)? {
            bail!(
                "local CAS blob failed digest verification: {}",
                path.display()
            );
        }
        Ok(Some(path))
    }

    /// Atomically store bytes after verifying their declared digest.
    pub fn store_bytes(&self, digest: &CacheDigest, bytes: &[u8]) -> Result<PathBuf> {
        if !digest.matches_bytes(bytes)? {
            bail!("bytes do not match the declared CAS digest");
        }
        self.store_with(digest, |temporary| {
            temporary.write_all(bytes)?;
            Ok(())
        })
    }

    /// Atomically store a file after verifying its declared digest.
    pub fn store_file(&self, digest: &CacheDigest, source: &Path) -> Result<PathBuf> {
        if !digest.matches_file(source)? {
            bail!(
                "file does not match the declared CAS digest: {}",
                source.display()
            );
        }
        self.store_with(digest, |temporary| {
            fs::copy(source, temporary.path())?;
            Ok(())
        })
    }

    fn store_with(
        &self,
        digest: &CacheDigest,
        write: impl FnOnce(&mut tempfile::NamedTempFile) -> Result<()>,
    ) -> Result<PathBuf> {
        let destination = self.path_for(digest)?;
        if let Some(existing) = self.find(digest)? {
            return Ok(existing);
        }
        let parent = destination.parent().expect("CAS path has a parent");
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        write(&mut temporary)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        if !digest.matches_file(temporary.path())? {
            bail!("staged blob does not match the declared CAS digest");
        }
        match temporary.persist_noclobber(&destination) {
            Ok(_) => Ok(destination),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => self
                .find(digest)?
                .ok_or_else(|| eyre::eyre!("concurrent CAS write did not publish a valid blob")),
            Err(error) => Err(error.error.into()),
        }
    }
}

impl LocalActionCache {
    /// Create an action-result index beneath `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            cas: LocalCas::new(root.clone()),
            root,
        }
    }

    /// Resolve the storage path for an action digest.
    pub fn path_for(&self, action: &CacheDigest) -> Result<PathBuf> {
        action.validate()?;
        if action.algorithm != "blake3" {
            bail!("local action keys must use blake3");
        }
        Ok(self
            .root
            .join("action-results/v1")
            .join(&action.algorithm)
            .join(&action.hash[..2])
            .join(format!("{}-{}.json", action.hash, action.size)))
    }

    /// Find and strictly validate a canonical action result.
    pub fn find(&self, action: &CacheDigest) -> Result<Option<RemoteActionResult>> {
        let path = self.path_for(action)?;
        if !path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&path)?;
        let result: RemoteActionResult = serde_json::from_slice(&bytes)?;
        if result.version != 1 || result.action != *action || canonical_json(&result)? != bytes {
            bail!("local action result is invalid: {}", path.display());
        }
        Ok(Some(result))
    }

    /// Atomically publish an action result after validating all referenced objects.
    pub fn store(&self, result: &RemoteActionResult) -> Result<PathBuf> {
        if result.version != 1 {
            bail!("unsupported local action result version");
        }
        for digest in [
            Some(&result.action),
            result.metadata.as_ref(),
            result.output_root.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if self.cas.find(digest)?.is_none() {
                bail!("cannot publish an action result with a missing blob");
            }
        }
        let destination = self.path_for(&result.action)?;
        let replace_invalid = match self.find(&result.action) {
            Ok(Some(existing)) => {
                if existing == *result {
                    return Ok(destination);
                }
                bail!("local action key already has a different result");
            }
            Ok(None) => false,
            Err(_) => true,
        };
        let parent = destination
            .parent()
            .expect("action-result path has a parent");
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&canonical_json(result)?)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        if replace_invalid {
            temporary
                .persist(&destination)
                .map_err(|error| error.error)?;
            return Ok(destination);
        }
        match temporary.persist_noclobber(&destination) {
            Ok(_) => Ok(destination),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => self
                .find(&result.action)?
                .filter(|existing| existing == result)
                .map(|_| destination)
                .ok_or_else(|| eyre::eyre!("concurrent action write was invalid or conflicting")),
            Err(error) => Err(error.error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_and_validates_blobs_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let cas = LocalCas::new(directory.path());
        let digest = CacheDigest::blake3(b"cached object");

        let path = cas.store_bytes(&digest, b"cached object").unwrap();
        assert_eq!(cas.find(&digest).unwrap(), Some(path.clone()));
        assert_eq!(fs::read(&path).unwrap(), b"cached object");
        assert_eq!(cas.store_bytes(&digest, b"cached object").unwrap(), path);
        assert!(cas.store_bytes(&digest, b"other object").is_err());
    }

    #[test]
    fn rejects_corrupt_existing_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let cas = LocalCas::new(directory.path());
        let digest = CacheDigest::blake3(b"cached object");
        let path = cas.store_bytes(&digest, b"cached object").unwrap();
        fs::write(path, b"corrupt").unwrap();

        assert!(cas.find(&digest).is_err());
    }

    #[test]
    fn publishes_action_results_after_referenced_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let cas = LocalCas::new(directory.path());
        let actions = LocalActionCache::new(directory.path());
        let action = CacheDigest::blake3(b"action");
        let metadata = CacheDigest::blake3(b"metadata");
        let output_root = CacheDigest::blake3(b"directory");
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: Some(metadata.clone()),
            output_root: Some(output_root.clone()),
            version: 1,
        };

        assert!(actions.store(&result).is_err());
        cas.store_bytes(&action, b"action").unwrap();
        cas.store_bytes(&metadata, b"metadata").unwrap();
        cas.store_bytes(&output_root, b"directory").unwrap();
        actions.store(&result).unwrap();
        assert_eq!(actions.find(&action).unwrap(), Some(result));
    }

    #[test]
    fn atomically_replaces_a_corrupt_action_result() {
        let directory = tempfile::tempdir().unwrap();
        let cas = LocalCas::new(directory.path());
        let actions = LocalActionCache::new(directory.path());
        let action = CacheDigest::blake3(b"action");
        let result = RemoteActionResult {
            action: action.clone(),
            metadata: None,
            output_root: None,
            version: 1,
        };
        cas.store_bytes(&action, b"action").unwrap();
        let path = actions.path_for(&action).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"truncated").unwrap();

        assert!(actions.find(&action).is_err());
        assert_eq!(actions.store(&result).unwrap(), path);
        assert_eq!(actions.find(&action).unwrap(), Some(result));
    }
}
