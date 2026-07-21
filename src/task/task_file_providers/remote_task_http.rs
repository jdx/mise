use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::{
    Result, dirs, env, file, hash, http::HTTP, lock_file::LockFile, remote_source::RemoteSource,
};

use super::{TaskFileArtifact, TaskFileProvider};

#[derive(Debug)]
pub struct RemoteTaskHttpBuilder {
    store_path: PathBuf,
    use_cache: bool,
}

impl RemoteTaskHttpBuilder {
    pub fn new() -> Self {
        Self {
            store_path: env::temp_dir(),
            use_cache: false,
        }
    }

    pub fn with_cache(mut self, use_cache: bool) -> Self {
        if use_cache {
            self.store_path = dirs::CACHE.join("remote-http-tasks-cache");
            self.use_cache = true;
        }
        self
    }

    pub fn build(self) -> RemoteTaskHttp {
        RemoteTaskHttp {
            storage_path: self.store_path,
            is_cached: self.use_cache,
        }
    }
}

#[derive(Debug)]
pub struct RemoteTaskHttp {
    storage_path: PathBuf,
    is_cached: bool,
}

impl RemoteTaskHttp {
    fn get_cache_key(&self, file: &str) -> String {
        hash::hash_sha256_to_str(file)
    }

    async fn download_file(&self, file: &str, destination: &Path) -> Result<()> {
        trace!("Downloading file: {}", file);
        HTTP.download_file(file, destination, None).await?;
        file::make_executable(destination)?;
        Ok(())
    }

    fn temp_download_path(destination: &Path) -> PathBuf {
        let mut path = destination.as_os_str().to_os_string();
        path.push(".download-tmp");
        path.into()
    }

    async fn download_file_atomically(&self, file: &str, destination: &Path) -> Result<()> {
        let temp = Self::temp_download_path(destination);
        if temp.exists() {
            file::remove_file(&temp)?;
        }
        if let Err(error) = self.download_file(file, &temp).await {
            let _ = file::remove_file(&temp);
            return Err(error);
        }
        if let Err(error) = file::rename(&temp, destination) {
            let _ = file::remove_file(&temp);
            return Err(error);
        }
        Ok(())
    }

    async fn get_unique_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        let cache_key = self.get_cache_key(file);
        file::create_dir_all(&self.storage_path)?;
        let temp_file =
            tempfile::NamedTempFile::with_prefix_in(format!("{cache_key}-"), &self.storage_path)?;
        let (_, destination) = temp_file.keep()?;
        if let Err(error) = self.download_file(file, &destination).await {
            let _ = file::remove_file(&destination);
            return Err(error);
        }
        Ok(TaskFileArtifact::temporary(
            destination.clone(),
            destination,
        ))
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskHttp {
    fn is_match(&self, file: &str) -> bool {
        RemoteSource::parse_http(file).is_some()
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        let cache_key = self.get_cache_key(file);
        let destination = self.storage_path.join(&cache_key);
        if self.is_cached {
            trace!("Cache mode enabled");
            file::create_dir_all(&self.storage_path)?;
            let _lock = LockFile::new(&destination).lock()?;
            if destination.exists() {
                debug!("Using cached file: {:?}", destination);
                return Ok(destination);
            }
            self.download_file_atomically(file, &destination).await?;
            return Ok(destination);
        }

        trace!("Cache mode disabled");
        file::create_dir_all(&self.storage_path)?;
        let _lock = LockFile::new(&destination).lock()?;
        self.download_file_atomically(file, &destination).await?;
        Ok(destination)
    }

    async fn get_local_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        if self.is_cached {
            return Ok(TaskFileArtifact::persistent(
                self.get_local_path(file).await?,
            ));
        }
        self.get_unique_artifact(file).await
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn test_is_match() {
        let provider = RemoteTaskHttpBuilder::new().build();

        // Positive cases
        assert!(provider.is_match("http://myhost.com/test.txt"));
        assert!(provider.is_match("https://myhost.com/test.txt"));
        assert!(provider.is_match("https://mydomain.com/myfile.py"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh?query=1"));

        // Negative cases
        assert!(!provider.is_match("https://myhost.com/js/"));
        assert!(!provider.is_match("https://myhost.com"));
        assert!(!provider.is_match("https://myhost.com/"));
    }

    #[tokio::test]
    async fn test_http_remote_task_get_local_artifact_without_cache() {
        let paths = vec![
            "/myfile.py",
            "/subpath/myfile.sh",
            "/myfile.sh?query=1&sdfsdf=2",
        ];
        let mut server = mockito::Server::new_async().await;

        for request_path in paths {
            let mocked_server: mockito::Mock = server
                .mock("GET", request_path)
                .with_status(200)
                .with_body("Random content")
                .expect(2)
                .create_async()
                .await;

            let provider = RemoteTaskHttpBuilder::new().build();
            let request_url = format!("{}{}", server.url(), request_path);
            let cache_key = provider.get_cache_key(&request_url);

            let mut local_paths = vec![];
            for _ in 0..2 {
                let artifact = provider.get_local_artifact(&request_url).await.unwrap();
                let local_path = artifact.path.clone();
                assert!(local_path.exists());
                assert!(local_path.is_file());
                assert!(
                    local_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(&cache_key)
                );
                local_paths.push((local_path, artifact));
            }
            assert_ne!(local_paths[0].0, local_paths[1].0);
            let retained_paths = local_paths
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>();
            drop(local_paths);
            assert!(retained_paths.iter().all(|path| !path.exists()));

            mocked_server.assert();
        }
    }

    #[tokio::test]
    async fn test_http_remote_task_get_local_path_with_cache() {
        let paths = vec![
            "/myfile.py",
            "/subpath/myfile.sh",
            "/myfile.sh?query=1&sdfsdf=2",
        ];
        let mut server = mockito::Server::new_async().await;

        for request_path in paths {
            let mocked_server = server
                .mock("GET", request_path)
                .with_status(200)
                .with_body("Random content")
                .expect(1)
                .create_async()
                .await;

            let provider = RemoteTaskHttpBuilder::new().with_cache(true).build();
            let request_url = format!("{}{}", server.url(), request_path);
            let cache_key = provider.get_cache_key(&request_url);

            for _ in 0..2 {
                let path = provider.get_local_path(&request_url).await.unwrap();
                assert!(path.exists());
                assert!(path.is_file());
                assert!(path.ends_with(&cache_key));
            }

            mocked_server.assert();
        }
    }

    #[tokio::test]
    async fn test_cached_download_failure_leaves_no_partial_file() {
        let mut server = mockito::Server::new_async().await;
        let remote = server
            .mock("GET", "/task")
            .with_status(500)
            .expect(4)
            .create_async()
            .await;
        let storage = tempfile::tempdir().unwrap();
        let provider = RemoteTaskHttp {
            storage_path: storage.path().to_path_buf(),
            is_cached: true,
        };
        let request_url = format!("{}/task", server.url());
        let destination = storage.path().join(provider.get_cache_key(&request_url));
        let temp_destination = RemoteTaskHttp::temp_download_path(&destination);
        std::fs::write(&temp_destination, b"partial download").unwrap();

        assert!(provider.get_local_path(&request_url).await.is_err());
        assert!(!destination.exists());
        assert!(!temp_destination.exists());
        remote.assert_async().await;
    }
}
