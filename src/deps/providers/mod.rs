mod bun;
mod bundler;
mod composer;
mod custom;
mod git_submodule;
mod go;
mod npm;
mod pip;
mod pnpm;
mod poetry;
mod uv;
mod yarn;

pub use bun::BunDepsProvider;
pub use bundler::BundlerDepsProvider;
pub use composer::ComposerDepsProvider;
pub use custom::CustomDepsProvider;
pub use git_submodule::GitSubmoduleDepsProvider;
pub use go::GoDepsProvider;
pub use npm::NpmDepsProvider;
pub use pip::PipDepsProvider;
pub use pnpm::PnpmDepsProvider;
pub use poetry::PoetryDepsProvider;
pub use uv::UvDepsProvider;
pub use yarn::YarnDepsProvider;

use std::path::{Path, PathBuf};

use crate::deps::rule::DepsProviderConfig;

/// Shared base for all deps providers, holding the id, project root, and config.
/// Provides common implementations for `id` and `is_auto`.
#[derive(Debug)]
pub struct ProviderBase {
    pub(crate) id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) config: DepsProviderConfig,
}

impl ProviderBase {
    pub fn new(id: impl Into<String>, project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            id: id.into(),
            project_root: project_root.to_path_buf(),
            config,
        }
    }

    pub fn is_auto(&self) -> bool {
        self.config.auto
    }

    /// Returns the effective root directory for resolving sources/outputs.
    /// When `dir` is set in config, returns `project_root/dir`; otherwise `project_root`.
    pub fn config_root(&self) -> PathBuf {
        match &self.config.dir {
            Some(dir) => self.project_root.join(dir),
            None => self.project_root.clone(),
        }
    }
}
