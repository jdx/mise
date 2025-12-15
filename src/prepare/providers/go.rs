use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for Go (go.sum)
#[derive(Debug)]
pub struct GoPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl GoPrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.to_path_buf(),
            config,
        }
    }
}

impl PrepareProvider for GoPrepareProvider {
    fn id(&self) -> &str {
        "go"
    }

    fn sources(&self) -> Vec<PathBuf> {
        // go.mod defines dependencies - changes here trigger downloads
        vec![self.project_root.join("go.mod")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Go downloads modules to GOPATH/pkg/mod, but we can check vendor/ if used
        let vendor = self.project_root.join("vendor");
        if vendor.exists() {
            vec![vendor]
        } else {
            // go.sum gets updated after go mod download completes
            vec![self.project_root.join("go.sum")]
        }
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.config.run {
            return PrepareCommand::from_string(run, &self.project_root, &self.config);
        }

        Ok(PrepareCommand {
            program: "go".to_string(),
            args: vec!["mod".to_string(), "download".to_string()],
            env: self.config.env.clone(),
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "go mod download".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.project_root.join("go.sum").exists()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }
}
