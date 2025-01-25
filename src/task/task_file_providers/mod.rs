use std::{fmt::Debug, path::PathBuf};

mod local_task;
mod remote_task_http;

pub use local_task::LocalTask;
use remote_task_http::RemoteTaskHttpBuilder;

pub trait TaskFileProvider: Debug {
    fn is_match(&self, file: &str) -> bool;
    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>>;
}

pub struct TaskFileProvidersBuilder {
    use_cache: bool,
}

impl TaskFileProvidersBuilder {
    pub fn new() -> Self {
        Self { use_cache: false }
    }

    pub fn with_cache(mut self, use_cache: bool) -> Self {
        self.use_cache = use_cache;
        self
    }

    pub fn build(self) -> TaskFileProviders {
        TaskFileProviders::new(self.use_cache)
    }
}

pub struct TaskFileProviders {
    use_cache: bool,
}

impl TaskFileProviders {
    pub fn new(use_cache: bool) -> Self {
        Self { use_cache }
    }

    fn get_providers(&self) -> Vec<Box<dyn TaskFileProvider>> {
        vec![
            Box::new(
                RemoteTaskHttpBuilder::new()
                    .with_cache(self.use_cache)
                    .build(),
            ),
            Box::new(LocalTask), // Must be the last provider
        ]
    }

    pub fn get_provider(&self, file: &str) -> Option<Box<dyn TaskFileProvider>> {
        self.get_providers().into_iter().find(|p| p.is_match(file))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_get_providers() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let providers = task_file_providers.get_providers();
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_local_file_match_local_provider() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let cases = vec!["file.txt", "./file.txt", "../file.txt", "/file.txt"];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            assert!(format!("{:?}", provider.unwrap()).contains("LocalTask"));
        }
    }

    #[test]
    fn test_http_file_match_http_remote_task_provider() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let cases = vec![
            "http://example.com/file.txt",
            "https://example.com/file.txt",
            "https://example.com/subfolder/file.txt",
        ];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            assert!(format!("{:?}", provider.unwrap()).contains("RemoteTaskHttp"));
        }
    }
}
