use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for uv (uv.lock)
#[derive(Debug)]
pub struct UvPrepareProvider {
    base: ProviderBase,
}

impl UvPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("uv", project_root, config),
        }
    }
}

impl PrepareProvider for UvPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.base.project_root.join("uv.lock"),
            self.base.project_root.join("pyproject.toml"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.project_root.join(".venv")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
            program: "uv".to_string(),
            args: vec!["sync".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "uv sync".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.project_root.join("uv.lock").exists()
    }
}
