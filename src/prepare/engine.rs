use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use eyre::Result;

use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::miseprintln;
use crate::ui::multi_progress_report::MultiProgressReport;

use super::PrepareProvider;
use super::providers::{
    BunPrepareProvider, CustomPrepareProvider, NpmPrepareProvider, PnpmPrepareProvider,
    YarnPrepareProvider,
};
use super::rule::{BUILTIN_PROVIDERS, PrepareConfig};

/// Options for running prepare steps
#[derive(Debug, Default)]
pub struct PrepareOptions {
    /// Only check if prepare is needed, don't run commands
    pub dry_run: bool,
    /// Force run all prepare steps even if outputs are fresh
    pub force: bool,
    /// Run specific prepare rule(s) only
    pub only: Option<Vec<String>>,
    /// Skip specific prepare rule(s)
    pub skip: Vec<String>,
    /// Environment variables to pass to prepare commands (e.g., toolset PATH)
    pub env: BTreeMap<String, String>,
    /// If true, only run providers with auto=true
    pub auto_only: bool,
}

/// Result of a prepare step
#[derive(Debug)]
pub enum PrepareStepResult {
    /// Step ran successfully
    Ran(String),
    /// Step would have run (dry-run mode)
    WouldRun(String),
    /// Step was skipped because outputs are fresh
    Fresh(String),
    /// Step was skipped by user request
    Skipped(String),
}

/// Result of running all prepare steps
#[derive(Debug)]
pub struct PrepareResult {
    pub steps: Vec<PrepareStepResult>,
}

impl PrepareResult {
    /// Returns true if any steps ran or would have run
    pub fn had_work(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s,
                PrepareStepResult::Ran(_) | PrepareStepResult::WouldRun(_)
            )
        })
    }
}

/// Engine that discovers and runs prepare providers
pub struct PrepareEngine {
    config: Arc<Config>,
    providers: Vec<Box<dyn PrepareProvider>>,
}

