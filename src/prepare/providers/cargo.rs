use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for Rust/Cargo (Cargo.lock)
#[derive(Debug)]
pub struct CargoPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl CargoPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }
}

impl PrepareProvider for CargoPrepareProvider {
    fn id(&self) -> &str {
        "cargo"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("Cargo.lock"),
            self.project_root.join("Cargo.toml"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.project_root.join("target")]
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
            program: "cargo".to_string(),
            args: vec!["fetch".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "cargo fetch".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("Cargo.lock").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
