use std::path::PathBuf;

use regex::Regex;

use crate::{dirs, env};

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

struct GitRepoStructure {
    url: String,
    user: Option<String>,
    host: String,
    repo: String,
    query: Option<String>,
    path: String,
}

impl GitRepoStructure {
    pub fn new(
        url: &str,
        user: Option<String>,
        host: &str,
        repo: &str,
        query: Option<String>,
        path: &str,
    ) -> Self {
        Self {
            url: url.to_string(),
            user,
            host: host.to_string(),
            repo: repo.to_string(),
            query,
            path: path.to_string(),
        }
    }
}

impl RemoteTaskGit {
    fn get_cache_key(&self, file: &str) -> String {
        "".to_string()
    }

    fn detect_ssh(&self, file: &str) -> Result<GitRepoStructure, Box<dyn std::error::Error>> {
        let re = Regex::new(r"^git::ssh://((?P<user>[^@]+)@)(?P<host>[^/]+)/(?P<repo>[^/]+)\.git//(?P<path>[^?]+)(\?(?P<query>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err("Invalid SSH URL".into());
        }

        let captures = re.captures(file).unwrap();

        Ok(GitRepoStructure::new(
            file,
            Some(captures.name("user").unwrap().as_str().to_string()),
            captures.name("host").unwrap().as_str(),
            captures.name("repo").unwrap().as_str(),
            captures.name("query").map(|m| m.as_str().to_string()),
            captures.name("path").unwrap().as_str(),
        ))
    }

    fn detect_https(&self, file: &str) -> Result<GitRepoStructure, Box<dyn std::error::Error>> {
        let re = Regex::new(r"^git::https://(?P<host>[^/]+)/(?P<repo>[^/]+(?:/[^/]+)?)\.git//(?P<path>[^?]+)(\?(?P<query>[^?]+))?$").unwrap();

        if !re.is_match(file) {
            return Err("Invalid HTTPS URL".into());
        }

        let captures = re.captures(file).unwrap();

        Ok(GitRepoStructure::new(
            file,
            None,
            captures.name("host").unwrap().as_str(),
            captures.name("repo").unwrap().as_str(),
            captures.name("query").map(|m| m.as_str().to_string()),
            captures.name("path").unwrap().as_str(),
        ))
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
        Ok(PathBuf::new())
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
}
