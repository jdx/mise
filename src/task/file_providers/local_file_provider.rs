use std::path::PathBuf;

use super::TaskFileProvider;

pub struct LocalTaskFileProvider;

impl TaskFileProvider for LocalTaskFileProvider {
    fn is_match(&self, file: &str) -> bool {
        file.starts_with("/") || file.starts_with("./") || file.starts_with("../")
    }

    fn get_local_path(
        &self,
        _: &PathBuf,
        file: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(PathBuf::from(file))
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_is_match() {
        let provider = LocalTaskFileProvider;
        assert!(provider.is_match("/test.txt"));
        assert!(provider.is_match("./test.txt"));
        assert!(provider.is_match("../test.txt"));
    }

    #[test]
    fn test_get_local_path() {
        let provider = LocalTaskFileProvider;
        let path = PathBuf::from("/test.txt");
        assert_eq!(
            provider.get_local_path(&path, "/test.txt").unwrap(),
            PathBuf::from("/test.txt")
        );
        assert_eq!(
            provider.get_local_path(&path, "./test.txt").unwrap(),
            PathBuf::from("./test.txt")
        );
        assert_eq!(
            provider.get_local_path(&path, "../test.txt").unwrap(),
            PathBuf::from("../test.txt")
        );
    }
}
