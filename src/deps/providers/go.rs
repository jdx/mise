use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for Go (go.sum)
#[derive(Debug)]
pub struct GoDepsProvider {
    base: ProviderBase,
}

impl GoDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("go", project_root, config),
        }
    }
}

impl DepsProvider for GoDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        // go.mod defines dependencies - changes here trigger downloads
        vec![self.base.config_root().join("go.mod")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        // Go downloads modules to GOPATH/pkg/mod by default, leaving nothing in
        // the project tree to check. Only treat `vendor/` as an output when the
        // project is vendored; otherwise fall back to source-hash freshness.
        let vendor = self.base.config_root().join("vendor");
        if vendor.exists() {
            vec![vendor]
        } else {
            vec![]
        }
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        // Use `go mod vendor` if vendor/ exists, otherwise `go mod download`
        let vendor = self.base.config_root().join("vendor");
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

        Ok(DepsCommand {
            program: "go".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
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
        self.base.config_root().join("go.mod").exists()
    }
}
