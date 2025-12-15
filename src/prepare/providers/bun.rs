use std::collections::BTreeMap;
use std::path::PathBuf;

use eyre::Result;

use crate::prepare::rule::PrepareProviderConfig;
use crate::prepare::{PrepareCommand, PrepareProvider};

/// Prepare provider for bun
#[derive(Debug)]
pub struct BunPrepareProvider {
    project_root: PathBuf,
    config: PrepareProviderConfig,
}

impl BunPrepareProvider {
    pub fn new(project_root: &PathBuf, config: PrepareProviderConfig) -> Self {
        Self {
            project_root: project_root.clone(),
            config,
        }
    }

    fn lockfile_path(&self) -> Option<PathBuf> {
        // Bun supports both bun.lockb (binary) and bun.lock (text)
        let binary_lock = self.project_root.join("bun.lockb");
        if binary_lock.exists() {
            return Some(binary_lock);
        }

        let text_lock = self.project_root.join("bun.lock");
        if text_lock.exists() {
            return Some(text_lock);
        }

        None
    }
}

impl PrepareProvider for BunPrepareProvider {
    fn id(&self) -> &str {
        "bun"
    }

    fn sources(&self) -> Vec<PathBuf> {
        let mut sources = vec![];

        // Add lockfile as primary source
        if let Some(lockfile) = self.lockfile_path() {
            sources.push(lockfile);
        }

        // Add package.json as secondary source
        let package_json = self.project_root.join("package.json");
        if package_json.exists() {
            sources.push(package_json);
        }

        // Add extra sources from config
        for extra in &self.config.extra_sources {
            let path = self.project_root.join(extra);
            if path.exists() {
                sources.push(path);
            }
        }

        sources
    }

    fn outputs(&self) -> Vec<PathBuf> {
        let mut outputs = vec![self.project_root.join("node_modules")];

        // Add extra outputs from config
        for extra in &self.config.extra_outputs {
            outputs.push(self.project_root.join(extra));
        }

        outputs
    }

    fn prepare_command(&self) -> Result<PrepareCommand> {
        // Check for custom command override
        if let Some(custom_run) = &self.config.run {
            let parts: Vec<&str> = custom_run.split_whitespace().collect();
            let (program, args) = parts.split_first().unwrap_or((&"bun", &[]));

            let mut env = BTreeMap::new();
            for (k, v) in &self.config.env {
                env.insert(k.clone(), v.clone());
            }

            return Ok(PrepareCommand {
                program: program.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                env,
                cwd: self
                    .config
                    .dir
                    .as_ref()
                    .map(|d| self.project_root.join(d))
                    .or_else(|| Some(self.project_root.clone())),
                description: self
                    .config
                    .description
                    .clone()
                    .unwrap_or_else(|| "Installing bun dependencies".to_string()),
            });
        }

        let mut env = BTreeMap::new();
        for (k, v) in &self.config.env {
            env.insert(k.clone(), v.clone());
        }

        Ok(PrepareCommand {
            program: "bun".to_string(),
            args: vec!["install".to_string()],
            env,
            cwd: Some(self.project_root.clone()),
            description: self
                .config
                .description
                .clone()
                .unwrap_or_else(|| "Installing bun dependencies".to_string()),
        })
    }

    fn is_applicable(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Applicable if bun.lockb or bun.lock exists
        self.lockfile_path().is_some()
    }

    fn is_auto(&self) -> bool {
        self.config.auto
    }

    fn priority(&self) -> u32 {
        self.config.priority
    }
}
