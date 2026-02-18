use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for yarn (yarn.lock)
#[derive(Debug)]
pub struct YarnPrepareProvider {
    base: ProviderBase,
}

impl YarnPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("yarn", project_root, config),
        }
    }
}

impl PrepareProvider for YarnPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.base.project_root.join("yarn.lock"),
            self.base.project_root.join("package.json"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.project_root.join("node_modules")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
            program: "yarn".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "yarn install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.project_root.join("yarn.lock").exists()
    }
}
