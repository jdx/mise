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
pub mod registry;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub use builder::{BuildOptions, BuildOutput, Builder};

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
    /// Override where mise installs go in the image. Defaults to the value of
    /// the `oci.default_mount_point` setting (`/mise`).
    #[serde(default)]
    pub mount_point: Option<String>,
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
    /// new keys from `other` are added.
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
        if self.mount_point.is_none() {
            self.mount_point = other.mount_point;
        }
        for (k, v) in other.env {
            self.env.entry(k).or_insert(v);
        }
        for (k, v) in other.labels {
            self.labels.entry(k).or_insert(v);
        }
    }
}
