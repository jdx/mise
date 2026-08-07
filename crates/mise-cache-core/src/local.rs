use crate::CacheDigest;
use eyre::{Result, bail};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct LocalCas {
    root: PathBuf,
}

impl LocalCas {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, digest: &CacheDigest) -> Result<PathBuf> {
        digest.validate()?;
        Ok(self
            .root
            .join("cas/v1")
            .join(&digest.algorithm)
            .join(&digest.hash[..2])
            .join(format!("{}-{}", digest.hash, digest.size)))
    }

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

    pub fn store_bytes(&self, digest: &CacheDigest, bytes: &[u8]) -> Result<PathBuf> {
        if !digest.matches_bytes(bytes)? {
            bail!("bytes do not match the declared CAS digest");
        }
        self.store_with(digest, |temporary| {
            temporary.write_all(bytes)?;
            Ok(())
        })
    }

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
}
