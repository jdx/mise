use super::SourceType;

pub struct GitSshFile {}

impl SourceType for GitSshFile {
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(GitSshFile {})
    }
}