use super::SourceType;

pub struct HttpFile {}

impl SourceType for HttpFile {
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(HttpFile {})
    }
}