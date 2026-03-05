use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for Ruby Bundler (Gemfile.lock)
#[derive(Debug)]
pub struct BundlerPrepareProvider {
    base: ProviderBase,
}

impl BundlerPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("bundler", project_root, config),
        }
    }
}

impl PrepareProvider for BundlerPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("Gemfile.lock"), root.join("Gemfile")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        // Check for vendor/bundle if using --path vendor/bundle
        let vendor = root.join("vendor/bundle");
        if vendor.exists() {
            vec![vendor]
        } else {
            // Use .bundle directory as fallback indicator
            vec![root.join(".bundle")]
        }
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
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
