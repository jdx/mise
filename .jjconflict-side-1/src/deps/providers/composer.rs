use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for PHP Composer (composer.lock)
#[derive(Debug)]
pub(crate) struct ComposerDepsProvider {
    base: ProviderBase,
}

impl ComposerDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("composer", project_root, config),
        }
    }
}

impl DepsProvider for ComposerDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base
            .sources(vec![root.join("composer.lock"), root.join("composer.json")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base
            .outputs(vec![self.base.config_root().join("vendor")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("composer", &["install"], "composer install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("composer.lock"))
    }
}