impl PrepareEngine {
    /// Create a new PrepareEngine, discovering all applicable providers
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let providers = Self::discover_providers(&config)?;
        Ok(Self { config, providers })
    }

    /// Discover all applicable prepare providers for the current project
    fn discover_providers(config: &Config) -> Result<Vec<Box<dyn PrepareProvider>>> {
        let project_root = config
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let mut providers: Vec<Box<dyn PrepareProvider>> = vec![];

        // Load prepare config from mise.toml
        let prepare_config = config
            .config_files
            .values()
            .filter_map(|cf| cf.prepare_config())
            .fold(PrepareConfig::default(), |acc, pc| acc.merge(&pc));

        // Iterate over all configured providers
        for (id, provider_config) in &prepare_config.providers {
            let provider: Box<dyn PrepareProvider> = if BUILTIN_PROVIDERS.contains(&id.as_str()) {
                // Built-in provider with specialized implementation
                match id.as_str() {
                    "npm" => Box::new(NpmPrepareProvider::new(
                        &project_root,
                        provider_config.clone(),
                    )),
                    "yarn" => Box::new(YarnPrepareProvider::new(
                        &project_root,
                        provider_config.clone(),
                    )),
                    "pnpm" => Box::new(PnpmPrepareProvider::new(
                        &project_root,
                        provider_config.clone(),
                    )),
                    "bun" => Box::new(BunPrepareProvider::new(
                        &project_root,
                        provider_config.clone(),
                    )),
                    // Future: "cargo", "go", "python"
                    _ => continue, // Skip unimplemented built-ins
                }
            } else {
                // Custom provider
                Box::new(CustomPrepareProvider::new(
                    id.clone(),
                    provider_config.clone(),
                    project_root.clone(),
                ))
            };

            if provider.is_applicable() {
                providers.push(provider);
            }
        }

        // Filter disabled providers
        providers.retain(|p| !prepare_config.disable.contains(&p.id().to_string()));

        // Sort by priority (higher first)
        providers.sort_by(|a, b| b.priority().cmp(&a.priority()));

        Ok(providers)
    }

    /// List all discovered providers
    pub fn list_providers(&self) -> Vec<&dyn PrepareProvider> {
        self.providers.iter().map(|p| p.as_ref()).collect()
    }

    /// Check if any auto-enabled provider has stale outputs (without running)
    /// Returns the IDs of stale providers
    pub fn check_staleness(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|p| p.is_auto())
            .filter(|p| !self.check_freshness(p.as_ref()).unwrap_or(true))
            .map(|p| p.id())
            .collect()
    }

    /// Run all stale prepare steps
    pub async fn run(&self, opts: PrepareOptions) -> Result<PrepareResult> {
        let mut results = vec![];
        let mpr = MultiProgressReport::get();

        for provider in &self.providers {
            let id = provider.id().to_string();

            // Check auto_only filter
            if opts.auto_only && !provider.is_auto() {
                trace!("prepare step {} is not auto, skipping", id);
                results.push(PrepareStepResult::Skipped(id));
                continue;
            }

            // Check skip list
            if opts.skip.contains(&id) {
                results.push(PrepareStepResult::Skipped(id));
                continue;
            }

            // Check only list
            if let Some(ref only) = opts.only
                && !only.contains(&id)
            {
                results.push(PrepareStepResult::Skipped(id));
                continue;
            }

            let is_fresh = if opts.force {
                false
            } else {
                self.check_freshness(provider.as_ref())?
            };

            if !is_fresh {
                let cmd = provider.prepare_command()?;

                if opts.dry_run {
                    miseprintln!("[dry-run] would run: {} ({})", cmd.description, id);
                    results.push(PrepareStepResult::WouldRun(id));
                } else {
                    let pr = mpr.add(&cmd.description);
                    match self
                        .execute_prepare(provider.as_ref(), &cmd, &opts.env)
                        .await
                    {
                        Ok(()) => {
                            pr.finish_with_message(format!("{} done", cmd.description));
                            results.push(PrepareStepResult::Ran(id));
                        }
                        Err(e) => {
                            pr.finish_with_message(format!("{} failed: {}", cmd.description, e));
                            return Err(e);
                        }
                    }
                }
            } else {
                trace!("prepare step {} is fresh, skipping", id);
                results.push(PrepareStepResult::Fresh(id));
            }
        }

        Ok(PrepareResult { steps: results })
    }

    /// Check if outputs are newer than sources (stateless mtime comparison)
    fn check_freshness(&self, provider: &dyn PrepareProvider) -> Result<bool> {
        let sources = provider.sources();
        let outputs = provider.outputs();

        if sources.is_empty() || outputs.is_empty() {
            return Ok(false); // If no sources or outputs defined, always run
        }

        let sources_mtime = Self::last_modified(&sources)?;
        let outputs_mtime = Self::last_modified(&outputs)?;

        match (sources_mtime, outputs_mtime) {
            (Some(src), Some(out)) => Ok(src < out), // Fresh if outputs newer than sources
            (None, _) => Ok(true),                   // No sources exist, consider fresh
            (_, None) => Ok(false),                  // No outputs exist, not fresh
        }
    }

    /// Get the most recent modification time from a list of paths
    fn last_modified(paths: &[PathBuf]) -> Result<Option<SystemTime>> {
        let mtimes: Vec<SystemTime> = paths
            .iter()
            .filter(|p| p.exists())
            .filter_map(|p| p.metadata().ok())
            .filter_map(|m| m.modified().ok())
            .collect();

        Ok(mtimes.into_iter().max())
    }

    /// Execute a prepare command
    async fn execute_prepare(
        &self,
        _provider: &dyn PrepareProvider,
        cmd: &super::PrepareCommand,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<()> {
        let cwd = cmd
            .cwd
            .clone()
            .or_else(|| self.config.project_root.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let mut runner = CmdLineRunner::new(&cmd.program)
            .args(&cmd.args)
            .current_dir(cwd);

        // Apply toolset environment (includes PATH with installed tools)
        for (k, v) in toolset_env {
            runner = runner.env(k, v);
        }

        // Apply command-specific environment (can override toolset env)
        for (k, v) in &cmd.env {
            runner = runner.env(k, v);
        }

        // Use raw output for better UX during dependency installation
        if Settings::get().raw {
            runner = runner.raw(true);
        }

        runner.execute()?;
        Ok(())
    }
}
