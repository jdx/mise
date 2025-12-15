use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for PHP Composer (composer.lock)
#[derive(Debug)]
pub struct ComposerPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl ComposerPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }
}

impl PrepareProvider for ComposerPrepareProvider {
    fn id(&self) -> &str {
        "composer"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("composer.lock"),
            self.project_root.join("composer.json"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.project_root.join("vendor")]
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
            program: "composer".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "composer install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("composer.lock").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
