use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for yarn (yarn.lock)
#[derive(Debug)]
pub struct YarnDepsProvider {
    base: ProviderBase,
}

impl YarnDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("yarn", project_root, config),
        }
    }
}

impl DepsProvider for YarnDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![root.join("yarn.lock"), root.join("package.json")]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![self.base.config_root().join("node_modules")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "yarn".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "yarn install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("yarn.lock").exists()
    }

    fn add_command(&self, package: &str, dev: bool) -> Result<DepsCommand> {
        let mut args = vec!["add".to_string()];
        if dev {
            args.push("--dev".to_string());
        }
        args.push(package.to_string());

        Ok(DepsCommand {
            program: "yarn".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("yarn add {package}"),
        })
    }

    fn remove_command(&self, package: &str) -> Result<DepsCommand> {
        Ok(DepsCommand {
            program: "yarn".to_string(),
            args: vec!["remove".to_string(), package.to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("yarn remove {package}"),
        })
    }
}
