use std::path::PathBuf;

use eyre::Result;
use glob::glob;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for user-defined custom rules from mise.toml [prepare.*]
#[derive(Debug)]
pub struct CustomPrepareProvider {
    id: String,
    config: PrepareProviderConfig,
    project_root: PathBuf,
}

impl CustomPrepareProvider {
    pub fn new(id: String, config: PrepareProviderConfig, project_root: PathBuf) -> Self {
        Self {
            id,
            config,
            project_root,
        }
    }

    /// Expand glob patterns in sources/outputs
    fn expand_globs(&self, patterns: &[String]) -> Vec<PathBuf> {
        let mut paths = vec![];

        for pattern in patterns {
            let full_pattern = if PathBuf::from(pattern).is_relative() {
                self.project_root.join(pattern)
            } else {
                PathBuf::from(pattern)
            };

            // Check if it's a glob pattern
            if pattern.contains('*') || pattern.contains('{') || pattern.contains('?') {
                if let Ok(entries) = glob(full_pattern.to_string_lossy().as_ref()) {
                    for entry in entries.flatten() {
                        paths.push(entry);
                    }
                }
            } else if full_pattern.exists() {
                paths.push(full_pattern);
            } else {
                // Include even if doesn't exist (for outputs that may not exist yet)
                paths.push(full_pattern);
            }
        }

        paths
    }
}

impl PrepareProvider for CustomPrepareProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.config.sources)
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.config.outputs)
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        let run = self
            .config
            .run
            .as_ref()
            .ok_or_else(|| eyre::eyre!("prepare rule {} has no run command", self.id))?;

        PrepareCommand::from_string(run, &self.project_root, &self.config)
    }

    fn is_applicable(&self) -> bool {
        // Custom providers require a run command to be applicable
        self.config.run.is_some()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
