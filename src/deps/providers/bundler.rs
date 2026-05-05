use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for Ruby Bundler (Gemfile.lock)
#[derive(Debug)]
pub struct BundlerDepsProvider {
    base: ProviderBase,
}

impl BundlerDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bundler", project_root, config),
        }
    }
}

impl DepsProvider for BundlerDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("Gemfile.lock"), root.join("Gemfile")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // `bundle install` writes to the system/user gem path by default and
        // only populates `vendor/bundle` when `--path vendor/bundle` is used.
        // Track it as optional so vendored projects detect deletion of
        // `vendor/bundle`, while non-vendored projects rely on source hashes.
        vec![self.base.config_root().join("vendor/bundle")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "bundle".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "bundle install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("Gemfile.lock").exists()
    }
}
