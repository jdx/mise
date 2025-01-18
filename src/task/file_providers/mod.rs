use std::path::{Path, PathBuf};

mod http_file_provider;
mod local_file_provider;

pub use http_file_provider::HttpTaskFileProvider;
pub use local_file_provider::LocalTaskFileProvider;

pub trait TaskFileProvider {
    fn is_match(&self, file: &str) -> bool;
    fn get_local_path(
        &self,
        tmpdir: &Path,
        file: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>>;
}

pub struct TaskFileProviders;

impl TaskFileProviders {
    pub fn get_providers() -> Vec<Box<dyn TaskFileProvider>> {
        vec![
            Box::new(HttpTaskFileProvider),
            Box::new(LocalTaskFileProvider),
        ]
    }
}
