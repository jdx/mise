use std::sync::LazyLock as Lazy;
use std::{fmt::Debug, path::PathBuf};

mod local_task;
mod remote_task_http;

pub use local_task::LocalTask;
pub use remote_task_http::RemoteTaskHttp;

use crate::config::Settings;
use crate::dirs;

static REMOTE_TASK_CACHE_DIR: Lazy<PathBuf> = Lazy::new(|| dirs::CACHE.join("remote-tasks-cache"));

pub trait TaskFileProvider: Debug {
    fn is_match(&self, file: &str) -> bool;
    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>>;
}

pub struct TaskFileProviders {
    no_cache: bool,
}

impl TaskFileProviders {
    pub fn new() -> Self {
        let no_cache = Settings::get().task_remote_no_cache.unwrap_or(false);
        Self { no_cache }
    }

    fn get_providers(&self) -> Vec<Box<dyn TaskFileProvider>> {
        vec![
            Box::new(RemoteTaskHttp::new(
                REMOTE_TASK_CACHE_DIR.clone(),
                self.no_cache,
            )),
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
        let task_file_providers = TaskFileProviders::new();
        let providers = task_file_providers.get_providers();
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_local_file_match_local_provider() {
        let task_file_providers = TaskFileProviders::new();
        let cases = vec!["file.txt", "./file.txt", "../file.txt", "/file.txt"];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            assert!(format!("{:?}", provider.unwrap()).contains("LocalTask"));
        }
    }

    #[test]
    fn test_http_file_match_http_remote_task_provider() {
        let task_file_providers = TaskFileProviders::new();
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
