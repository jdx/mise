use regex::Regex;

use super::SourceType;

pub struct GitHttpsFile {
    url: String,
    file_path: String,
    ref_name: String,
}

impl SourceType for GitHttpsFile {
    fn from_str(s: &str) -> Result<Self, String> {
        let pattern = Regex::new(r"git::(https://.*?\.git)//(.*)\?ref=(.*)").unwrap();

        if !pattern.is_match(s) {
            return Err("Invalid GitHttpsSource format".to_string());
        }

        let captures = pattern.captures(s).unwrap();
        Ok(GitHttpsFile {
            url: captures[1].to_string(),
            file_path: captures[2].to_string(),
            ref_name: captures[3].to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_invalid_str() {
        let result = GitHttpsFile::from_str("bad");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_valid_str() {
        let result = GitHttpsFile::from_str("git::https://github.com/user/repo.git//path/to/task?ref=main");
        assert!(result.is_ok());
        let source = result.unwrap();
        assert_eq!(source.url, "https://github.com/user/repo.git");
        assert_eq!(source.file_path, "path/to/task");
        assert_eq!(source.ref_name, "main");
    }

}
