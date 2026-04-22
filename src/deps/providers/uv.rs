use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for uv (uv.lock)
#[derive(Debug)]
pub struct UvDepsProvider {
    base: ProviderBase,
}

impl UvDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("uv", project_root, config),
        }
    }
}

impl DepsProvider for UvDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("uv.lock"), root.join("pyproject.toml")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join(".venv")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "uv".to_string(),
            args: vec!["sync".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "uv sync".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("uv.lock").exists()
    }
}
