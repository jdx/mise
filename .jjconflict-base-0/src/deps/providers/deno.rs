use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for Deno (deno.lock)
#[derive(Debug)]
pub(crate) struct DenoDepsProvider {
    base: ProviderBase,
}

impl DenoDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("deno", project_root, config),
        }
    }
}

impl DepsProvider for DenoDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        self.base.sources(vec![
            root.join("deno.lock"),
            root.join("deno.json"),
            root.join("deno.jsonc"),
            root.join("package.json"),
        ])
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base.outputs(vec![])
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // https://docs.deno.com/runtime/fundamentals/node/#node_modules
        self.base
            .optional_outputs(vec![self.base.config_root().join("node_modules")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("deno", &["install"], "deno install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        DepsProviderApplicability::require_file(&self.base.config_root().join("deno.lock"))
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("deno", "add", dev.then_some("--dev"), packages))
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        Ok(self.base.package_command("deno", "remove", None, packages))
    }
}
