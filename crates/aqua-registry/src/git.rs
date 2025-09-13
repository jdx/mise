use eyre::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Git repository manager that shells out to the git command
pub struct GitRepo {
    path: PathBuf,
}

impl GitRepo {
    /// Create a new GitRepo instance for the given path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Check if the directory exists and contains a .git directory
    pub fn exists(&self) -> bool {
        self.path.exists() && self.path.join(".git").exists()
    }

    /// Clone a repository to the path
    pub fn clone(&self, url: &str) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let output = Command::new("git")
            .args(["clone", url])
            .arg(&self.path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eyre::bail!("Failed to clone git repository: {}", stderr);
        }

        Ok(())
    }

    /// Fetch the latest changes from the remote
    pub fn fetch(&self) -> Result<()> {
        if !self.exists() {
            eyre::bail!("Repository does not exist: {:?}", self.path);
        }

        let output = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eyre::bail!("Failed to fetch git repository: {}", stderr);
        }

        Ok(())
    }

    /// Reset to the latest commit on the default branch
    pub fn reset_to_latest(&self) -> Result<()> {
        if !self.exists() {
            eyre::bail!("Repository does not exist: {:?}", self.path);
        }

        // Get the default branch name
        let output = Command::new("git")
            .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
            .current_dir(&self.path)
            .output()?;

        let default_branch = if output.status.success() {
            let branch_ref = String::from_utf8_lossy(&output.stdout);
            branch_ref
                .trim()
                .strip_prefix("refs/remotes/origin/")
                .unwrap_or("main")
                .to_string()
        } else {
            // Fallback to main/master
            "main".to_string()
        };

        // Reset to the latest commit on the default branch
        let output = Command::new("git")
            .args(["reset", "--hard", &format!("origin/{}", default_branch)])
            .current_dir(&self.path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eyre::bail!("Failed to reset git repository: {}", stderr);
        }

        Ok(())
    }

    /// Update the repository (fetch + reset)
    pub fn update(&self) -> Result<()> {
        self.fetch()?;
        self.reset_to_latest()?;
        Ok(())
    }

    /// Find registry.yaml files in the repository
    pub fn find_registry_files(&self) -> Result<Vec<PathBuf>> {
        if !self.exists() {
            return Ok(Vec::new());
        }

        let mut registry_files = Vec::new();

        // Check for main registry.yaml in root
        let main_registry = self.path.join("registry.yaml");
        if main_registry.exists() {
            registry_files.push(main_registry);
        }

        // Check for pkgs/**/registry.yaml files
        let pkgs_dir = self.path.join("pkgs");
        if pkgs_dir.exists() {
            registry_files.extend(self.find_registry_files_in_dir(&pkgs_dir)?);
        }

        Ok(registry_files)
    }

    /// Recursively find registry.yaml files in a directory
    fn find_registry_files_in_dir(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        Self::find_registry_files_in_dir_impl(dir)
    }

    /// Implementation of recursive registry file search
    fn find_registry_files_in_dir_impl(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                // Look for registry.yaml in this subdirectory
                let registry_file = path.join("registry.yaml");
                if registry_file.exists() {
                    files.push(registry_file);
                }
                // Recursively search subdirectories
                files.extend(Self::find_registry_files_in_dir_impl(&path)?);
            }
        }

        Ok(files)
    }

    /// Get the path to the repository
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Clone or update a git registry and return the registry files
pub fn clone_or_update_registry(url: &str, cache_dir: &Path) -> Result<Vec<PathBuf>> {
    let repo = GitRepo::new(cache_dir);

    if repo.exists() {
        // Repository exists, update it
        repo.update()?;
    } else {
        // Repository doesn't exist, clone it
        repo.clone(url)?;
    }

    // Find all registry.yaml files
    repo.find_registry_files()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_git_repo_not_exists() {
        let temp_dir = std::env::temp_dir().join("test-git-repo-not-exists");
        let repo = GitRepo::new(&temp_dir);
        assert!(!repo.exists());
    }

    #[test]
    fn test_find_registry_files_empty_dir() {
        let temp_dir = std::env::temp_dir().join("test-find-registry-files-empty");
        fs::create_dir_all(&temp_dir).unwrap();

        let repo = GitRepo::new(&temp_dir);
        let files = repo.find_registry_files().unwrap();
        assert!(files.is_empty());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_find_registry_files_with_files() {
        let temp_dir = std::env::temp_dir().join("test-find-registry-files-with-files");
        fs::create_dir_all(&temp_dir).unwrap();

        // Create a fake .git directory
        fs::create_dir_all(temp_dir.join(".git")).unwrap();

        // Create registry.yaml in root
        fs::write(temp_dir.join("registry.yaml"), "packages: []").unwrap();

        // Create pkgs structure
        let pkgs_dir = temp_dir.join("pkgs");
        fs::create_dir_all(&pkgs_dir).unwrap();

        let tool_dir = pkgs_dir.join("some-tool");
        fs::create_dir_all(&tool_dir).unwrap();
        fs::write(tool_dir.join("registry.yaml"), "packages: []").unwrap();

        let repo = GitRepo::new(&temp_dir);
        assert!(repo.exists());

        let files = repo.find_registry_files().unwrap();
        assert_eq!(files.len(), 2);

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_clone_or_update_registry_invalid_url() {
        let temp_dir = std::env::temp_dir().join("test-clone-invalid-url");

        // This should fail gracefully for an invalid URL
        let result =
            clone_or_update_registry("https://invalid-git-url.example.com/repo.git", &temp_dir);
        assert!(result.is_err());

        // Cleanup
        fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_git_repo_path() {
        let expected_path = PathBuf::from("/test/path");
        let repo = GitRepo::new(&expected_path);
        assert_eq!(repo.path(), expected_path);
    }
}
