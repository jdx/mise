use std::path::{Path, PathBuf};

use super::TaskFileProvider;

#[derive(Debug)]
pub struct LocalTaskFileProvider;

impl TaskFileProvider for LocalTaskFileProvider {
    fn is_match(&self, file: &str) -> bool {
        let path = Path::new(file);

        path.is_relative() || path.is_absolute()
    }

    fn get_local_path(&self, file: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(PathBuf::from(file))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_is_match() {
        let provider = LocalTaskFileProvider;
        assert!(provider.is_match("filetask.bat"));
        assert!(provider.is_match("filetask"));
        assert!(provider.is_match("/test.txt"));
        assert!(provider.is_match("./test.txt"));
        assert!(provider.is_match("../test.txt"));
    }

    #[test]
    fn test_get_local_path() {
        let provider = LocalTaskFileProvider;
        assert_eq!(
            provider.get_local_path("/test.txt").unwrap(),
            PathBuf::from("/test.txt")
        );
        assert_eq!(
            provider.get_local_path("./test.txt").unwrap(),
            PathBuf::from("./test.txt")
        );
        assert_eq!(
            provider.get_local_path("../test.txt").unwrap(),
            PathBuf::from("../test.txt")
        );
    }
}
