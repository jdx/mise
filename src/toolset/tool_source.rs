use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use crate::file::display_path;

/// where a tool version came from (e.g.: .tool-versions)
#[derive(Debug, Clone)]
pub enum ToolSource {
    ToolVersions(PathBuf),
    RtxToml(PathBuf),
    LegacyVersionFile(PathBuf),
    Argument,
    Environment(String, String),
}

impl Display for ToolSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            ToolSource::ToolVersions(path) => write!(f, "{}", display_path(path)),
            ToolSource::RtxToml(path) => write!(f, "{}", display_path(path)),
            ToolSource::LegacyVersionFile(path) => write!(f, "{}", display_path(path)),
            ToolSource::Argument => write!(f, "--runtime"),
            ToolSource::Environment(k, v) => write!(f, "{k}={v}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_tool_source_display() {
        let path = PathBuf::from("/home/user/.test-tool-versions");

        let ts = ToolSource::ToolVersions(path);
        assert_str_eq!(ts.to_string(), "/home/user/.test-tool-versions");

        let ts = ToolSource::RtxToml(PathBuf::from("/home/user/.rtx.toml"));
        assert_str_eq!(ts.to_string(), "/home/user/.rtx.toml");

        let ts = ToolSource::Argument;
        assert_str_eq!(ts.to_string(), "--runtime");

        let ts = ToolSource::Environment("RTX_NODEJS_VERSION".to_string(), "18".to_string());
        assert_str_eq!(ts.to_string(), "RTX_NODEJS_VERSION=18");
    }
}
