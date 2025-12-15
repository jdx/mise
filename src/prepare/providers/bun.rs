use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for bun (bun.lockb or bun.lock)
#[derive(Debug)]
pub struct BunPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl BunPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }

    fn lockfile_path(&self) -> Option<PathBuf> {
        // Bun supports both bun.lockb (binary) and bun.lock (text)
        let binary_lock = self.project_root.join("bun.lockb");
        if binary_lock.exists() {
            return Some(binary_lock);
        }
        let text_lock = self.project_root.join("bun.lock");
        if text_lock.exists() {
            return Some(text_lock);
        }
        None
    }
}

impl PrepareProvider for BunPrepareProvider {
    fn id(&self) -> &str {
        "bun"
    }

    fn sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![];
        if let Some(lockfile) = self.lockfile_path() {
            sources.push(lockfile);
        }
        sources.push(self.project_root.join("package.json"));
        sources
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
            program: "bun".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "bun install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.lockfile_path().is_some()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
