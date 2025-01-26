use std::path::PathBuf;

use regex::Regex;
use xx::git;

use crate::{dirs, env, hash};

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
    url: String,
    url_without_path: String,
    path: String,
}

impl GitRepoStructure {
    pub fn new(url: &str, url_without_path: &str, path: &str) -> Self {
        Self {
            url: url.to_string(),
            url_without_path: url_without_path.to_string(),
            path: path.to_string(),
        }
    }
}

impl RemoteTaskGit {
    fn get_cache_key(&self, repo_structure: &GitRepoStructure) -> String {
        hash::hash_sha256_to_str(&repo_structure.url_without_path)
    }

    fn get_repo_structure(&self, file: &str) -> GitRepoStructure {
        if self.detect_ssh(file).is_ok() {
            return self.detect_ssh(file).unwrap();
        }
        self.detect_https(file).unwrap()
    }

    fn detect_ssh(&self, file: &str) -> Result<GitRepoStructure, Box<dyn std::error::Error>> {
        let re = Regex::new(r"^git::ssh://((?P<user>[^@]+)@)(?P<host>[^/]+)/(?P<repo>[^/]+)\.git//(?P<path>[^?]+)(\?(?P<query>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err("Invalid SSH URL".into());
        }

        let captures = re.captures(file).unwrap();

        let path = captures.name("path").unwrap().as_str();

        Ok(GitRepoStructure::new(file, &file.replace(path, ""), path))
    }

    fn detect_https(&self, file: &str) -> Result<GitRepoStructure, Box<dyn std::error::Error>> {
        let re = Regex::new(r"^git::https://(?P<host>[^/]+)/(?P<repo>[^/]+(?:/[^/]+)?)\.git//(?P<path>[^?]+)(\?(?P<query>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err("Invalid HTTPS URL".into());
        }

        let captures = re.captures(file).unwrap();

        let path = captures.name("path").unwrap().as_str();

        Ok(GitRepoStructure::new(file, &file.replace(path, ""), path))
    }
}

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

    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let repo_structure = self.get_repo_structure(file);
        let cache_key = self.get_cache_key(&repo_structure);
        let destination = self.storage_path.join(&cache_key);
        let repo_file_path = repo_structure.path.clone();
        let full_path = destination.join(&repo_file_path);

        match self.is_cached {
            true => {
                trace!("Cache mode enabled");

                if full_path.exists() {
                    return Ok(full_path);
                }
            }
            false => {
                trace!("Cache mode disabled");

                if full_path.exists() {
                    crate::file::remove_dir(full_path)?;
                }
            }
        }

        let git_cloned = git::clone(repo_structure.url.as_str(), destination)?;

        Ok(git_cloned.dir.join(&repo_file_path))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_valid_detect_ssh() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            "git::ssh://git@github.com:myorg/example.git//myfile?ref=v1.0.0",
            "git::ssh://git@github.com:myorg/example.git//terraform/myfile?ref=master",
            "git::ssh://git@github.com:myorg/example.git//terraform/myfile?depth=1",
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
            "git::https://github.com/myorg/example.git//terraform/myfile?depth=1",
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
            "git::ssh://git@github.com:myorg/example.git//myfile?ref=v1.0.0",
        ];

        for url in test_cases {
            let result = remote_task_git.detect_https(url);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_compare_ssh_get_cache_key() {
        let remote_task_git = RemoteTaskGitBuilder::new().build();

        let test_cases = vec![
            (
                "git::ssh://git@github.com:myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com:myorg/example.git//myfile?ref=v2.0.0",
                false,
            ),
            (
                "git::ssh://git@github.com:myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://user@myserver.com/example.git//myfile?ref=master",
                false,
            ),
            (
                "git::ssh://git@github.com/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com/example.git//subfolder/mysecondfile?ref=v1.0.0",
                true,
            ),
            (
                "git::ssh://git@github.com:myorg/example.git//myfile?ref=v1.0.0",
                "git::ssh://git@github.com:myorg/example.git//subfolder/mysecondfile?ref=v1.0.0",
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
