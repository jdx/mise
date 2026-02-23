mod bun;
mod bundler;
mod composer;
mod custom;
mod go;
mod npm;
mod pip;
mod pnpm;
mod poetry;
mod uv;
mod yarn;

pub use bun::BunPrepareProvider;
pub use bundler::BundlerPrepareProvider;
pub use composer::ComposerPrepareProvider;
pub use custom::CustomPrepareProvider;
pub use go::GoPrepareProvider;
pub use npm::NpmPrepareProvider;
pub use pip::PipPrepareProvider;
pub use pnpm::PnpmPrepareProvider;
pub use poetry::PoetryPrepareProvider;
pub use uv::UvPrepareProvider;
pub use yarn::YarnPrepareProvider;

use std::path::{Path, PathBuf};

use crate::prepare::rule::PrepareProviderConfig;

/// Shared base for all prepare providers, holding the id, project root, and config.
/// Provides common implementations for `id`, `is_auto`, and `touch_outputs`.
#[derive(Debug)]
pub struct ProviderBase {
    pub(crate) id: String,
    pub(crate) project_root: PathBuf,
    pub(crate) config: PrepareProviderConfig,
}

impl ProviderBase {
    pub fn new(id: impl Into<String>, project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            id: id.into(),
            project_root: project_root.to_path_buf(),
            config,
        }
    }

    pub fn is_auto(&self) -> bool {
        self.config.auto
    }

    pub fn touch_outputs(&self) -> bool {
        self.config.touch_outputs.unwrap_or(true)
    }
}
