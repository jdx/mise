use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider, DepsProviderApplicability};

use super::ProviderBase;

/// Deps provider for bun (bun.lockb or bun.lock)
#[derive(Debug)]
pub(crate) struct BunDepsProvider {
    base: ProviderBase,
}

impl BunDepsProvider {
    pub(crate) fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bun", project_root, config),
        }
    }

    fn lockfile_path(&self) -> Option<PathBuf> {
        let root = self.base.config_root();
        // Bun supports both bun.lockb (binary) and bun.lock (text)
        let binary_lock = root.join("bun.lockb");
        if binary_lock.is_file() {
            return Some(binary_lock);
        }
        let text_lock = root.join("bun.lock");
        if text_lock.is_file() {
            return Some(text_lock);
        }
        None
    }
}

impl DepsProvider for BunDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![];
        if let Some(lockfile) = self.lockfile_path() {
            sources.push(lockfile);
        }
        sources.push(self.base.config_root().join("package.json"));
        self.base.sources(sources)
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.base
            .outputs(vec![self.base.config_root().join("node_modules")])
    }

    fn install_command(&self) -> Result<DepsCommand> {
        self.base
            .install_command("bun", &["install"], "bun install")
    }

    fn applicability(&self) -> DepsProviderApplicability {
        let root = self.base.config_root();
        let binary_lock = root.join("bun.lockb");
        let text_lock = root.join("bun.lock");
        DepsProviderApplicability::require_any_file(&[&binary_lock, &text_lock])
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        Ok(self
            .base
            .package_command("bun", "add", dev.then_some("--dev"), packages))
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        Ok(self.base.package_command("bun", "remove", None, packages))
    }
}
