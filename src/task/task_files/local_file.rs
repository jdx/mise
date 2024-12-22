use super::SourceType;

pub struct LocalFile {}

impl SourceType for LocalFile {
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(LocalFile {})
    }
}
