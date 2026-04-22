use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for bun (bun.lockb or bun.lock)
#[derive(Debug)]
pub struct BunDepsProvider {
    base: ProviderBase,
}

impl BunDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bun", project_root, config),
        }
    }

    fn lockfile_path(&self) -> Option<PathBuf> {
        let root = self.base.config_root();
        // Bun supports both bun.lockb (binary) and bun.lock (text)
        let binary_lock = root.join("bun.lockb");
        if binary_lock.exists() {
            return Some(binary_lock);
        }
        let text_lock = root.join("bun.lock");
        if text_lock.exists() {
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
        sources
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join("node_modules")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "bun".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "bun install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.lockfile_path().is_some()
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        let mut args = vec!["add".to_string()];
        if dev {
            args.push("--dev".to_string());
        }
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "bun".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("bun add {}", packages.join(" ")),
        })
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        let mut args = vec!["remove".to_string()];
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "bun".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("bun remove {}", packages.join(" ")),
        })
    }
}
