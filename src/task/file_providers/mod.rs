use std::{fmt::Debug, path::PathBuf};

mod http_file_provider;
mod local_file_provider;

pub use http_file_provider::HttpTaskFileProvider;
pub use local_file_provider::LocalTaskFileProvider;

pub trait TaskFileProvider: Debug {
    fn is_match(&self, file: &str) -> bool;
    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>>;
}

pub struct TaskFileProviders {
    tmpdir: PathBuf,
}

impl TaskFileProviders {
    fn get_providers(&self) -> Vec<Box<dyn TaskFileProvider>> {
        vec![
            Box::new(HttpTaskFileProvider::new(self.tmpdir.clone())),
            Box::new(LocalTaskFileProvider), // Must be the last provider
        ]
    }

    pub fn new(tmpdir: PathBuf) -> Self {
        Self { tmpdir }
    }

    pub fn get_provider(&self, file: &str) -> Option<Box<dyn TaskFileProvider>> {
        self.get_providers().into_iter().find(|p| p.is_match(file))
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn test_get_providers() {
        let task_file_providers = TaskFileProviders::new(env::temp_dir());
        let providers = task_file_providers.get_providers();
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_local_file_match_local_provider() {
        let task_file_providers = TaskFileProviders::new(env::temp_dir());
        let cases = vec!["file.txt", "./file.txt", "../file.txt", "/file.txt"];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            assert!(format!("{:?}", provider.unwrap()).contains("LocalTaskFileProvider"));
        }
    }

    #[test]
    fn test_http_file_match_http_provider() {
        let task_file_providers = TaskFileProviders::new(env::temp_dir());
        let cases = vec![
            "http://example.com/file.txt",
            "https://example.com/file.txt",
            "https://example.com/subfolder/file.txt",
        ];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            assert!(format!("{:?}", provider.unwrap()).contains("HttpTaskFileProvider"));
        }
    }
}
