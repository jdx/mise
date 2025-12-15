use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for yarn (yarn.lock)
#[derive(Debug)]
pub struct YarnPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl YarnPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            config,
        }
    }
}

impl PrepareProvider for YarnPrepareProvider {
    fn id(&self) -> &str {
        "yarn"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("yarn.lock"),
            self.project_root.join("package.json"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.project_root.join("node_modules")]
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return PrepareCommand::from_string(run, &self.project_root, &self.config);
        }

        Ok(PrepareCommand {
            program: "yarn".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "yarn install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("yarn.lock").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
