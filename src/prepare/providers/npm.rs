use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for npm (package-lock.json)
#[derive(Debug)]
pub struct NpmPrepareProvider {
    base: ProviderBase,
}

impl NpmPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("npm", project_root, config),
        }
    }
}

impl PrepareProvider for NpmPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.base.project_root.join("package-lock.json"),
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
            program: "npm".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "npm install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.project_root.join("package-lock.json").exists()
    }
}
