use std::path::{Path, PathBuf};

use eyre::Result;

use crate::deps::rule::DepsProviderConfig;
use crate::deps::{DepsCommand, DepsProvider};

use super::ProviderBase;

/// Deps provider for Deno (deno.lock)
#[derive(Debug)]
pub struct DenoDepsProvider {
    base: ProviderBase,
}

impl DenoDepsProvider {
    pub fn new(project_root: &Path, config: DepsProviderConfig) -> Self {
        Self {
            base: ProviderBase::new("deno", project_root, config),
        }
    }
}

impl DepsProvider for DenoDepsProvider {
    fn base(&self) -> &ProviderBase {
        &self.base
    }

    fn sources(&self) -> Vec<PathBuf> {
        let root = self.base.config_root();
        vec![
            root.join("deno.lock"),
            root.join("deno.json"),
            root.join("deno.jsonc"),
            root.join("package.json"),
        ]
    }

    fn outputs(&self) -> Vec<PathBuf> {
        vec![]
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        // https://docs.deno.com/runtime/fundamentals/node/#node_modules
        vec![self.base.config_root().join("node_modules")]
    }

    fn install_command(&self) -> Result<DepsCommand> {
        if let Some(run) = &self.base.config.run {
            return DepsCommand::from_string(run, &self.base.project_root, &self.base.config);
        }

        Ok(DepsCommand {
            program: "deno".to_string(),
            args: vec!["install".to_string()],
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: self
                .base
                .config
                .description
                .clone()
                .unwrap_or_else(|| "deno install".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        self.base.config_root().join("deno.lock").exists()
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<DepsCommand> {
        let mut args = vec!["add".to_string()];
        if dev {
            args.push("--dev".to_string());
        }
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "deno".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("deno add {}", packages.join(" ")),
        })
    }

    fn remove_command(&self, packages: &[&str]) -> Result<DepsCommand> {
        let mut args = vec!["remove".to_string()];
        args.extend(packages.iter().map(|p| p.to_string()));

        Ok(DepsCommand {
            program: "deno".to_string(),
            args,
            env: self.base.config.env.clone(),
            cwd: Some(self.base.config_root()),
            description: format!("deno remove {}", packages.join(" ")),
        })
    }
}
