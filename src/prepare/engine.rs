use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use eyre::Result;
use filetime::FileTime;

use crate::cmd::CmdLineRunner;
use crate::config::config_file::ConfigFile;
use crate::config::{Config, Settings};
use crate::parallel;
use crate::ui::multi_progress_report::MultiProgressReport;

use super::PrepareProvider;
use super::providers::{
    BunPrepareProvider, BundlerPrepareProvider, ComposerPrepareProvider, CustomPrepareProvider,
    GoPrepareProvider, NpmPrepareProvider, PipPrepareProvider, PnpmPrepareProvider,
    PoetryPrepareProvider, UvPrepareProvider, YarnPrepareProvider,
};
use super::rule::BUILTIN_PROVIDERS;

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

/// A prepare job ready to be executed
struct PrepareJob {
    id: String,
    cmd: super::PrepareCommand,
    outputs: Vec<PathBuf>,
    touch: bool,
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
    providers: Vec<Box<dyn PrepareProvider>>,
}

impl PrepareEngine {
    /// Create a new PrepareEngine, discovering all applicable providers
    pub fn new(config: &Config) -> Result<Self> {
        let providers = Self::discover_providers(config)?;
        // Only require experimental when prepare is actually configured
        if !providers.is_empty() {
            Settings::get().ensure_experimental("prepare")?;
        }
        Ok(Self { providers })
    }

    /// Discover all applicable prepare providers for the current project
    ///
    /// Each config file's prepare providers are scoped to that config file's directory.
    /// For example, a `[prepare.pnpm]` defined in the root `mise.toml` only applies when
    /// running from the root directory, not from subdirectories.
    fn discover_providers(config: &Config) -> Result<Vec<Box<dyn PrepareProvider>>> {
        let project_root = config
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let mut providers: Vec<Box<dyn PrepareProvider>> = vec![];
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut disabled: Vec<String> = vec![];

        // Process each config file's prepare config independently, using that
        // config file's directory as the project root for its providers.
        // Only include config files that belong to the current project root
        // (skip config files outside the current project root, e.g. from parent directories).
        for cf in config.config_files.values() {
            let Some(prepare_config) = cf.prepare_config() else {
                continue;
            };

            // Skip config files from parent directories - prepare providers
            // should only run from the directory where they are defined.
            // Global/system configs (project_root() == None) are always included.
            if let Some(cf_project_root) = cf.project_root()
                && cf_project_root != project_root
            {
                continue;
            }

            // Collect disable list scoped to this project root
            disabled.extend(prepare_config.disable.iter().cloned());

            let config_root = cf.config_root();

            for (id, provider_config) in &prepare_config.providers {
                // Skip duplicate provider IDs (first config file wins)
                if !seen_ids.insert(id.clone()) {
                    continue;
                }

                if let Some(provider) =
                    Self::build_provider(id, &config_root, provider_config.clone())
                    && provider.is_applicable()
                {
                    providers.push(provider);
                }
            }
        }

        // Filter disabled providers
        providers.retain(|p| !disabled.contains(&p.id().to_string()));

        Ok(providers)
    }

