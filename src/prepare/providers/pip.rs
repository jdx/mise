use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for pip (requirements.txt)
#[derive(Debug)]
pub struct PipPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl PipPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }
}

impl PrepareProvider for PipPrepareProvider {
    fn id(&self) -> &str {
        "pip"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![self.project_root.join("requirements.txt")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Check for .venv directory as output indicator
        vec![self.project_root.join(".venv")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return PrepareCommand::from_string(run, &self.project_root, &self.config);
        }

        Ok(PrepareCommand {
            program: "pip".to_string(),
            args: vec![
                "install".to_string(),
                "-r".to_string(),
                "requirements.txt".to_string(),
            ],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "pip install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("requirements.txt").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
