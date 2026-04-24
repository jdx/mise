use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for Poetry (poetry.lock)
#[derive(Debug)]
pub struct PoetryDepsProvider {
    base: ProviderBase,
}

impl PoetryDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("poetry", project_root, config),
        }
    }
}

impl DepsProvider for PoetryDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("poetry.lock"), root.join("pyproject.toml")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join(".venv")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "poetry".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "poetry install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("poetry.lock").exists()
    }
}
