use std::path::PathBuf;

use crate::{dirs, env, file, hash, http::HTTP};

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

    fn download_file(
        &self,
        file: &str,
        destination: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        HTTP.download_file(file, destination, None)?;
        file::make_executable(destination)?;
        Ok(())
    }
}

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

    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        match self.is_cached {
            true => {
                trace!("Cache mode enabled");
                let cache_key = self.get_cache_key(file);
                let destination = self.storage_path.join(&cache_key);

                if destination.exists() {
                    debug!("Using cached file: {:?}", destination);
                    return Ok(destination);
                }

                debug!("Downloading file: {}", file);
                self.download_file(file, &destination)?;
                Ok(destination)
            }
            false => {
                trace!("Cache mode disabled");
                let url = url::Url::parse(file)?;
                let filename = url
                    .path_segments()
                    .and_then(|segments| segments.last())
                    .unwrap();

                let destination = env::temp_dir().join(filename);
                if destination.exists() {
                    file::remove_file(&destination)?;
                }
                self.download_file(file, &destination)?;
                Ok(destination)
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_is_match() {
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

    #[test]
    fn test_http_remote_task_get_local_path_without_cache() {
        let paths = vec![
            ("/myfile.py", "myfile.py"),
            ("/subpath/myfile.sh", "myfile.sh"),
            ("/myfile.sh?query=1&sdfsdf=2", "myfile.sh"),
        ];
        let mut server = mockito::Server::new();

        for (request_path, expected_file_name) in paths {
            let mocked_server: mockito::Mock = server
                .mock("GET", request_path)
                .with_status(200)
                .with_body("Random content")
                .expect(2)
                .create();

            let provider = RemoteTaskHttpBuilder::new().build();
            let mock = format!("{}{}", server.url(), request_path);

            for _ in 0..2 {
                let local_path = provider.get_local_path(&mock).unwrap();
                assert!(local_path.exists());
                assert!(local_path.is_file());
                assert!(local_path.ends_with(expected_file_name));
            }

            mocked_server.assert();
        }
    }

    #[test]
    fn test_http_remote_task_get_local_path_with_cache() {
        let paths = vec![
            ("/myfile.py", "myfile.py"),
            ("/subpath/myfile.sh", "myfile.sh"),
            ("/myfile.sh?query=1&sdfsdf=2", "myfile.sh"),
        ];
        let mut server = mockito::Server::new();

        for (request_path, not_expected_file_name) in paths {
            let mocked_server = server
                .mock("GET", request_path)
                .with_status(200)
                .with_body("Random content")
                .expect(1)
                .create();

            let provider = RemoteTaskHttpBuilder::new().with_cache(true).build();
            let mock = format!("{}{}", server.url(), request_path);

            for _ in 0..2 {
                let path = provider.get_local_path(&mock).unwrap();
                assert!(path.exists());
                assert!(path.is_file());
                assert!(!path.ends_with(not_expected_file_name));
            }

            mocked_server.assert();
        }
    }
}
