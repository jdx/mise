use std::{
    fmt::Debug,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

mod local_task;
mod remote_task_git;
mod remote_task_http;
use crate::Result;
use async_trait::async_trait;
use local_task::LocalTask;
use remote_task_git::RemoteTaskGitBuilder;
use remote_task_http::RemoteTaskHttpBuilder;

#[async_trait]
pub trait TaskFileProvider: Debug + Send + Sync {
    fn is_match(&self, file: &str) -> bool;
    async fn get_local_path(&self, file: &str) -> Result<PathBuf>;

    async fn get_local_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        Ok(TaskFileArtifact::persistent(
            self.get_local_path(file).await?,
        ))
    }
}

#[derive(Debug)]
struct TaskFileArtifactCleanup {
    path: PathBuf,
}

impl Drop for TaskFileArtifactCleanup {
    fn drop(&mut self) {
        if let Err(err) = remove_temporary_artifact(&self.path) {
            warn!(
                "failed to clean up remote task artifact {}: {err:#}",
                crate::file::display_path(&self.path)
            );
        }
    }
}

fn remove_temporary_artifact(path: &Path) -> io::Result<()> {
    retry_remove_temporary_artifact(|| match path.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(err) => Err(err),
    })
}

fn retry_remove_temporary_artifact(mut remove: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    const MAX_RETRIES: u32 = 4;

    for retry in 0..MAX_RETRIES {
        match remove() {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err)
                if err.kind() == io::ErrorKind::DirectoryNotEmpty
                    || cfg!(windows) && err.kind() == io::ErrorKind::PermissionDenied =>
            {
                std::thread::sleep(Duration::from_millis(10 * (1 << retry)));
            }
            Err(err) => return Err(err),
        }
    }

    match remove() {
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[derive(Debug, Clone)]
pub struct TaskFileArtifact {
    pub path: PathBuf,
    _cleanup: Option<Arc<TaskFileArtifactCleanup>>,
}

impl TaskFileArtifact {
    pub(crate) fn persistent(path: PathBuf) -> Self {
        Self {
            path,
            _cleanup: None,
        }
    }

    pub(crate) fn temporary(path: PathBuf, cleanup_path: PathBuf) -> Self {
        Self {
            path,
            _cleanup: Some(Arc::new(TaskFileArtifactCleanup { path: cleanup_path })),
        }
    }
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
                RemoteTaskGitBuilder::new()
                    .with_cache(self.use_cache)
                    .build(),
            ),
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
    fn test_temporary_artifact_cleanup_ignores_missing_paths() {
        retry_remove_temporary_artifact(|| Err(io::Error::from(io::ErrorKind::NotFound))).unwrap();
    }

    #[test]
    fn test_temporary_artifact_cleanup_removes_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("artifact");
        std::fs::write(&file, "artifact").unwrap();

        remove_temporary_artifact(&file).unwrap();

        assert!(!file.exists());
    }

    #[test]
    fn test_temporary_artifact_cleanup_retries_directory_not_empty() {
        let mut attempts = 0;
        retry_remove_temporary_artifact(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::DirectoryNotEmpty))
            } else {
                Ok(())
            }
        })
        .unwrap();

        assert_eq!(attempts, 3);
    }

    #[test]
    #[cfg(not(windows))]
    fn test_temporary_artifact_cleanup_does_not_retry_permission_denied() {
        let mut attempts = 0;
        let err = retry_remove_temporary_artifact(|| {
            attempts += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();

        assert_eq!(attempts, 1);
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    #[cfg(windows)]
    fn test_temporary_artifact_cleanup_retries_permission_denied() {
        let mut attempts = 0;
        retry_remove_temporary_artifact(|| {
            attempts += 1;
            if attempts < 3 {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                Ok(())
            }
        })
        .unwrap();

        assert_eq!(attempts, 3);
    }

    #[test]
    fn test_get_providers() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let providers = task_file_providers.get_providers();
        assert_eq!(providers.len(), 3);
    }

    #[test]
    fn test_local_file_match_local_provider() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let cases = vec!["file.txt", "./file.txt", "../file.txt", "/file.txt"];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            let provider_name = format!("{:?}", provider.unwrap());
            assert!(provider_name.contains("LocalTask"));
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
            let provider_name = format!("{:?}", provider.unwrap());
            assert!(provider_name.contains("RemoteTaskHttp"));
        }
    }

    #[test]
    fn test_git_file_match_git_remote_task_provider() {
        let task_file_providers = TaskFileProvidersBuilder::new().build();
        let cases = vec![
            "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::ssh://user@myserver.com/example.git//subfolder/myfile.py",
            "git::https://myserver.com/example.git//subfolder/myfile.sh",
        ];

        for file in cases {
            let provider = task_file_providers.get_provider(file);
            assert!(provider.is_some());
            let provider_name = format!("{:?}", provider.unwrap());
            assert!(provider_name.contains("RemoteTaskGit"));
        }
    }
}
