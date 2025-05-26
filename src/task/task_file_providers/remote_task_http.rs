use std::path::PathBuf;

use async_trait::async_trait;

use crate::{Result, dirs, env, file, hash, http::HTTP};

use super::TaskFileProvider;

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

    async fn download_file(&self, file: &str, destination: &PathBuf) -> Result<()> {
        trace!("Downloading file: {}", file);
        HTTP.download_file(file, destination, None).await?;
        file::make_executable(destination)?;
        Ok(())
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskHttp {
    fn is_match(&self, file: &str) -> bool {
        let url = url::Url::parse(file);

        // Check if the URL is valid and the scheme is http or https
        // and the path is not empty
        // and the path is not a directory
        url.is_ok_and(|url| {
            (url.scheme() == "http" || url.scheme() == "https")
                && url.path().len() > 1
                && !url.path().ends_with('/')
        })
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        let cache_key = self.get_cache_key(file);
        let destination = self.storage_path.join(&cache_key);

        match self.is_cached {
            true => {
                trace!("Cache mode enabled");

                if destination.exists() {
                    debug!("Using cached file: {:?}", destination);
                    return Ok(destination);
                }
            }
            false => {
                trace!("Cache mode disabled");

                if destination.exists() {
                    file::remove_file(&destination)?;
                }
            }
        }

        self.download_file(file, &destination).await?;
        Ok(destination)
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
    async fn test_http_remote_task_get_local_path_without_cache() {
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

            for _ in 0..2 {
                let local_path = provider.get_local_path(&request_url).await.unwrap();
                assert!(local_path.exists());
                assert!(local_path.is_file());
                assert!(local_path.ends_with(&cache_key));
            }

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
}
