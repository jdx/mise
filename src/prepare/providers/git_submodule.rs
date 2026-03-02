use std::path::{Path, PathBuf};

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

use super::ProviderBase;

/// Prepare provider for git submodules (.gitmodules)
#[derive(Debug)]
pub struct GitSubmodulePrepareProvider {
    base: ProviderBase,
}

impl GitSubmodulePrepareProvider {
    pub fn new(project_root: &Path, config: PrepareProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("git-submodule", project_root, config),
        }
    }

    /// Parse submodule paths from .gitmodules file
    fn submodule_paths(&self) -> Vec<PathBuf> {
        let gitmodules = self.base.project_root.join(".gitmodules");
        let Ok(content) = std::fs::read_to_string(&gitmodules) else {
            return vec![];
        };

        content
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if let Some(value) = line.strip_prefix("path") {
                    let value = value.trim_start();
                    value
                        .strip_prefix('=')
                        .map(|value| self.base.project_root.join(value.trim()))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl PrepareProvider for GitSubmodulePrepareProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        vec![self.base.project_root.join(".gitmodules")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.submodule_paths()
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        if let Some(run) = &self.base.config.run {
            return PrepareCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(PrepareCommand {
            program: "git".to_string(),
            args: vec![
                "submodule".to_string(),
                "update".to_string(),
                "--init".to_string(),
                "--recursive".to_string(),
            ],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.project_root.clone()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "git submodule update --init --recursive".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        let gitmodules = self.base.project_root.join(".gitmodules");
        gitmodules.exists() && gitmodules.metadata().map(|m| m.len() > 0).unwrap_or(false)
    }
}