    /// Build a provider from its ID, config root, and configuration
    fn build_provider(
        id: &str,
        config_root: &Path,
        provider_config: super::rule::PrepareProviderConfig,
    ) -> Option<Box<dyn PrepareProvider>> {
        if BUILTIN_PROVIDERS.contains(&id) {
            match id {
                "npm" => Some(Box::new(NpmPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "yarn" => Some(Box::new(YarnPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "pnpm" => Some(Box::new(PnpmPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "bun" => Some(Box::new(BunPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "go" => Some(Box::new(GoPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "pip" => Some(Box::new(PipPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "poetry" => Some(Box::new(PoetryPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "uv" => Some(Box::new(UvPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "bundler" => Some(Box::new(BundlerPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                "composer" => Some(Box::new(ComposerPrepareProvider::new(
                    config_root,
                    provider_config,
                ))),
                _ => None,
            }
        } else {
            Some(Box::new(CustomPrepareProvider::new(
                id.to_string(),
                provider_config,
                config_root,
            )))
        }
    }

    /// Add providers from additional config files (e.g., monorepo subdirectory configs).
    ///
    /// Unlike `discover_providers`, this does NOT filter by project root, since these
    /// configs are intentionally from different directories (monorepo subdirectories).
    pub fn add_config_files(
        &mut self,
        config_files: impl IntoIterator<Item = Arc<dyn ConfigFile>>,
    ) {
        let mut seen_ids: HashSet<String> =
            self.providers.iter().map(|p| p.id().to_string()).collect();
        let mut disabled: Vec<String> = vec![];

        for cf in config_files {
            let Some(prepare_config) = cf.prepare_config() else {
                continue;
            };

            disabled.extend(prepare_config.disable.iter().cloned());
            let config_root = cf.config_root();

            for (id, provider_config) in &prepare_config.providers {
                if !seen_ids.insert(id.clone()) {
                    continue;
                }

                if let Some(provider) =
                    Self::build_provider(id, &config_root, provider_config.clone())
                    && provider.is_applicable()
                {
                    self.providers.push(provider);
                }
            }
        }

        if !disabled.is_empty() {
            self.providers
                .retain(|p| !disabled.contains(&p.id().to_string()));
        }
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

    /// Run all stale prepare steps in parallel
    pub async fn run(&self, opts: PrepareOptions) -> Result<PrepareResult> {
        let mut results = vec![];

        // Collect providers that need to run
        let mut to_run: Vec<PrepareJob> = vec![];

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
                let outputs = provider.outputs();
                let touch = provider.touch_outputs();

                if opts.dry_run {
                    // Just record that it would run, let CLI handle output
                    results.push(PrepareStepResult::WouldRun(id));
                } else {
                    to_run.push(PrepareJob {
                        id,
                        cmd,
                        outputs,
                        touch,
                    });
                }
            } else {
                trace!("prepare step {} is fresh, skipping", id);
                results.push(PrepareStepResult::Fresh(id));
            }
        }

        // Run stale providers in parallel
        if !to_run.is_empty() {
            let mpr = MultiProgressReport::get();
            let toolset_env = opts.env.clone();

            // Include mpr/env in the tuple so closure doesn't capture anything
            let to_run_with_context: Vec<_> = to_run
                .into_iter()
                .map(|job| (job, mpr.clone(), toolset_env.clone()))
                .collect();

            let run_results =
                parallel::parallel(to_run_with_context, |(job, mpr, toolset_env)| async move {
                    let pr = mpr.add(&job.cmd.description);
                    match Self::execute_prepare_static(&job.cmd, &toolset_env) {
                        Ok(()) => {
                            if job.touch {
                                Self::touch_outputs(&job.outputs);
                            }
                            pr.finish_with_message(format!("{} done", job.cmd.description));
                            // Return outputs along with result so we can clear stale status
                            // after ALL providers complete successfully
                            Ok((PrepareStepResult::Ran(job.id), job.outputs))
                        }
                        Err(e) => {
                            pr.finish_with_message(format!(
                                "{} failed: {}",
                                job.cmd.description, e
                            ));
                            Err(e)
                        }
                    }
                })
                .await?;

            // All providers completed successfully - now clear stale status for all outputs
            for (step_result, outputs) in run_results {
                for output in &outputs {
                    super::clear_output_stale(output);
                }
                results.push(step_result);
            }
        }

        Ok(PrepareResult { steps: results })
    }

    /// Check if outputs are newer than sources (stateless mtime comparison)
    fn check_freshness(&self, provider: &dyn PrepareProvider) -> Result<bool> {
        let sources = provider.sources();
        let outputs = provider.outputs();

        if outputs.is_empty() {
            return Ok(false); // No outputs defined, always run to be safe
        }

        // Check if any output was created this session (before prepare ran)
        // This handles the case where venv is auto-created but packages aren't installed yet
        for output in &outputs {
            if super::is_output_stale(output) {
                return Ok(false); // Created this session, needs prepare
            }
        }

        // Note: empty sources is handled below - last_modified([]) returns None,
        // and if outputs don't exist either, (_, None) takes precedence → stale

        let sources_mtime = Self::last_modified(&sources)?;
        let outputs_mtime = Self::last_modified(&outputs)?;

        match (sources_mtime, outputs_mtime) {
            (Some(src), Some(out)) => Ok(src <= out), // Fresh if outputs newer or equal to sources
            (_, None) => Ok(false), // No outputs exist, not fresh (takes precedence)
            (None, _) => Ok(true),  // No sources exist, consider fresh
        }
    }

    /// Get the most recent modification time from a list of paths
    /// For directories, recursively finds the newest file within (up to 3 levels deep)
    fn last_modified(paths: &[PathBuf]) -> Result<Option<SystemTime>> {
        let mut mtimes: Vec<SystemTime> = vec![];

        for path in paths.iter().filter(|p| p.exists()) {
            if path.is_dir() {
                // For directories, find the newest file within (limited depth for performance)
                if let Some(mtime) = Self::newest_file_in_dir(path, 3) {
                    mtimes.push(mtime);
                }
            } else if let Some(mtime) = path.metadata().ok().and_then(|m| m.modified().ok()) {
                mtimes.push(mtime);
            }
        }

        Ok(mtimes.into_iter().max())
    }

    /// Recursively find the newest file modification time in a directory.
    /// The directory's own mtime is always included so that touching the directory
    /// itself (e.g. via `touch_outputs`) is reflected in freshness checks.
    fn newest_file_in_dir(dir: &Path, max_depth: usize) -> Option<SystemTime> {
        // Always seed with the directory's own mtime so that touching the dir
        // (without modifying its contents) is visible to freshness checks.
        let mut newest = dir.metadata().ok().and_then(|m| m.modified().ok());

        if max_depth == 0 {
            return newest;
        }

        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let mtime = if path.is_dir() {
                    Self::newest_file_in_dir(&path, max_depth - 1)
                } else {
                    path.metadata().ok().and_then(|m| m.modified().ok())
                };

                if let Some(t) = mtime {
                    newest = Some(newest.map_or(t, |n| n.max(t)));
                }
            }
        }

        newest
    }

    /// Execute a prepare command (static version for parallel execution)
    fn execute_prepare_static(
        cmd: &super::PrepareCommand,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<()> {
        let cwd = match cmd.cwd.clone() {
            Some(dir) => dir,
            None => std::env::current_dir()?,
        };

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

    /// Update the mtime of output files/directories to now
    fn touch_outputs(outputs: &[PathBuf]) {
        let now = FileTime::now();
        for path in outputs {
            if path.exists()
                && let Err(e) = filetime::set_file_mtime(path, now)
            {
                warn!("failed to touch {}: {e}", path.display());
            }
        }
    }
}
