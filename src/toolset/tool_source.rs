use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use indexmap::{indexmap, IndexMap};

use crate::file::display_path;

/// where a tool version came from (e.g.: .tool-versions)
#[derive(Debug, Default, Clone, PartialEq, Eq, Ord, PartialOrd, Hash, strum::EnumIs)]
pub enum ToolSource {
    ToolVersions(PathBuf),
    MiseToml(PathBuf),
    LegacyVersionFile(PathBuf),
    Argument,
    Environment(String, String),
    #[default]
    Unknown,
}

impl Display for ToolSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            ToolSource::ToolVersions(path) => write!(f, "{}", display_path(path)),
            ToolSource::MiseToml(path) => write!(f, "{}", display_path(path)),
            ToolSource::LegacyVersionFile(path) => write!(f, "{}", display_path(path)),
            ToolSource::Argument => write!(f, "--runtime"),
            ToolSource::Environment(k, v) => write!(f, "{k}={v}"),
            ToolSource::Unknown => write!(f, "unknown"),
        }
    }
}

impl ToolSource {
    pub fn path(&self) -> Option<&Path> {
        match self {
            ToolSource::ToolVersions(path) => Some(path),
            ToolSource::MiseToml(path) => Some(path),
            ToolSource::LegacyVersionFile(path) => Some(path),
            _ => None,
        }
    }

    pub fn as_json(&self) -> IndexMap<String, String> {
        match self {
            ToolSource::ToolVersions(path) => indexmap! {
                "type".to_string() => ".tool-versions".to_string(),
                "path".to_string() => path.to_string_lossy().to_string(),
            },
            ToolSource::MiseToml(path) => indexmap! {
                "type".to_string() => "mise.toml".to_string(),
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
            ToolSource::Unknown => indexmap! {
                "type".to_string() => "unknown".to_string(),
            },
        }
    }
}

impl Serialize for ToolSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = serializer.serialize_struct("ToolSource", 3)?;
        match self {
            ToolSource::ToolVersions(path) => {
                s.serialize_field("type", ".tool-versions")?;
                s.serialize_field("path", path)?;
            }
            ToolSource::MiseToml(path) => {
                s.serialize_field("type", "mise.toml")?;
                s.serialize_field("path", path)?;
            }
            ToolSource::LegacyVersionFile(path) => {
                s.serialize_field("type", "legacy-version-file")?;
                s.serialize_field("path", path)?;
            }
            ToolSource::Argument => {
                s.serialize_field("type", "argument")?;
            }
            ToolSource::Environment(key, value) => {
                s.serialize_field("type", "environment")?;
                s.serialize_field("key", key)?;
                s.serialize_field("value", value)?;
            }
            ToolSource::Unknown => {
                s.serialize_field("type", "unknown")?;
            }
        }

        s.end()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_str_eq};

    use super::*;

    #[test]
    fn test_tool_source_display() {
        let path = PathBuf::from("/home/user/.test-tool-versions");

        let ts = ToolSource::ToolVersions(path);
        assert_str_eq!(ts.to_string(), "/home/user/.test-tool-versions");

        let ts = ToolSource::MiseToml(PathBuf::from("/home/user/.mise.toml"));
        assert_str_eq!(ts.to_string(), "/home/user/.mise.toml");

        let ts = ToolSource::LegacyVersionFile(PathBuf::from("/home/user/.node-version"));
        assert_str_eq!(ts.to_string(), "/home/user/.node-version");

        let ts = ToolSource::Argument;
        assert_str_eq!(ts.to_string(), "--runtime");

        let ts = ToolSource::Environment("MISE_NODE_VERSION".to_string(), "18".to_string());
        assert_str_eq!(ts.to_string(), "MISE_NODE_VERSION=18");
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

        let ts = ToolSource::MiseToml(PathBuf::from("/home/user/.mise.toml"));
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => "mise.toml".to_string(),
                "path".to_string() => "/home/user/.mise.toml".to_string(),
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

        let ts = ToolSource::Environment("MISE_NODE_VERSION".to_string(), "18".to_string());
        assert_eq!(
            ts.as_json(),
            indexmap! {
                "type".to_string() => "environment".to_string(),
                "key".to_string() => "MISE_NODE_VERSION".to_string(),
                "value".to_string() => "18".to_string(),
            }
        );
    }
}
