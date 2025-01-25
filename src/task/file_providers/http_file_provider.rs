use std::path::PathBuf;

use md5::Digest;
use sha2::Sha256;

use crate::{env, file, http::HTTP};

use super::TaskFileProvider;

#[derive(Debug)]
pub struct HttpTaskFileProvider {
    cache_path: PathBuf,
    no_cache: bool,
}

impl HttpTaskFileProvider {
    pub fn new(cache_path: PathBuf, no_cache: bool) -> Self {
        Self {
            cache_path,
            no_cache,
        }
    }
}

impl HttpTaskFileProvider {
    fn get_cache_key(&self, file: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(file);
        format!("{:x}", hasher.finalize())
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

impl TaskFileProvider for HttpTaskFileProvider {
    fn is_match(&self, file: &str) -> bool {
        file.starts_with("http://") || file.starts_with("https://")
    }

    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        match self.no_cache {
            false => {
                debug!("Cache mode enabled");
                let cache_key = self.get_cache_key(file);
                let destination = self.cache_path.join(&cache_key);

                if destination.exists() {
                    debug!("Using cached file: {:?}", destination);
                    return Ok(destination);
                }

                debug!("Downloading file: {}", file);
                self.download_file(file, &destination)?;
                Ok(destination)
            }
            true => {
                debug!("Cache mode disabled");
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

    use std::env;

    use super::*;

    #[test]
    fn test_is_match() {
        let provider = HttpTaskFileProvider::new(env::temp_dir(), true);
        assert!(provider.is_match("http://test.txt"));
        assert!(provider.is_match("https://test.txt"));
        assert!(provider.is_match("https://mydomain.com/myfile.py"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh?query=1"));
    }

    #[test]
    fn test_http_task_file_provider_get_local_path_without_cache() {
        let paths = vec![
            "/myfile.py",
            "/subpath/myfile.sh",
            "/myfile.sh?query=1&sdfsdf=2",
        ];
        let mut server = mockito::Server::new();

        for path in paths {
            let mocked_server = server
                .mock("GET", path)
                .with_status(200)
                .with_body("Random content")
                .expect(2)
                .create();

            let provider = HttpTaskFileProvider::new(env::temp_dir(), true);
            let mock = format!("{}{}", server.url(), path);

            for _ in 0..2 {
                let path = provider.get_local_path(&mock).unwrap();
                assert!(path.exists());
                assert!(path.is_file());
            }

            mocked_server.assert();
        }
    }

    #[test]
    fn test_http_task_file_provider_get_local_path_with_cache() {
        let paths = vec![
            "/myfile.py",
            "/subpath/myfile.sh",
            "/myfile.sh?query=1&sdfsdf=2",
        ];
        let mut server = mockito::Server::new();

        for path in paths {
            let mocked_server = server
                .mock("GET", path)
                .with_status(200)
                .with_body("Random content")
                .expect(1)
                .create();

            let provider = HttpTaskFileProvider::new(env::temp_dir(), false);
            let mock = format!("{}{}", server.url(), path);

            for _ in 0..2 {
                let path = provider.get_local_path(&mock).unwrap();
                assert!(path.exists());
                assert!(path.is_file());
            }

            mocked_server.assert();
        }
    }
}
