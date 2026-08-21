use crate::Result;
use std::path::{Path, PathBuf};

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
    pub(super) fn validate_remote_path(
        checkout_root: &Path,
        path: &Path,
    ) -> Result<std::fs::Metadata> {
        let metadata = path.symlink_metadata()?;
        let checkout_root = checkout_root.canonicalize()?;
        let canonical_path = path.canonicalize()?;
        if !canonical_path.starts_with(&checkout_root) {
            eyre::bail!(
                "remote task path escapes its Git checkout: {}",
                display_path(path)
            );
        }
        if metadata.file_type().is_file() || metadata.file_type().is_dir() {
            return Ok(metadata);
        }
        eyre::bail!(
            "remote task path is not a regular file or directory: {}",
            display_path(path)
        )
    }

    /// Make fetched task files executable while leaving task include directories intact.
    fn prepare_remote_path(checkout_root: &Path, path: &Path) -> Result<()> {
        let metadata = Self::validate_remote_path(checkout_root, path)?;
        if metadata.file_type().is_file() {
            file::make_executable(path)?;
        }
        Ok(())
    }

    fn get_cache_key(&self, repo_structure: &GitRepoStructure) -> String {
        let key = format!(
            "{}\0{}",
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

    fn path_cache_destination(&self, cache_key: &str, repo_path: &str) -> PathBuf {
        // Hash the complete identity into one component so gix temporary pack
        // paths remain below the legacy Windows path limit.
        let path_key = hash::hash_sha256_to_str(&format!("{cache_key}\0{repo_path}"));
        self.storage_path.join(format!("path-{path_key}"))
    }

    fn fetch_to_destination(
        &self,
        repo_structure: &GitRepoStructure,
        destination: &PathBuf,
        reuse_existing: bool,
    ) -> Result<Option<PathBuf>> {
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

        let destination_metadata = destination.symlink_metadata().ok();
        let published_directory = destination_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_dir());
        if reuse_existing && published_directory && full_path.exists() {
            debug!("Using cached file: {:?}", full_path);
            Self::prepare_remote_path(destination, &full_path)?;
            return Ok(Some(full_path));
        }
        if reuse_existing && destination_metadata.is_some() {
            return Ok(None);
        }

        let mut tmp_destination = destination.as_os_str().to_os_string();
        tmp_destination.push(".clone-tmp");
        let tmp_destination = PathBuf::from(tmp_destination);
        if tmp_destination.exists() {
            crate::file::remove_all(&tmp_destination)?;
        }

        if let Err(err) = Self::clone_to(repo_structure, &tmp_destination) {
            let _ = crate::file::remove_all(&tmp_destination);
            return Err(err);
        }

        let tmp_full_path = tmp_destination.join(&repo_file_path);
        if let Err(err) = Self::prepare_remote_path(&tmp_destination, &tmp_full_path) {
            let _ = crate::file::remove_all(&tmp_destination);
            return Err(err);
        }

        let destination_metadata = destination.symlink_metadata().ok();
        let published_directory = destination_metadata
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_dir());
        if destination_metadata.is_some() {
            let _ = crate::file::remove_all(&tmp_destination);
            if reuse_existing && published_directory && full_path.exists() {
                Self::prepare_remote_path(destination, &full_path)?;
                return Ok(Some(full_path));
            }
            return Ok(None);
        }

        if let Err(err) = file::rename(&tmp_destination, destination) {
            let _ = crate::file::remove_all(&tmp_destination);
            return Err(eyre::eyre!(
                "failed to move cloned repo into cache at {}: {err}",
                display_path(destination)
            ));
        }

        Ok(Some(full_path))
    }

    fn get_cached_path(&self, repo_structure: &GitRepoStructure) -> Result<PathBuf> {
        let cache_key = self.get_cache_key(repo_structure);
        let destination = self.storage_path.join(&cache_key);
        if let Some(path) = self.fetch_to_destination(repo_structure, &destination, true)? {
            return Ok(path);
        }

        // Never replace a published repo tree that another process may still
        // be reading. A path-specific snapshot handles paths added later.
        let destination = self.path_cache_destination(&cache_key, &repo_structure.path);
        self.fetch_to_destination(repo_structure, &destination, true)?
            .ok_or_else(|| {
                eyre::eyre!(
                    "remote git task cache at {} does not contain {}",
                    display_path(&destination),
                    repo_structure.path
                )
            })
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
        if self.is_cached {
            return self.get_cached_path(&repo_structure);
        }
        let cache_key = self.get_cache_key(&repo_structure);
        let destination = self.unique_artifact_root(&cache_key)?;
        self.fetch_to_destination(&repo_structure, &destination, false)?
            .ok_or_else(|| eyre::eyre!("failed to publish remote git task snapshot"))
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
        Self::prepare_remote_path(&artifact_root, &path)?;
        Ok(artifact.with_path(path))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::process::Command;

    fn run_git(repo: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn test_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        run_git(repo.path(), &["init"]);
        run_git(repo.path(), &["config", "user.name", "Mise Test"]);
        run_git(
            repo.path(),
            &["config", "user.email", "mise-test@example.com"],
        );
        repo
    }

    fn commit_file(repo: &std::path::Path, path: &str, body: &str) {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        run_git(repo, &["add", "."]);
        run_git(repo, &["commit", "-m", "update task fixture"]);
    }

    #[test]
    fn test_cached_missing_path_preserves_published_repo_tree() {
        let source = test_repo();
        commit_file(source.path(), "tasks/first", "first revision\n");
        let source_url = url::Url::from_directory_path(source.path())
            .unwrap()
            .to_string();
        let storage = tempfile::tempdir().unwrap();
        let provider = RemoteTaskGit {
            storage_path: storage.path().to_path_buf(),
            is_cached: true,
        };

        let first_repo = GitRepoStructure::new(&source_url, "tasks/first", None);
        let first_path = provider.get_cached_path(&first_repo).unwrap();
        let cache_key = provider.get_cache_key(&first_repo);
        let published_repo = storage.path().join(&cache_key);
        let reader_marker = published_repo.join("reader-marker");
        std::fs::write(&reader_marker, "live reader state\n").unwrap();

        commit_file(source.path(), "tasks/second", "second revision\n");
        let second_repo = GitRepoStructure::new(&source_url, "tasks/second", None);
        let second_path = provider.get_cached_path(&second_repo).unwrap();

        assert_eq!(
            std::fs::read_to_string(&first_path)
                .unwrap()
                .replace("\r\n", "\n"),
            "first revision\n"
        );
        assert_eq!(
            std::fs::read_to_string(&reader_marker).unwrap(),
            "live reader state\n"
        );
        assert_eq!(
            std::fs::read_to_string(&second_path)
                .unwrap()
                .replace("\r\n", "\n"),
            "second revision\n"
        );
        assert!(first_path.starts_with(&published_repo));
        assert!(!second_path.starts_with(&published_repo));

        commit_file(source.path(), "tasks/second", "third revision\n");
        let cached_second_path = provider.get_cached_path(&second_repo).unwrap();
        assert_eq!(cached_second_path, second_path);
        assert_eq!(
            std::fs::read_to_string(cached_second_path)
                .unwrap()
                .replace("\r\n", "\n"),
            "second revision\n"
        );
    }

    #[test]
    fn test_missing_path_does_not_publish_invalid_cache_tree() {
        let source = test_repo();
        commit_file(source.path(), "tasks/first", "first revision\n");
        let source_url = url::Url::from_directory_path(source.path())
            .unwrap()
            .to_string();
        let storage = tempfile::tempdir().unwrap();
        let provider = RemoteTaskGit {
            storage_path: storage.path().to_path_buf(),
            is_cached: true,
        };
        let missing_repo = GitRepoStructure::new(&source_url, "tasks/missing", None);
        let destination = storage.path().join(provider.get_cache_key(&missing_repo));

        assert!(provider.get_cached_path(&missing_repo).is_err());
        assert!(!destination.exists());
    }

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

        RemoteTaskGit::prepare_remote_path(temp_dir.path(), &task_file).unwrap();

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

        let error = RemoteTaskGit::prepare_remote_path(temp_dir.path(), &task_file).unwrap_err();

        assert!(error.to_string().contains("not a regular file"));
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn test_prepare_remote_path_allows_task_include_directory() {
        let temp_dir = tempfile::tempdir().unwrap();

        RemoteTaskGit::prepare_remote_path(temp_dir.path(), temp_dir.path()).unwrap();
    }

    #[test]
    fn test_prepare_remote_path_rejects_path_outside_checkout() {
        let temp_dir = tempfile::tempdir().unwrap();
        let checkout = temp_dir.path().join("checkout");
        let outside = temp_dir.path().join("outside-task");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(&outside, "#!/usr/bin/env bash\necho outside\n").unwrap();

        let escaped_path = checkout.join("..").join("outside-task");
        let error = RemoteTaskGit::prepare_remote_path(&checkout, &escaped_path).unwrap_err();

        assert!(error.to_string().contains("escapes its Git checkout"));
    }

    #[test]
    #[cfg(windows)]
    fn test_prepare_remote_path_rejects_windows_backslash_escape() {
        let temp_dir = tempfile::tempdir().unwrap();
        let checkout = temp_dir.path().join("checkout");
        let outside = temp_dir.path().join("outside-task");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::write(&outside, "echo outside\n").unwrap();

        let escaped_path = checkout.join(r"..\outside-task");
        let error = RemoteTaskGit::prepare_remote_path(&checkout, &escaped_path).unwrap_err();

        assert!(error.to_string().contains("escapes its Git checkout"));
    }

    #[test]
    #[cfg(unix)]
    fn test_prepare_remote_path_rejects_intermediate_symlink_escape() {
        use std::os::unix::fs::symlink;

        let checkout = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_task = outside.path().join("task");
        std::fs::write(&outside_task, "#!/usr/bin/env bash\necho outside\n").unwrap();
        symlink(outside.path(), checkout.path().join("linked-dir")).unwrap();

        let escaped_path = checkout.path().join("linked-dir/task");
        let error = RemoteTaskGit::prepare_remote_path(checkout.path(), &escaped_path).unwrap_err();

        assert!(error.to_string().contains("escapes its Git checkout"));
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
            // URL and ref need an explicit identity boundary. These otherwise
            // concatenate to the same text.
            (
                "git::https://github.com/example.git//myfile?ref=next.gitmain",
                "git::https://github.com/example.gitnext.git//myfile?ref=main",
                false,
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
