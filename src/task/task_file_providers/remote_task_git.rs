use crate::Result;
use std::path::PathBuf;

use async_trait::async_trait;

use crate::{
    dirs, env,
    file::{self, display_path},
    git::{self, CloneOptions},
    hash,
    lock_file::LockFile,
    remote_source::{RemoteGitSource, RemoteSource},
};

use super::{TaskFileArtifact, TaskFileProvider};

#[derive(Debug)]
pub struct RemoteTaskGitBuilder {
    store_path: PathBuf,
    use_cache: bool,
}

impl RemoteTaskGitBuilder {
    pub fn new() -> Self {
        Self {
            store_path: env::temp_dir(),
            use_cache: false,
        }
    }

    pub fn with_cache(mut self, use_cache: bool) -> Self {
        if use_cache {
            self.store_path = dirs::CACHE.join("remote-git-tasks-cache");
            self.use_cache = true;
        }
        self
    }

    pub fn build(self) -> RemoteTaskGit {
        RemoteTaskGit {
            storage_path: self.store_path,
            is_cached: self.use_cache,
        }
    }
}

#[derive(Debug)]
pub struct RemoteTaskGit {
    storage_path: PathBuf,
    is_cached: bool,
}

#[derive(Debug, Clone)]
struct GitRepoStructure {
    url_without_path: String,
    path: String,
    branch: Option<String>,
}

impl GitRepoStructure {
    pub fn new(url_without_path: &str, path: &str, branch: Option<String>) -> Self {
        Self {
            url_without_path: url_without_path.to_string(),
            path: path.to_string(),
            branch,
        }
    }
}

impl RemoteTaskGit {
    /// Make fetched task files executable while leaving task include directories intact.
    fn prepare_remote_path(path: &PathBuf) -> Result<()> {
        let metadata = path.symlink_metadata()?;
        if metadata.file_type().is_file() {
            return file::make_executable(path);
        }
        if metadata.file_type().is_dir() {
            return Ok(());
        }
        eyre::bail!(
            "remote task path is not a regular file or directory: {}",
            display_path(path)
        )
    }

    fn get_cache_key(&self, repo_structure: &GitRepoStructure) -> String {
        let key = format!(
            "{}{}",
            repo_structure.url_without_path,
            repo_structure.branch.to_owned().unwrap_or("".to_string())
        );
        hash::hash_sha256_to_str(&key)
    }

    fn get_repo_structure(&self, file: &str) -> GitRepoStructure {
        RemoteSource::parse_git(file)
            .map(|source| source.into())
            .unwrap()
    }

    fn unique_artifact_root(&self, cache_key: &str) -> Result<PathBuf> {
        file::create_dir_all(&self.storage_path)?;
        let temp_dir = tempfile::Builder::new()
            .prefix(&format!("{cache_key}-"))
            .tempdir_in(&self.storage_path)?;
        Ok(temp_dir.keep())
    }

    fn clone_to(repo_structure: &GitRepoStructure, destination: &PathBuf) -> Result<()> {
        let git_repo = git::Git::new(destination);
        let mut clone_options = CloneOptions::default();
        if let Some(branch) = &repo_structure.branch {
            trace!("Use specific branch {}", branch);
            clone_options = clone_options.branch(branch);
        }
        git_repo.clone(repo_structure.url_without_path.as_str(), clone_options)
    }

    fn fetch_to_destination(
        &self,
        repo_structure: &GitRepoStructure,
        destination: &PathBuf,
        reuse_existing: bool,
    ) -> Result<PathBuf> {
        let repo_file_path = repo_structure.path.clone();
        let full_path = destination.join(&repo_file_path);

        debug!("Repo structure: {:?}", repo_structure);

        let _lock = LockFile::new(destination)
            .with_callback(|l| {
                debug!(
                    "waiting for lock on remote git task cache: {}",
                    display_path(l)
                );
            })
            .lock()?;

        if reuse_existing && full_path.exists() {
            debug!("Using cached file: {:?}", full_path);
            Self::prepare_remote_path(&full_path)?;
            return Ok(full_path);
        }

        let mut tmp_destination = destination.as_os_str().to_os_string();
        tmp_destination.push(".clone-tmp");
        let tmp_destination = PathBuf::from(tmp_destination);
        if tmp_destination.exists() {
            crate::file::remove_all(&tmp_destination)?;
        }

        match Self::clone_to(repo_structure, &tmp_destination) {
            Ok(()) => {
                if destination.exists()
                    && let Err(e) = crate::file::remove_all(destination)
                {
                    let _ = crate::file::remove_all(&tmp_destination);
                    return Err(e);
                }
                if let Err(e) = std::fs::rename(&tmp_destination, destination) {
                    let _ = crate::file::remove_all(&tmp_destination);
                    return Err(eyre::eyre!(
                        "failed to move cloned repo into cache at {}: {e}",
                        display_path(destination)
                    ));
                }
            }
            Err(e) => {
                let _ = crate::file::remove_all(&tmp_destination);
                return Err(e);
            }
        }

        Self::prepare_remote_path(&full_path)?;
        Ok(full_path)
    }
}

