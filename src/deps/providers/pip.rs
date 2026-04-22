use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for pip (requirements.txt)
#[derive(Debug)]
pub struct PipDepsProvider {
    base: ProviderBase,
}

impl PipDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("pip", project_root, config),
        }
    }
}

impl DepsProvider for PipDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join("requirements.txt")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Check for .venv directory as output indicator
        vec![self.base.config_root().join(".venv")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "pip".to_string(),
            args: vec![
                "install".to_string(),
                "-r".to_string(),
                "requirements.txt".to_string(),
            ],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "pip install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("requirements.txt").exists()
    }
}
