use super::SourceType;

pub struct S3File {}

impl SourceType for S3File {
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(S3File {})
    }
}