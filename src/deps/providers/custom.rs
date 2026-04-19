use std::path::{Path, PathBuf};

use eyre::Result;
use glob::glob;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for user-defined custom rules from mise.toml [deps.*]
#[derive(Debug)]
pub struct CustomDepsProvider {
    base: ProviderBase,
}

impl CustomDepsProvider {
    pub fn new(id: String, config: DepsProviderConfig, project_root: &Path) -> Self {
        Self {
            base: ProviderBase::new(id, project_root, config),
        }
    }

    /// Expand glob patterns in sources/outputs
    fn expand_globs(&self, patterns: &[String]) -> Vec<PathBuf> {
        let mut paths = vec![];

        for pattern in patterns {
            let base_dir = self.base.config_root();
            let full_pattern = if PathBuf::from(pattern).is_relative() {
                base_dir.join(pattern)
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

impl DepsProvider for CustomDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.base.config.sources)
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.expand_globs(&self.base.config.outputs)
    }

    fn install_command(&self) -> Result<DepsCommand> {
        let run = self
            .base
            .config
            .run
            .as_ref()
            .ok_or_else(|| eyre::eyre!("deps rule {} has no run command", self.base.id))?;

        DepsCommand::from_string(run, &self.base.project_root, &self.base.config)
    }

    fn is_applicable(&self) -> bool {
        // Custom providers require a run command to be applicable
        self.base.config.run.is_some()
    }
}
