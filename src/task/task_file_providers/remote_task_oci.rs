use crate::Result;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use walkdir::WalkDir;

use crate::{
    dirs, env,
    file::{self, display_path},
    hash,
    oci::registry,
};

use super::{TaskFileArtifact, TaskFileProvider};

/// Prefix of a `task_config.includes` entry that pulls an OCI artifact.
pub(crate) const OCI_INCLUDE_PREFIX: &str = "oci::";

#[derive(Debug)]
pub(super) struct RemoteTaskOciBuilder {
    store_path: PathBuf,
    use_cache: bool,
}

impl RemoteTaskOciBuilder {
    pub(super) fn new() -> Self {
        Self {
            store_path: env::temp_dir(),
            use_cache: false,
        }
    }

    pub(super) fn with_cache(mut self, use_cache: bool) -> Self {
        if use_cache {
            self.store_path = dirs::CACHE.join("remote-oci-tasks-cache");
            self.use_cache = true;
        }
        self
    }

    pub(super) fn build(self) -> RemoteTaskOci {
        RemoteTaskOci {
            storage_path: self.store_path,
            is_cached: self.use_cache,
        }
    }
}

/// Pulls a task catalog published as an OCI artifact. The unpacked artifact is
/// a directory, loaded like any other task include directory.
#[derive(Debug)]
pub(super) struct RemoteTaskOci {
    storage_path: PathBuf,
    is_cached: bool,
}

fn reference_of(file: &str) -> Result<&str> {
    let reference = file.strip_prefix(OCI_INCLUDE_PREFIX).unwrap_or(file).trim();
    if reference.is_empty() {
        eyre::bail!("OCI task include is missing a reference: {file}");
    }
    Ok(reference)
}

fn starts_with_shebang(path: &Path) -> bool {
    use std::io::Read;
    let mut buf = Vec::with_capacity(2);
    std::fs::File::open(path)
        .and_then(|f| f.take(2).read_to_end(&mut buf))
        .is_ok()
        && buf == b"#!"
}

/// Artifacts don't carry file modes (`oras` and `podman artifact` store bare
/// bytes), so treat any non-TOML file that starts with a shebang as an
/// executable task script.
fn prepare_unpacked_artifact(root: &Path) -> Result<()> {
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_file()
            && path.extension().is_none_or(|ext| ext != "toml")
            && starts_with_shebang(path)
        {
            file::make_executable(path)?;
        }
    }
    Ok(())
}

impl RemoteTaskOci {
    fn get_cache_key(reference: &str) -> String {
        hash::hash_sha256_to_str(reference)
    }

    /// Pull into a fresh staging directory. Returns the staging root and the
    /// unpacked artifact inside it; the caller owns the staging root. The
    /// directory is removed by its guard if the pull fails or is cancelled.
    async fn pull_staged(&self, reference: &str) -> Result<(PathBuf, PathBuf)> {
        file::create_dir_all(&self.storage_path)?;
        let staging = tempfile::Builder::new()
            .prefix(".oci-pull-")
            .tempdir_in(&self.storage_path)?;
        let unpacked = staging.path().join("artifact");
        registry::pull_artifact(reference, &unpacked)
            .await
            .map_err(|err| err.wrap_err(format!("failed to pull OCI task include {reference}")))?;
        prepare_unpacked_artifact(&unpacked)?;
        Ok((staging.keep(), unpacked))
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskOci {
    fn is_match(&self, file: &str) -> bool {
        file.starts_with(OCI_INCLUDE_PREFIX)
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        let artifact = self.get_local_artifact(file).await?;
        Ok(artifact.path)
    }

    async fn get_local_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        let reference = reference_of(file)?;
        if !self.is_cached {
            let (staging, unpacked) = self.pull_staged(reference).await?;
            return Ok(TaskFileArtifact::temporary(unpacked, staging));
        }
        let destination = self.storage_path.join(Self::get_cache_key(reference));
        if destination.is_dir() {
            debug!(
                "Using cached OCI task include: {}",
                display_path(&destination)
            );
            return Ok(TaskFileArtifact::persistent(destination));
        }
        let (staging, unpacked) = self.pull_staged(reference).await?;
        // The cache entry appears only once complete, so an interrupted pull
        // never leaves a half-populated directory behind.
        let moved = std::fs::rename(&unpacked, &destination);
        let _ = file::remove_all(&staging);
        // A concurrent mise process may have populated the cache first.
        if let Err(err) = moved
            && !destination.is_dir()
        {
            eyre::bail!(
                "failed to move OCI task include into place at {}: {err}",
                display_path(&destination)
            );
        }
        Ok(TaskFileArtifact::persistent(destination))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_only_oci_prefix() {
        let provider = RemoteTaskOciBuilder::new().build();
        assert!(provider.is_match("oci::ghcr.io/org/tasks:latest"));
        assert!(!provider.is_match("ghcr.io/org/tasks:latest"));
        assert!(!provider.is_match("git::https://github.com/org/repo.git//tasks"));
    }

    #[test]
    fn rejects_empty_reference() {
        assert!(reference_of("oci::").is_err());
        assert_eq!(reference_of("oci:: a/b:c ").unwrap(), "a/b:c");
    }

    #[test]
    fn cache_key_depends_on_the_full_reference() {
        assert_ne!(
            RemoteTaskOci::get_cache_key("ghcr.io/org/tasks:1"),
            RemoteTaskOci::get_cache_key("ghcr.io/org/tasks:2")
        );
    }

    #[test]
    #[cfg(unix)]
    fn shebang_scripts_become_executable_but_toml_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("build");
        let toml = dir.path().join("tasks.toml");
        let readme = dir.path().join("README.md");
        std::fs::write(&script, "#!/usr/bin/env bash\necho hi\n").unwrap();
        std::fs::write(&toml, "#!not really\n").unwrap();
        std::fs::write(&readme, "docs\n").unwrap();

        prepare_unpacked_artifact(dir.path()).unwrap();

        assert!(file::is_executable(&script));
        assert!(!file::is_executable(&toml));
        assert!(!file::is_executable(&readme));
    }
}
