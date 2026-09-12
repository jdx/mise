use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for pnpm (pnpm-lock.yaml)
#[derive(Debug)]
pub(crate) struct PnpmDepsProvider {
    base: ProviderBase,
}

impl PnpmDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("pnpm", project_root, config),
        }
    }
}

impl DepsProvider for PnpmDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base
            .sources(vec![root.join("pnpm-lock.yaml"), root.join("package.json")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base
            .outputs(vec![self.base.config_root().join("node_modules")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("pnpm", &["install"], "pnpm install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("pnpm-lock.yaml"))
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("pnpm", "add", dev.then_some("--save-dev"), packages))
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        Ok(self.base.package_command("pnpm", "remove", None, packages))
    }
}
