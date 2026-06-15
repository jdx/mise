//! Aqua Registry
//!
//! This crate provides functionality for working with Aqua package registry files.
//! It handles parsing registry YAML, looking up packages, and managing compiled
//! registry cache files. Fetching policy, remote fallback behavior, and baked-in
//! registry integration live in mise.

mod cache;
mod codec;
mod compiled;
mod file_ext;
mod template;
pub mod types;

// Re-export only what's needed by the main mise crate
pub use cache::RegistryCache;
pub use codec::{decode_package_rkyv, encode_package_rkyv};
pub use compiled::{CompiledRegistry, ParsedRegistry};
pub use types::{
    AquaChecksum, AquaChecksumType, AquaCosign, AquaFile, AquaGithubArtifactAttestations,
    AquaMinisign, AquaMinisignType, AquaPackage, AquaPackageType, AquaVar, RegistryYaml,
};

use thiserror::Error;

/// Errors that can occur when working with the Aqua registry
#[derive(Error, Debug)]
pub enum AquaRegistryError {
    #[error("package not found: {0}")]
    PackageNotFound(String),
    #[error("registry not available: {0}")]
    RegistryNotAvailable(String),
    #[error("template error: {0}")]
    TemplateError(#[from] eyre::Error),
    #[error("yaml parse error: {0}")]
    YamlError(#[from] serde_yaml::Error),
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("expression error: {0}")]
    ExpressionError(String),
}

pub type Result<T> = std::result::Result<T, AquaRegistryError>;
