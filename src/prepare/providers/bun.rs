use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for bun (bun.lockb or bun.lock)
#[derive(Debug)]
pub struct BunPrepareProvider {
    base: ProviderBase,
}

impl BunPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bun", project_root, config),
        }
    }

    fn lockfile_path(&self) -> Option<PathBuf> {
        // Bun supports both bun.lockb (binary) and bun.lock (text)
        let binary_lock = self.base.project_root.join("bun.lockb");
        if binary_lock.exists() {
            return Some(binary_lock);
        }
        let text_lock = self.base.project_root.join("bun.lock");
        if text_lock.exists() {
            return Some(text_lock);
        }
        None
    }
}

impl PrepareProvider for BunPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![];
        if let Some(lockfile) = self.lockfile_path() {
            sources.push(lockfile);
        }
        sources.push(self.base.project_root.join("package.json"));
        sources
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.project_root.join("node_modules")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
            program: "bun".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
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
}
