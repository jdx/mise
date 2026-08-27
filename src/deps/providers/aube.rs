use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for aube (aube-lock.yaml)
#[derive(Debug)]
pub(crate) struct AubeDepsProvider {
    base: ProviderBase,
}

impl AubeDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("aube", project_root, config),
        }
    }
}

impl DepsProvider for AubeDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base
            .sources(vec![root.join("aube-lock.yaml"), root.join("package.json")])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base
            .outputs(vec![self.base.config_root().join("node_modules")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("aube", &["install"], "aube install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("aube-lock.yaml"))
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("aube", "add", dev.then_some("-D"), packages))
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        Ok(self.base.package_command("aube", "remove", None, packages))
    }
}
