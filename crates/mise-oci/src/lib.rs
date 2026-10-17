//! OCI image building from a mise.toml.
//!
//! The core invariant: each installed tool version becomes its own OCI layer.
//! Because mise installs tools to isolated, non-overlapping directories, layer
//! *ordering* is semantically irrelevant — swapping a tool version swaps
//! exactly one content-addressable blob. See the builder in mise for how this
//! is orchestrated.

#[macro_use]
mod macros;
#[cfg(test)]
mod test_host;

pub mod auth;
pub mod docker_archive;
pub mod layer;
pub mod layout;
pub mod manifest;
pub mod packages;
pub mod registry;

pub use layer::LayerOwner;
pub use mise_config::oci::{OciConfig, OciCopy};

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
