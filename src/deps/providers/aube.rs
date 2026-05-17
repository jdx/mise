use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for aube (aube-lock.yaml)
#[derive(Debug)]
pub struct AubeDepsProvider {
    base: ProviderBase,
}

impl AubeDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("aube", project_root, config),
        }
    }
}

impl DepsProvider for AubeDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("aube-lock.yaml"), root.join("package.json")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join("node_modules")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "aube".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "aube install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("aube-lock.yaml").exists()
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        let mut args = vec!["add".to_string()];
        if dev {
            args.push("-D".to_string());
        }
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "aube".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("aube add {}", packages.join(" ")),
        })
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        let mut args = vec!["remove".to_string()];
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "aube".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("aube remove {}", packages.join(" ")),
        })
    }
}
