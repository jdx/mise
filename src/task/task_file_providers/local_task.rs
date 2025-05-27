use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::Result;

use super::TaskFileProvider;

#[derive(Debug)]
pub struct LocalTask;

#[async_trait]
impl TaskFileProvider for LocalTask {
    fn is_match(&self, file: &str) -> bool {
        let path = Path::new(file);

        path.is_relative() || path.is_absolute()
    }

    async fn get_local_path(&self, file: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(file))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_is_match() {
        let provider = LocalTask;
        assert!(provider.is_match("filetask.bat"));
        assert!(provider.is_match("filetask"));
        assert!(provider.is_match("/test.txt"));
        assert!(provider.is_match("./test.txt"));
        assert!(provider.is_match("../test.txt"));
    }

    #[tokio::test]
    async fn test_get_local_path() {
        let provider = LocalTask;
        assert_eq!(
            provider.get_local_path("/test.txt").await.unwrap(),
            PathBuf::from("/test.txt")
        );
        assert_eq!(
            provider.get_local_path("./test.txt").await.unwrap(),
            PathBuf::from("./test.txt")
        );
        assert_eq!(
            provider.get_local_path("../test.txt").await.unwrap(),
            PathBuf::from("../test.txt")
        );
    }
}
