use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for Go (go.sum)
#[derive(Debug)]
pub struct GoPrepareProvider {
    base: ProviderBase,
}

impl GoPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("go", project_root, config),
        }
    }
}

impl PrepareProvider for GoPrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        // go.mod defines dependencies - changes here trigger downloads
        vec![self.base.project_root.join("go.mod")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Go downloads modules to GOPATH/pkg/mod, but we can check vendor/ if used
        let vendor = self.base.project_root.join("vendor");
        if vendor.exists() {
            vec![vendor]
        } else {
            // go.sum gets updated after go mod download completes
            vec![self.base.project_root.join("go.sum")]
        }
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        // Use `go mod vendor` if vendor/ exists, otherwise `go mod download`
        let vendor = self.base.project_root.join("vendor");
        let (args, desc) = if vendor.exists() {
            (
                vec!["mod".to_string(), "vendor".to_string()],
                "go mod vendor",
            )
        } else {
            (
                vec!["mod".to_string(), "download".to_string()],
                "go mod download",
            )
        };

        Ok(PrepareCommand {
            program: "go".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| desc.to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        // Check for go.mod (the source/lockfile), not go.sum (which may be an output)
        self.base.project_root.join("go.mod").exists()
    }
}
