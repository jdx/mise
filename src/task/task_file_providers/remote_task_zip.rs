use crate::Result;
use std::path::PathBuf;

use async_trait::async_trait;

use crate::{
    dirs, env,
    file::{self, ExtractOptions, display_path},
    hash,
    http::HTTP,
};

use super::remote_task_oci::prepare_unpacked_artifact;
use super::{TaskFileArtifact, TaskFileProvider};

/// Prefix of a `task_config.includes` or `include` entry that downloads a zip archive.
pub(crate) const ZIP_INCLUDE_PREFIX: &str = "zip::";

#[derive(Debug)]
pub(super) struct RemoteTaskZipBuilder {
    store_path: PathBuf,
    use_cache: bool,
}

impl RemoteTaskZipBuilder {
    pub(super) fn new() -> Self {
        Self {
            store_path: env::temp_dir(),
            use_cache: false,
        }
    }

    pub(super) fn with_cache(mut self, use_cache: bool) -> Self {
        if use_cache {
            self.store_path = dirs::CACHE.join("remote-zip-tasks-cache");
            self.use_cache = true;
        }
        self
    }

    pub(super) fn build(self) -> RemoteTaskZip {
        RemoteTaskZip {
            storage_path: self.store_path,
            is_cached: self.use_cache,
        }
    }
}

/// Downloads a task catalog or config published as a zip archive over http(s).
/// The unpacked archive is a directory, loaded like any other task include
/// directory.
#[derive(Debug)]
pub(super) struct RemoteTaskZip {
    storage_path: PathBuf,
    is_cached: bool,
}

fn url_of(file: &str) -> Result<&str> {
    let url = file.strip_prefix(ZIP_INCLUDE_PREFIX).unwrap_or(file).trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        eyre::bail!("a zip:: include must be an http(s) URL to a .zip archive: {file}");
    }
    Ok(url)
}

impl RemoteTaskZip {
    fn get_cache_key(url: &str) -> String {
        hash::hash_sha256_to_str(url)
    }

    /// Download and unpack into a fresh staging directory. Returns the staging
    /// root and the unpacked archive inside it; the caller owns the staging
    /// root, which its guard removes if the download or unpacking fails.
    async fn fetch_staged(&self, url: &str) -> Result<(PathBuf, PathBuf)> {
        file::create_dir_all(&self.storage_path)?;
        let staging = tempfile::Builder::new()
            .prefix(".zip-fetch-")
            .tempdir_in(&self.storage_path)?;
        let archive = staging.path().join("archive.zip");
        HTTP.download_file(url, &archive, None)
            .await
            .map_err(|err| err.wrap_err(format!("failed to download zip include {url}")))?;
        let unpacked = staging.path().join("artifact");
        file::create_dir_all(&unpacked)?;
        file::unzip(&archive, &unpacked, &ExtractOptions::default())?;
        file::remove_file(&archive)?;
        prepare_unpacked_artifact(&unpacked)?;
        Ok((staging.keep(), unpacked))
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskZip {
    fn is_match(&self, file: &str) -> bool {
        file.starts_with(ZIP_INCLUDE_PREFIX)
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        Ok(self.get_local_artifact(file).await?.path)
    }

    async fn get_local_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        let url = url_of(file)?;
        if !self.is_cached {
            let (staging, unpacked) = self.fetch_staged(url).await?;
            return Ok(TaskFileArtifact::temporary(unpacked, staging));
        }
        let destination = self.storage_path.join(Self::get_cache_key(url));
        if destination.is_dir() {
            debug!("Using cached zip include: {}", display_path(&destination));
            return Ok(TaskFileArtifact::persistent(destination));
        }
        let (staging, unpacked) = self.fetch_staged(url).await?;
        // The cache entry appears only once complete, so an interrupted
        // download never leaves a half-populated directory behind.
        let moved = std::fs::rename(&unpacked, &destination);
        let _ = file::remove_all(&staging);
        // A concurrent mise process may have populated the cache first.
        if let Err(err) = moved
            && !destination.is_dir()
        {
            eyre::bail!(
                "failed to move zip include into place at {}: {err}",
                display_path(&destination)
            );
        }
        Ok(TaskFileArtifact::persistent(destination))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        buf.into_inner()
    }

    #[test]
    fn matches_only_zip_prefix() {
        let provider = RemoteTaskZipBuilder::new().build();
        assert!(provider.is_match("zip::https://example.com/tasks.zip"));
        assert!(!provider.is_match("https://example.com/tasks.zip"));
        assert!(!provider.is_match("oci::ghcr.io/org/tasks:latest"));
    }

    #[test]
    fn requires_an_http_url() {
        assert!(url_of("zip::").is_err());
        assert!(url_of("zip::./tasks.zip").is_err());
        assert_eq!(
            url_of("zip:: https://example.com/a.zip ").unwrap(),
            "https://example.com/a.zip"
        );
    }

    #[test]
    fn cache_key_depends_on_the_full_url() {
        assert_ne!(
            RemoteTaskZip::get_cache_key("https://example.com/a.zip"),
            RemoteTaskZip::get_cache_key("https://example.com/a.zip?v=2")
        );
    }

    #[tokio::test]
    async fn downloads_and_unpacks_the_archive() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/tasks.zip")
            .with_status(200)
            .with_body(make_zip(&[
                ("mise.toml", "[tasks.hi]\nrun = 'echo hi'\n"),
                ("build", "#!/usr/bin/env bash\necho build\n"),
            ]))
            .create_async()
            .await;
        let provider = RemoteTaskZipBuilder::new().build();
        let artifact = provider
            .get_local_artifact(&format!("zip::{}/tasks.zip", server.url()))
            .await
            .unwrap();
        assert!(artifact.path.join("mise.toml").is_file());
        #[cfg(unix)]
        assert!(file::is_executable(&artifact.path.join("build")));
        let path = artifact.path.clone();
        drop(artifact);
        assert!(!path.exists());
        mock.assert();
    }
}
