//! Backend trait decomposition
//!
//! This module splits the monolithic Backend trait into focused sub-traits
//! for better separation of concerns and easier implementation.
//!
//! # Trait Hierarchy
//!
//! - `BackendIdentity` - Core identity (required by all)
//! - `VersionProvider` - Version discovery and listing
//! - `Installer` - Installation and uninstallation
//! - `LockfileSupport` - Lockfile and platform-specific metadata
//! - `DependencyManager` - Dependency resolution
//! - `BinPathProvider` - Binary paths and execution environment
//!
//! The main `Backend` trait combines all these traits with additional
//! convenience methods and caching.

mod bin_paths;
mod dependencies;
mod identity;
mod installer;
mod lockfile;
mod version_provider;

pub use bin_paths::BinPathProvider;
pub use dependencies::DependencyManager;
pub use identity::BackendIdentity;
pub use installer::Installer;
pub use lockfile::LockfileSupport;
pub use version_provider::VersionProvider;