impl From<RemoteGitSource> for GitRepoStructure {
    fn from(source: RemoteGitSource) -> Self {
        GitRepoStructure::new(&source.url, &source.path, source.git_ref)
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskGit {
    fn is_match(&self, file: &str) -> bool {
        RemoteSource::parse_git(file).is_some()
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        let repo_structure = self.get_repo_structure(file);
        let cache_key = self.get_cache_key(&repo_structure);
        let destination = self.storage_path.join(&cache_key);
        self.fetch_to_destination(&repo_structure, &destination, self.is_cached)
    }

    async fn get_local_artifact(&self, file: &str) -> Result<TaskFileArtifact> {
        if self.is_cached {
            return Ok(TaskFileArtifact::persistent(
                self.get_local_path(file).await?,
            ));
        }
        let repo_structure = self.get_repo_structure(file);
        let cache_key = self.get_cache_key(&repo_structure);
        let artifact_root = self.unique_artifact_root(&cache_key)?;
        let artifact = TaskFileArtifact::temporary(artifact_root.clone(), artifact_root.clone());
        Self::clone_to(&repo_structure, &artifact_root)?;
        let path = artifact_root.join(repo_structure.path);
        Self::prepare_remote_path(&path)?;
        Ok(artifact.with_path(path))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn parse_ssh(file: &str) -> Option<GitRepoStructure> {
        RemoteSource::parse_git_ssh(file).map(Into::into)
    }

    fn parse_https(file: &str) -> Option<GitRepoStructure> {
        RemoteSource::parse_git_https(file).map(Into::into)
    }

    #[test]
    fn test_unique_artifact_root_remains_reserved_for_clone() {
        let storage = tempfile::tempdir().unwrap();
        let provider = RemoteTaskGit {
            storage_path: storage.path().to_path_buf(),
            is_cached: false,
        };

        let artifact_root = provider.unique_artifact_root("cache-key").unwrap();

        assert!(artifact_root.is_dir());
        assert_eq!(std::fs::read_dir(&artifact_root).unwrap().count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn test_prepare_remote_path_makes_non_executable_file_executable() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let task_file = temp_dir.path().join("task");
        fs::write(&task_file, "#!/usr/bin/env bash\necho ok\n").unwrap();
        fs::set_permissions(&task_file, fs::Permissions::from_mode(0o644)).unwrap();

        RemoteTaskGit::prepare_remote_path(&task_file).unwrap();

        assert!(file::is_executable(&task_file));
    }

    #[test]
    #[cfg(unix)]
    fn test_prepare_remote_path_rejects_symlink_without_modifying_target() {
        use std::fs;
        use std::os::unix::fs::{PermissionsExt, symlink};

        let temp_dir = tempfile::tempdir().unwrap();
        let target = temp_dir.path().join("target");
        let task_file = temp_dir.path().join("task");
        fs::write(&target, "#!/usr/bin/env bash\necho ok\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&target, &task_file).unwrap();

        let error = RemoteTaskGit::prepare_remote_path(&task_file).unwrap_err();

        assert!(error.to_string().contains("not a regular file"));
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn test_prepare_remote_path_allows_task_include_directory() {
        let temp_dir = tempfile::tempdir().unwrap();

        RemoteTaskGit::prepare_remote_path(&temp_dir.path().to_path_buf()).unwrap();
    }

    #[test]
    fn test_valid_parse_ssh() {
        let test_cases = vec![
            "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::ssh://git@github.com/myorg/example.git//terraform/myfile?ref=master",
            "git::ssh://git@git.acme.com:1222/myorg/example.git//terraform/myfile?ref=master",
            "git::ssh://git@myserver.com/example.git//terraform/myfile",
            "git::ssh://user@myserver.com/example.git//myfile?ref=master",
            "git::ssh://myserver.com/example.git//myfile?ref=master",
            "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
            "git::ssh://git@dev.azure/org/project/_git/example//terraform/myfile?ref=master",
            "git::ssh://git@dev.azure:1222/org/project/_git/example//terraform/myfile?ref=master",
            "git::ssh://git@dev.azure/org/project/_git/example//terraform/myfile",
            "git::ssh://user@dev.azure/org/project/_git/example//myfile?ref=master",
            "git::ssh://dev.azure/org/project/_git/example//myfile?ref=master",
            "git::git@ssh.dev.azure.com:v3/org/project/example//myfile?ref=v1.0.0",
            "git::git@ssh.dev.azure.com:v3/org/project/example//terraform/myfile?ref=master",
            "git::git@ssh.dev.azure.com:v3/org/project/example//terraform/myfile",
        ];

        for url in test_cases {
            assert!(parse_ssh(url).is_some(), "Failed for: {}", url);
        }
    }

    #[test]
    fn test_invalid_parse_ssh() {
        let test_cases = vec![
            "git::ssh://user@myserver.com/example.git?ref=master",
            "git::ssh://user@myserver.com/example.git",
            "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::ssh://user@dev.azure/org/project/_git/example?ref=master",
            "git::ssh://user@dev.azure/org/project/_git/example",
            "git::git@ssh.dev.azure.com:v3/org/project/example?ref=master",
            "git::git@ssh.dev.azure.com:v3/org/project/example",
            "git::https://dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
        ];

        for url in test_cases {
            assert!(parse_ssh(url).is_none(), "Should fail for: {}", url);
        }
    }

    #[test]
    fn test_valid_parse_https() {
        let test_cases = vec![
            "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::https://github.com/myorg/example.git//terraform/myfile?ref=master",
            "git::https://git.acme.com:8080/myorg/example.git//terraform/myfile?ref=master",
            "git::https://myserver.com/example.git//terraform/myfile",
            "git::https://myserver.com/example.git//myfile?ref=master",
            "git::http://localhost:8080/repo.git//xtasks/lint/remote-task", // HTTP support for local testing
            "git::https://dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
            "git::https://dev.azure/org/project/_git/example//terraform/myfile?ref=master",
            "git::https://dev.azure:8080/org/project/_git/example//terraform/myfile?ref=master",
            "git::https://dev.azure/org/project/_git/example//terraform/myfile",
            "git::https://dev.azure/org/project/_git/example//myfile?ref=master",
            "git::http://localhost:8080/org/project/_git/example//xtasks/lint/remote-task", // HTTP support for local testing
        ];

        for url in test_cases {
            assert!(parse_https(url).is_some(), "Failed for: {}", url);
        }
    }

    #[test]
    fn test_invalid_parse_https() {
        let test_cases = vec![
            "git::https://myserver.com/example.git?ref=master",
            "git::https://user@myserver.com/example.git",
            "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::https://dev.azure/org/project/_git/example?ref=master",
            "git::https://user@dev.azure/org/project/_git/example",
            "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
            "git::git@ssh.dev.azure.com:v3/org/project/example//myfile?ref=v1.0.0",
        ];

        for url in test_cases {
            assert!(parse_https(url).is_none(), "Should fail for: {}", url);
        }
    }

    #[test]
    fn test_extract_ssh_url_information() {
        let test_cases: Vec<(&str, &str, &str, Option<String>)> = vec![
            (
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
                "ssh://git@github.com/myorg/example.git",
                "myfile",
                Some("v1.0.0".to_string()),
            ),
            (
                "git::ssh://git@github.com/myorg/example.git//terraform/myfile?ref=master",
                "ssh://git@github.com/myorg/example.git",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::ssh://git@myserver.com/example.git//terraform/myfile",
                "ssh://git@myserver.com/example.git",
                "terraform/myfile",
                None,
            ),
            (
                "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "ssh://git@dev.azure/org/project/_git/example",
                "myfile",
                Some("v1.0.0".to_string()),
            ),
            (
                "git::ssh://git@dev.azure/org/project/_git/example//terraform/myfile?ref=master",
                "ssh://git@dev.azure/org/project/_git/example",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::ssh://git@dev.azure/org/project/_git/example//terraform/myfile",
                "ssh://git@dev.azure/org/project/_git/example",
                "terraform/myfile",
                None,
            ),
            (
                "git::git@ssh.dev.azure.com:v3/org/project/example//myfile?ref=v1.0.0",
                "git@ssh.dev.azure.com:v3/org/project/example",
                "myfile",
                Some("v1.0.0".to_string()),
            ),
            (
                "git::git@ssh.dev.azure.com:v3/org/project/example//terraform/myfile?ref=master",
                "git@ssh.dev.azure.com:v3/org/project/example",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::git@ssh.dev.azure.com:v3/org/project/example//terraform/myfile",
                "git@ssh.dev.azure.com:v3/org/project/example",
                "terraform/myfile",
                None,
            ),
        ];

        for (url, expected_repo, expected_path, expected_branch) in test_cases {
            let repo = parse_ssh(url).unwrap();
            assert_eq!(expected_repo, repo.url_without_path);
            assert_eq!(expected_path, repo.path);
            assert_eq!(expected_branch, repo.branch);
        }
    }

    #[test]
    fn test_extract_https_url_information() {
        let test_cases: Vec<(&str, &str, &str, Option<String>)> = vec![
            (
                "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
                "https://github.com/myorg/example.git",
                "myfile",
                Some("v1.0.0".to_string()),
            ),
            (
                "git::https://github.com/myorg/example.git//terraform/myfile?ref=master",
                "https://github.com/myorg/example.git",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::https://myserver.com/example.git//terraform/myfile",
                "https://myserver.com/example.git",
                "terraform/myfile",
                None,
            ),
            (
                "git::https://dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "https://dev.azure/org/project/_git/example",
                "myfile",
                Some("v1.0.0".to_string()),
            ),
            (
                "git::https://dev.azure/org/project/_git/example//terraform/myfile?ref=master",
                "https://dev.azure/org/project/_git/example",
                "terraform/myfile",
                Some("master".to_string()),
            ),
            (
                "git::https://dev.azure/org/project/_git/example//terraform/myfile",
                "https://dev.azure/org/project/_git/example",
                "terraform/myfile",
                None,
            ),
        ];

        for (url, expected_repo, expected_path, expected_branch) in test_cases {
            let repo = parse_https(url).unwrap();
            assert_eq!(expected_repo, repo.url_without_path);
            assert_eq!(expected_path, repo.path);
            assert_eq!(expected_branch, repo.branch);
        }
    }

    #[test]
    fn test_compare_ssh_get_cache_key() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            (
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v2.0.0",
                false,
            ),
            (
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://user@myserver.com/example.git//myfile?ref=master",
                false,
            ),
            (
                "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v2.0.0",
                false,
            ),
            (
                "git::ssh://git@github.com/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/example.git//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
            (
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/myorg/example.git//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
            (
                "git::ssh://git@dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "git::ssh://git@dev.azure/org/project/_git/example//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
        ];

        for (first_url, second_url, expected) in test_cases {
            let first_repo = parse_ssh(first_url).unwrap();
            let second_repo = parse_ssh(second_url).unwrap();
            let first_cache_key = remote_task_git.get_cache_key(&first_repo);
            let second_cache_key = remote_task_git.get_cache_key(&second_repo);
            assert_eq!(expected, first_cache_key == second_cache_key);
        }
    }

    #[test]
    fn test_compare_https_get_cache_key() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            (
                "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::https://github.com/myorg/example.git//myfile?ref=v2.0.0",
                false,
            ),
            (
                "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::https://bitbucket.com/myorg/example.git//myfile?ref=v1.0.0",
                false,
            ),
            (
                "git::https://dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "git::https://dev.azure/org/project/_git/example//myfile?ref=v2.0.0",
                false,
            ),
            (
                "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::https://github.com/myorg/example.git//subfolder/myfile?ref=v1.0.0",
                true,
            ),
            (
                "git::https://github.com/example.git//myfile?ref=v1.0.0",
                "git::https://github.com/example.git//subfolder/myfile?ref=v1.0.0",
                true,
            ),
            (
                "git::https://dev.azure/org/project/_git/example//myfile?ref=v1.0.0",
                "git::https://dev.azure/org/project/_git/example//subfolder/myfile?ref=v1.0.0",
                true,
            ),
        ];

        for (first_url, second_url, expected) in test_cases {
            let first_repo = parse_https(first_url).unwrap();
            let second_repo = parse_https(second_url).unwrap();
            let first_cache_key = remote_task_git.get_cache_key(&first_repo);
            let second_cache_key = remote_task_git.get_cache_key(&second_repo);
            assert_eq!(expected, first_cache_key == second_cache_key);
        }
    }
}
