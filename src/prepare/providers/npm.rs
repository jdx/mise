use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for npm (package-lock.json)
#[derive(Debug)]
pub struct NpmPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl NpmPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }
}

impl PrepareProvider for NpmPrepareProvider {
    fn id(&self) -> &str {
        "npm"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("package-lock.json"),
            self.project_root.join("package.json"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.project_root.join("node_modules")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return Ok(PrepareCommand::from_string(
                run,
                &self.project_root,
                &self.config,
            ));
        }

        Ok(PrepareCommand {
            program: "npm".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "npm install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("package-lock.json").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
