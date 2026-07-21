//! OCI image manifest and config JSON structures.
//!
//! Hand-rolled serde structs matching the OCI image-spec v1.0.2. Kept minimal
//! — only the fields we read or write. See:
//! <https://github.com/opencontainers/image-spec/blob/main/image-layout.md>

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub const OCI_LAYOUT_VERSION: &str = "1.0.0";

pub const MEDIA_TYPE_OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const MEDIA_TYPE_OCI_CONFIG: &str = "application/vnd.oci.image.config.v1+json";
pub const MEDIA_TYPE_OCI_INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const MEDIA_TYPE_OCI_LAYER_GZIP: &str = "application/vnd.oci.image.layer.v1.tar+gzip";

pub const MEDIA_TYPE_DOCKER_MANIFEST: &str = "application/vnd.docker.distribution.manifest.v2+json";
pub const MEDIA_TYPE_DOCKER_CONFIG: &str = "application/vnd.docker.container.image.v1+json";
pub const MEDIA_TYPE_DOCKER_LAYER_GZIP: &str = "application/vnd.docker.image.rootfs.diff.tar.gzip";
pub const MEDIA_TYPE_DOCKER_MANIFEST_LIST: &str =
    "application/vnd.docker.distribution.manifest.list.v2+json";

/// `oci-layout` marker file (written at the root of an image layout directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OciLayout {
    #[serde(rename = "imageLayoutVersion")]
    pub image_layout_version: String,
}

impl Default for OciLayout {
    fn default() -> Self {
        Self {
            image_layout_version: OCI_LAYOUT_VERSION.to_string(),
        }
    }
}

/// `index.json` at the root of an image layout — lists the manifest(s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageIndex {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub manifests: Vec<Descriptor>,
}

/// Descriptor referencing a blob by digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub size: u64,
    pub digest: String,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub annotations: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<Platform>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Platform {
    pub architecture: String,
    pub os: String,
    #[serde(
        rename = "os.version",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub os_version: Option<String>,
    #[serde(rename = "os.features", default, skip_serializing_if = "Vec::is_empty")]
    pub os_features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
}

/// Image manifest JSON blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub config: Descriptor,
    pub layers: Vec<Descriptor>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub annotations: IndexMap<String, String>,
}

/// Image config JSON blob (what `docker inspect` surfaces as `.Config`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub architecture: String,
    pub os: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(rename = "config", default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Config>,
    pub rootfs: RootFs,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<History>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, rename = "Env", skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[serde(default, rename = "Cmd", skip_serializing_if = "Option::is_none")]
    pub cmd: Option<Vec<String>>,
    #[serde(
        default,
        rename = "Entrypoint",
        skip_serializing_if = "Option::is_none"
    )]
    pub entrypoint: Option<Vec<String>>,
    #[serde(
        default,
        rename = "WorkingDir",
        skip_serializing_if = "Option::is_none"
    )]
    pub working_dir: Option<String>,
    #[serde(default, rename = "User", skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, rename = "Labels", skip_serializing_if = "IndexMap::is_empty")]
    pub labels: IndexMap<String, String>,
    #[serde(
        default,
        rename = "ExposedPorts",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub exposed_ports: IndexMap<String, serde_json::Value>,
    #[serde(
        default,
        rename = "Volumes",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub volumes: IndexMap<String, serde_json::Value>,
    #[serde(
        default,
        rename = "StopSignal",
        skip_serializing_if = "Option::is_none"
    )]
    pub stop_signal: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootFs {
    #[serde(rename = "type")]
    pub type_: String,
    pub diff_ids: Vec<String>,
}

impl Default for RootFs {
    fn default() -> Self {
        Self {
            type_: "layers".to_string(),
            diff_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_layer: Option<bool>,
}

/// Normalize a Docker media type to the OCI equivalent. We pass base-image
/// layers through byte-for-byte, so we keep whatever media type they came with
/// — but when building our own manifest we always emit OCI types.
pub fn media_type_to_oci(mt: &str) -> &str {
    match mt {
        MEDIA_TYPE_DOCKER_MANIFEST => MEDIA_TYPE_OCI_MANIFEST,
        MEDIA_TYPE_DOCKER_CONFIG => MEDIA_TYPE_OCI_CONFIG,
        MEDIA_TYPE_DOCKER_LAYER_GZIP => MEDIA_TYPE_OCI_LAYER_GZIP,
        _ => mt,
    }
}
