//! Resource identity, plan output data, and path serialization.

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use eyre::Result;
use serde::{Serialize, Serializer};

use crate::state::ManagedFilePhase;

/// Stable identity for one declarative bootstrap resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ResourceId {
    pub kind: String,
    pub name: String,
}

/// Where a declarative resource came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResourceOrigin {
    #[serde(serialize_with = "serialize_path")]
    pub config: PathBuf,
    #[serde(serialize_with = "serialize_path")]
    pub config_root: PathBuf,
    pub environment: Vec<String>,
    #[serde(serialize_with = "serialize_optional_path")]
    pub source: Option<PathBuf>,
}

impl ResourceOrigin {
    /// Formats the declaration and source paths for a sibling-resource conflict.
    pub fn conflict_description(&self) -> String {
        let mut description = format!("config: {}", self.config.display());
        if let Some(source) = &self.source {
            description.push_str(&format!("\n    source: {}", source.display()));
        }
        description
    }
}

const ENCODED_PATH_PREFIX: &str = "mise:path-";

pub fn serialize_path<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path_json_string(path))
}

fn serialize_optional_path<S>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    path.as_deref().map(path_json_string).serialize(serializer)
}

fn path_json_string(path: &Path) -> Cow<'_, str> {
    if let Some(path) = path.to_str()
        && !path.starts_with(ENCODED_PATH_PREFIX)
    {
        return Cow::Borrowed(path);
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        let encoded = BASE64_URL_SAFE_NO_PAD.encode(path.as_os_str().as_bytes());
        Cow::Owned(format!("{ENCODED_PATH_PREFIX}bytes:{encoded}"))
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let bytes = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let encoded = BASE64_URL_SAFE_NO_PAD.encode(bytes);
        Cow::Owned(format!("{ENCODED_PATH_PREFIX}utf16:{encoded}"))
    }
}

impl ResourceId {
    pub fn new(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.name)
    }
}

/// The operation needed to converge a resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAction {
    Create,
    Update,
    Remove,
    Noop,
    Unknown,
}

impl fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Remove => "remove",
            Self::Noop => "unchanged",
            Self::Unknown => "unknown",
        })
    }
}

/// A secret-safe description of one resource's current and desired state.
#[derive(Clone, Debug, Serialize)]
pub struct ResourcePlan {
    pub id: ResourceId,
    pub current: String,
    pub desired: String,
    pub action: ResourceAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ResourceOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ResourceId>,
    /// Execution order only; these resources do not affect change prediction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_after: Vec<ResourceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ManagedFilePhase>,
}

impl ResourcePlan {
    pub fn new(
        id: ResourceId,
        current: impl Into<String>,
        desired: impl Into<String>,
        action: ResourceAction,
    ) -> Self {
        Self {
            id,
            current: current.into(),
            desired: desired.into(),
            action,
            origin: None,
            depends_on: vec![],
            order_after: vec![],
            phase: None,
        }
    }

    pub fn with_origin(mut self, origin: ResourceOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    pub fn with_file_phase(mut self, phase: ManagedFilePhase) -> Self {
        self.phase = Some(phase);
        self
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn resource_origin_serializes_non_utf8_paths_losslessly() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let invalid_path = PathBuf::from(OsString::from_vec(b"/tmp/invalid-\xff".to_vec()));
        let other_invalid_path = PathBuf::from(OsString::from_vec(b"/tmp/invalid-\xfe".to_vec()));
        let origin = ResourceOrigin {
            config: invalid_path.clone(),
            config_root: invalid_path.clone(),
            environment: vec![],
            source: Some(invalid_path.clone()),
        };

        let value = serde_json::to_value(origin).unwrap();
        let encoded = value["config"].as_str().unwrap();
        assert!(encoded.starts_with("mise:path-bytes:"));
        assert_eq!(value["config_root"], encoded);
        assert_eq!(value["source"], encoded);
        assert_ne!(encoded, path_json_string(&other_invalid_path));
    }
}
