use std::path::PathBuf;

use crate::{file, http::HTTP};

use super::TaskFileProvider;

#[derive(Debug)]
pub struct HttpTaskFileProvider {
    tmpdir: PathBuf,
}

impl HttpTaskFileProvider {
    pub fn new(tmpdir: PathBuf) -> Self {
        Self { tmpdir }
    }
}

impl TaskFileProvider for HttpTaskFileProvider {
    fn is_match(&self, file: &str) -> bool {
        file.starts_with("http://") || file.starts_with("https://")
    }

    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let url = url::Url::parse(file)?;
        let filename = url
            .path_segments()
            .and_then(|segments| segments.last())
            .unwrap();
        let tmp_path = self.tmpdir.join(filename);
        HTTP.download_file(file, &tmp_path, None)?;
        file::make_executable(&tmp_path)?;
        Ok(tmp_path)
    }
}

#[cfg(test)]
mod tests {

    use std::env;

    use super::*;

    #[test]
    fn test_is_match() {
        let provider = HttpTaskFileProvider::new(env::temp_dir());
        assert!(provider.is_match("http://test.txt"));
        assert!(provider.is_match("https://test.txt"));
        assert!(provider.is_match("https://mydomain.com/myfile.py"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh"));
        assert!(provider.is_match("https://subdomain.mydomain.com/myfile.sh?query=1"));
    }

    #[test]
    fn test_http_task_file_provider_get_local_path() {
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
                .create();

            let provider = HttpTaskFileProvider::new(env::temp_dir());
            let mock = format!("{}{}", server.url(), path);
            let path = provider.get_local_path(&mock).unwrap();
            assert!(path.exists());
            assert!(path.is_file());

            mocked_server.assert();
        }
    }
}
