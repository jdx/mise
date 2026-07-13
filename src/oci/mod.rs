//! OCI image building from a mise.toml.
//!
//! The core invariant: each installed tool version becomes its own OCI layer.
//! Because mise installs tools to isolated, non-overlapping directories, layer
//! *ordering* is semantically irrelevant — swapping a tool version swaps
//! exactly one content-addressable blob. See `builder.rs` for how this is
//! orchestrated.

pub mod builder;
pub mod layer;
pub mod layout;
pub mod manifest;
pub mod packages;
pub mod registry;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

pub use builder::{BuildOptions, BuildOutput, Builder};
pub use layer::LayerOwner;

/// A host path copied into an OCI image as an independent layer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OciCopy {
    pub host: PathBuf,
    pub image: String,
}

impl OciCopy {
    pub fn validate(&self) -> Result<(), String> {
        if self.host.as_os_str().is_empty() {
            return Err("copy host path must not be empty".to_string());
        }
        validate_image_path(&self.image)
    }
}

impl FromStr for OciCopy {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // Split from the right so Windows drive letters in HOST remain intact.
        let (host, image) = value
            .rsplit_once(':')
            .ok_or_else(|| "copy must be HOST_PATH:IMAGE_PATH".to_string())?;
        if host.is_empty() {
            return Err("copy host path must not be empty".to_string());
        }
        let copy = Self {
            host: PathBuf::from(host),
            image: image.to_string(),
        };
        copy.validate()?;
        Ok(copy)
    }
}

fn validate_image_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err(format!("copy image path must be absolute (got {path:?})"));
    }
    if path.trim_matches('/').is_empty() {
        return Err(format!(
            "copy image path must not be the root `/` (got {path:?})"
        ));
    }
    if path.split('/').any(|part| part == "." || part == "..") {
        return Err(format!(
            "copy image path must not contain `.` or `..` components (got {path:?})"
        ));
    }
    Ok(())
}

/// Normalize a Rust-style arch name (`x86_64`, `aarch64`) to the OCI-spec
/// value (`amd64`, `arm64`).
pub fn normalize_arch(a: &str) -> &str {
    match a {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// Normalize a host OS name to the OCI-spec value. OCI images are
/// linux-targeted in v1, so any non-linux host (macos, windows) maps to
/// `linux` — otherwise the platform lookup in a multi-arch index
/// (e.g. `debian:bookworm-slim`) would fail with "no matching platform",
/// and a scratch build would label its `ImageConfig.os` as a value that
/// makes the image unrunnable as a Linux container.
pub fn normalize_os(o: &str) -> &str {
    match o {
        "macos" | "windows" => "linux",
        other => other,
    }
}

/// The `[oci]` section of a `mise.toml`. All fields optional.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OciConfig {
    /// Base image reference (overrides `oci.default_from` setting).
    #[serde(default)]
    pub from: Option<String>,
    /// Default tag applied to the built image.
    #[serde(default)]
    pub tag: Option<String>,
    /// Working directory baked into the image config.
    #[serde(default)]
    pub workdir: Option<String>,
    /// Entrypoint baked into the image config.
    #[serde(default)]
    pub entrypoint: Option<Vec<String>>,
    /// Cmd baked into the image config.
    #[serde(default)]
    pub cmd: Option<Vec<String>>,
    /// User baked into the image config.
    #[serde(default)]
    pub user: Option<String>,
    /// Numeric UID assigned to tar layer entries.
    #[serde(default)]
    pub user_id: Option<u32>,
    /// Numeric GID assigned to tar layer entries. Defaults to `user_id` when unset.
    #[serde(default)]
    pub group_id: Option<u32>,
    /// Override where mise installs go in the image. Defaults to the value of
    /// the `oci.default_mount_point` setting (`/mise`).
    #[serde(default)]
    pub mount_point: Option<String>,
    /// Host files or directories copied into the image as independent layers.
    #[serde(default)]
    pub copy: Vec<OciCopy>,
    /// Extra env vars baked into the image config in addition to those derived
    /// from the mise.toml `[env]` section and per-tool `exec_env()`.
    #[serde(default)]
    pub env: IndexMap<String, String>,
    /// Labels baked into the image config.
    #[serde(default)]
    pub labels: IndexMap<String, String>,
}

impl OciConfig {
    /// Fill any field on `self` that is `None` / empty from `other`, leaving
    /// existing values on `self` untouched. Call this while iterating
    /// config files from **most specific to least specific** — the first
    /// value encountered wins, independent of the map's iteration order.
    ///
    /// For map fields (env, labels), keys already present on `self` win;
    /// new keys from `other` are added. Copy entries accumulate with less
    /// specific configs first, so a more specific copy targeting the same
    /// image path is emitted later and takes precedence.
    pub fn fill_defaults_from(&mut self, other: Self) {
        if self.from.is_none() {
            self.from = other.from;
        }
        if self.tag.is_none() {
            self.tag = other.tag;
        }
        if self.workdir.is_none() {
            self.workdir = other.workdir;
        }
        if self.entrypoint.is_none() {
            self.entrypoint = other.entrypoint;
        }
        if self.cmd.is_none() {
            self.cmd = other.cmd;
        }
        if self.user.is_none() {
            self.user = other.user;
        }
        let had_user_id = self.user_id.is_some();
        if self.user_id.is_none() {
            self.user_id = other.user_id;
        }
        // If a more-specific layer selected a UID but omitted GID, leave GID
        // unset so owner resolution can apply the documented gid = uid fallback.
        if self.group_id.is_none() && !had_user_id {
            self.group_id = other.group_id;
        }
        if self.mount_point.is_none() {
            self.mount_point = other.mount_point;
        }
        let mut copy = other.copy;
        copy.append(&mut self.copy);
        self.copy = copy;
        for (k, v) in other.env {
            self.env.entry(k).or_insert(v);
        }
        for (k, v) in other.labels {
            self.labels.entry(k).or_insert(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(host: &str, image: &str) -> OciCopy {
        OciCopy {
            host: PathBuf::from(host),
            image: image.to_string(),
        }
    }

    #[test]
    fn copy_rejects_root_and_relative_image_paths() {
        assert!("file:/".parse::<OciCopy>().is_err());
        assert!("file:relative".parse::<OciCopy>().is_err());
    }

    #[test]
    fn layered_copies_put_more_specific_entries_last() {
        let mut merged = OciConfig {
            copy: vec![copy("project", "/same")],
            ..Default::default()
        };
        merged.fill_defaults_from(OciConfig {
            copy: vec![copy("parent", "/same")],
            ..Default::default()
        });

        assert_eq!(
            merged.copy,
            vec![copy("parent", "/same"), copy("project", "/same")]
        );
    }
}
