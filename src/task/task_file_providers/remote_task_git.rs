use crate::Result;
use std::path::PathBuf;

use async_trait::async_trait;
use eyre::eyre;
use regex::Regex;

use crate::{
    dirs, env,
    git::{self, CloneOptions},
    hash,
};

use super::TaskFileProvider;

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
    fn get_cache_key(&self, repo_structure: &GitRepoStructure) -> String {
        let key = format!(
            "{}{}",
            &repo_structure.url_without_path,
            &repo_structure.branch.to_owned().unwrap_or("".to_string())
        );
        hash::hash_sha256_to_str(&key)
    }

    fn get_repo_structure(&self, file: &str) -> GitRepoStructure {
        if self.detect_ssh(file).is_ok() {
            return self.detect_ssh(file).unwrap();
        }
        self.detect_https(file).unwrap()
    }

    fn detect_ssh(&self, file: &str) -> Result<GitRepoStructure> {
        let re = Regex::new(r"^git::(?P<url>ssh://((?P<user>[^@]+)@)(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<branch>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err(eyre!("Invalid SSH URL"));
        }

        let captures = re.captures(file).unwrap();

        let url_without_path = captures.name("url").unwrap().as_str();

        let path = captures.name("path").unwrap().as_str();

        let branch: Option<String> = captures.name("branch").map(|m| m.as_str().to_string());

        Ok(GitRepoStructure::new(url_without_path, path, branch))
    }

    fn detect_https(&self, file: &str) -> Result<GitRepoStructure> {
        let re = Regex::new(r"^git::(?P<url>https://(?P<host>[^/]+)/(?P<repo>.+)\.git)//(?P<path>[^?]+)(\?ref=(?P<branch>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err(eyre!("Invalid HTTPS URL"));
        }

        let captures = re.captures(file).unwrap();

        let url_without_path = captures.name("url").unwrap().as_str();

        let path = captures.name("path").unwrap().as_str();

        let branch: Option<String> = captures.name("branch").map(|m| m.as_str().to_string());

        Ok(GitRepoStructure::new(url_without_path, path, branch))
    }
}

#[async_trait]
impl TaskFileProvider for RemoteTaskGit {
    fn is_match(&self, file: &str) -> bool {
        if self.detect_ssh(file).is_ok() {
            return true;
        }

        if self.detect_https(file).is_ok() {
            return true;
        }

        false
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        let repo_structure = self.get_repo_structure(file);
        let cache_key = self.get_cache_key(&repo_structure);
        let destination = self.storage_path.join(&cache_key);
        let repo_file_path = repo_structure.path.clone();
        let full_path = destination.join(&repo_file_path);

        debug!("Repo structure: {:?}", repo_structure);

        match self.is_cached {
            true => {
                trace!("Cache mode enabled");

                if full_path.exists() {
                    debug!("Using cached file: {:?}", full_path);
                    return Ok(full_path);
                }
            }
            false => {
                trace!("Cache mode disabled");

                if full_path.exists() {
                    crate::file::remove_all(&destination)?;
                }
            }
        }

        let git_repo = git::Git::new(destination);

        let mut clone_options = CloneOptions::default();

        if let Some(branch) = &repo_structure.branch {
            trace!("Use specific branch {}", branch);
            clone_options = clone_options.branch(branch);
        }

        git_repo.clone(repo_structure.url_without_path.as_str(), clone_options)?;

        Ok(full_path)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_valid_detect_ssh() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::ssh://git@github.com/myorg/example.git//terraform/myfile?ref=master",
            "git::ssh://git@git.acme.com:1222/myorg/example.git//terraform/myfile?ref=master",
            "git::ssh://git@myserver.com/example.git//terraform/myfile",
            "git::ssh://user@myserver.com/example.git//myfile?ref=master",
        ];

        for url in test_cases {
            let result = remote_task_git.detect_ssh(url);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_invalid_detect_ssh() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            "git::ssh://myserver.com/example.git//myfile?ref=master",
            "git::ssh://user@myserver.com/example.git?ref=master",
            "git::ssh://user@myserver.com/example.git",
            "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
        ];

        for url in test_cases {
            let result = remote_task_git.detect_ssh(url);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_valid_detect_https() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
            "git::https://github.com/myorg/example.git//terraform/myfile?ref=master",
            "git::https://git.acme.com:8080/myorg/example.git//terraform/myfile?ref=master",
            "git::https://myserver.com/example.git//terraform/myfile",
            "git::https://myserver.com/example.git//myfile?ref=master",
        ];

        for url in test_cases {
            let result = remote_task_git.detect_https(url);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_invalid_detect_https() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            "git::https://myserver.com/example.git?ref=master",
            "git::https://user@myserver.com/example.git",
            "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
        ];

        for url in test_cases {
            let result = remote_task_git.detect_https(url);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_extract_ssh_url_information() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

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
        ];

        for (url, expected_repo, expected_path, expected_branch) in test_cases {
            let repo = remote_task_git.detect_ssh(url).unwrap();
            assert_eq!(expected_repo, repo.url_without_path);
            assert_eq!(expected_path, repo.path);
            assert_eq!(expected_branch, repo.branch);
        }
    }

    #[test]
    fn test_extract_https_url_information() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

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
        ];

        for (url, expected_repo, expected_path, expected_branch) in test_cases {
            let repo = remote_task_git.detect_https(url).unwrap();
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
                "git::ssh://git@github.com/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/example.git//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
            (
                "git::ssh://git@github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/myorg/example.git//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
        ];

        for (first_url, second_url, expected) in test_cases {
            let first_repo = remote_task_git.detect_ssh(first_url).unwrap();
            let second_repo = remote_task_git.detect_ssh(second_url).unwrap();
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
                "git::https://github.com/myorg/example.git//myfile?ref=v1.0.0",
                "git::https://github.com/myorg/example.git//subfolder/myfile?ref=v1.0.0",
                true,
            ),
            (
                "git::https://github.com/example.git//myfile?ref=v1.0.0",
                "git::https://github.com/example.git//subfolder/myfile?ref=v1.0.0",
                true,
            ),
        ];

        for (first_url, second_url, expected) in test_cases {
            let first_repo = remote_task_git.detect_https(first_url).unwrap();
            let second_repo = remote_task_git.detect_https(second_url).unwrap();
            let first_cache_key = remote_task_git.get_cache_key(&first_repo);
            let second_cache_key = remote_task_git.get_cache_key(&second_repo);
            assert_eq!(expected, first_cache_key == second_cache_key);
        }
    }
}
