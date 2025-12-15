use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for Ruby Bundler (Gemfile.lock)
#[derive(Debug)]
pub struct BundlerPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl BundlerPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            config,
        }
    }
}

impl PrepareProvider for BundlerPrepareProvider {
    fn id(&self) -> &str {
        "bundler"
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![
            self.project_root.join("Gemfile.lock"),
            self.project_root.join("Gemfile"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Check for vendor/bundle if using --path vendor/bundle
        let vendor = self.project_root.join("vendor/bundle");
        if vendor.exists() {
            vec![vendor]
        } else {
            // Use .bundle directory as fallback indicator
            vec![self.project_root.join(".bundle")]
        }
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return PrepareCommand::from_string(run, &self.project_root, &self.config);
        }

        Ok(PrepareCommand {
            program: "bundle".to_string(),
            args: vec!["install".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "bundle install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("Gemfile.lock").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
