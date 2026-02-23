use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for pip (requirements.txt)
#[derive(Debug)]
pub struct PipPrepareProvider {
    base: ProviderBase,
}

impl PipPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("pip", project_root, config),
        }
    }
}

impl PrepareProvider for PipPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![self.base.project_root.join("requirements.txt")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Check for .venv directory as output indicator
        vec![self.base.project_root.join(".venv")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
            program: "pip".to_string(),
            args: vec![
                "install".to_string(),
                "-r".to_string(),
                "requirements.txt".to_string(),
            ],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "pip install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.project_root.join("requirements.txt").exists()
    }
}
