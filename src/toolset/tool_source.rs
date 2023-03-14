use std::fmt::{Display, Formatter};
use std::path::PathBuf;

use indexmap::{indexmap, IndexMap};
use serde_derive::Serialize;

use crate::file::display_path;

/// where a tool version came from (e.g.: .tool-versions)
#[derive(Debug, Clone, Serialize)]
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

impl ToolSource {
    pub fn as_json(&self) -> IndexMap<String, String> {
        match self {
            ToolSource::ToolVersions(path) => indexmap! {
                "type".to_string() => ".tool-versions".to_string(),
                "path".to_string() => path.to_string_lossy().to_string(),
            },
            ToolSource::RtxToml(path) => indexmap! {
                "type".to_string() => ".rtx.toml".to_string(),
                "path".to_string() => path.to_string_lossy().to_string(),
            },
            ToolSource::LegacyVersionFile(path) => indexmap! {
                "type".to_string() => "legacy-version-file".to_string(),
                "path".to_string() => path.to_string_lossy().to_string(),
            },
            ToolSource::Argument => indexmap! {
                "type".to_string() => "argument".to_string(),
            },
            ToolSource::Environment(key, value) => indexmap! {
                "type".to_string() => "environment".to_string(),
                "key".to_string() => key.to_string(),
                "value".to_string() => value.to_string(),
            },
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

        let ts = ToolSource::LegacyVersionFile(PathBuf::from("/home/user/.node-version"));
        assert_str_eq!(ts.to_string(), "/home/user/.node-version");

        let ts = ToolSource::Argument;
        assert_str_eq!(ts.to_string(), "--runtime");

        let ts = ToolSource::Environment("RTX_NODEJS_VERSION".to_string(), "18".to_string());
        assert_str_eq!(ts.to_string(), "RTX_NODEJS_VERSION=18");
    }

    #[test]
    fn test_tool_source_as_json() {
        let ts = ToolSource::ToolVersions(PathBuf::from("/home/user/.test-tool-versions"));
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => ".tool-versions".to_string(),
                "path".to_string() => "/home/user/.test-tool-versions".to_string(),
            }
        );

        let ts = ToolSource::RtxToml(PathBuf::from("/home/user/.rtx.toml"));
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => ".rtx.toml".to_string(),
                "path".to_string() => "/home/user/.rtx.toml".to_string(),
            }
        );

        let ts = ToolSource::LegacyVersionFile(PathBuf::from("/home/user/.node-version"));
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => "legacy-version-file".to_string(),
                "path".to_string() => "/home/user/.node-version".to_string(),
            }
        );

        let ts = ToolSource::Argument;
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => "argument".to_string(),
            }
        );

        let ts = ToolSource::Environment("RTX_NODEJS_VERSION".to_string(), "18".to_string());
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => "environment".to_string(),
                "key".to_string() => "RTX_NODEJS_VERSION".to_string(),
                "value".to_string() => "18".to_string(),
            }
        );
    }
}
