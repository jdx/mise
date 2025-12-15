use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for Poetry (poetry.lock)
#[derive(Debug)]
pub struct PoetryPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl PoetryPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            config,
        }
    }
}

impl PrepareProvider for PoetryPrepareProvider {
    fn id(&self) -> &str {
        "poetry"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("poetry.lock"),
            self.project_root.join("pyproject.toml"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.project_root.join(".venv")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return PrepareCommand::from_string(run, &self.project_root, &self.config);
        }

        Ok(PrepareCommand {
            program: "poetry".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "poetry install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("poetry.lock").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
