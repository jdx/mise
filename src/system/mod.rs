//! The bootstrap, dotfiles, and history modules live in `mise-system`; this keeps the paths mise's
//! other modules use and adds the pieces that need mise's config, toolset, and plugins.

pub use mise_system::*;

pub mod bootstrap_host;
pub mod package_plugin;
pub mod packages_host;

// Deleted with the cleanup it guards, which lives in `mise_system::packages::apt_sources`.
crate::remove_by!("2027.10.0", "apt-legacy-keyring-cleanup");
