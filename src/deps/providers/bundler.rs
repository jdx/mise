use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for Ruby Bundler (Gemfile.lock)
#[derive(Debug)]
pub(crate) struct BundlerDepsProvider {
    base: ProviderBase,
}

impl BundlerDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bundler", project_root, config),
        }
    }
}

impl DepsProvider for BundlerDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base
            .sources(vec![root.join("Gemfile.lock"), root.join("Gemfile")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // `bundle install` writes to the system/user gem path by default and
        // only populates `vendor/bundle` when `--path vendor/bundle` is used.
        // Track it as optional so vendored projects detect deletion of
        // `vendor/bundle`, while non-vendored projects rely on source hashes.
        self.base
            .optional_outputs(vec![self.base.config_root().join("vendor/bundle")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("bundle", &["install"], "bundle install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("Gemfile.lock"))
    }
}
