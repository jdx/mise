use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for npm (package-lock.json)
#[derive(Debug)]
pub(crate) struct NpmDepsProvider {
    base: ProviderBase,
}

impl NpmDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("npm", project_root, config),
        }
    }
}

impl DepsProvider for NpmDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base.sources(vec![
            root.join("package-lock.json"),
            root.join("package.json"),
        ])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base
            .outputs(vec![self.base.config_root().join("node_modules")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("npm", &["install"], "npm install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("package-lock.json"))
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("npm", "install", dev.then_some("--save-dev"), packages))
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("npm", "uninstall", None, packages))
    }
}
