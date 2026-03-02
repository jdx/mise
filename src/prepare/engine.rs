use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use eyre::Result;
use filetime::FileTime;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::cmd::CmdLineRunner;
use crate::config::config_file::ConfigFile;
use crate::config::{Config, Settings};
use crate::ui::multi_progress_report::MultiProgressReport;

type StepOutput = (PrepareStepResult, Vec<PathBuf>);
type JobOutput = Result<(String, PrepareStepResult, Vec<PathBuf>), (String, eyre::Report)>;

use super::PrepareProvider;
use super::prepare_deps::PrepareDeps;
use super::providers::{
    BunPrepareProvider, BundlerPrepareProvider, ComposerPrepareProvider, CustomPrepareProvider,
    GitSubmodulePrepareProvider, GoPrepareProvider, NpmPrepareProvider, PipPrepareProvider,
    PnpmPrepareProvider, PoetryPrepareProvider, UvPrepareProvider, YarnPrepareProvider,
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

/// Result of a freshness check with human-readable reason
#[derive(Debug, Clone)]
pub enum FreshnessResult {
    /// Outputs are up to date
    Fresh,
    /// No outputs defined — always run
    NoOutputs,
    /// Output was created this session (e.g., auto-created venv)
    SessionStale(String),
    /// Some output files/dirs don't exist yet
    OutputsMissing(String),
    /// Sources are newer than outputs
    Stale(String),
    /// No sources exist — consider fresh
    NoSources,
    /// Forced by user request
    Forced,
}

impl FreshnessResult {
    pub fn is_fresh(&self) -> bool {
        matches!(self, FreshnessResult::Fresh | FreshnessResult::NoSources)
    }

    pub fn reason(&self) -> &str {
        match self {
            FreshnessResult::Fresh => "up to date",
            FreshnessResult::NoOutputs => "no outputs defined",
            FreshnessResult::SessionStale(r) => r,
            FreshnessResult::OutputsMissing(r) => r,
            FreshnessResult::Stale(r) => r,
            FreshnessResult::NoSources => "no sources",
            FreshnessResult::Forced => "forced",
        }
    }
}

/// Result of a prepare step
#[derive(Debug)]
pub enum PrepareStepResult {
    /// Step ran successfully
    Ran(String),
    /// Step would have run (dry-run mode) — (id, reason)
    WouldRun(String, String),
    /// Step was skipped because outputs are fresh
    Fresh(String),
    /// Step was skipped by user request
    Skipped(String),
    /// Step failed
    Failed(String),
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
    depends: Vec<String>,
    timeout: Option<std::time::Duration>,
}

impl PrepareResult {
    /// Returns true if any steps ran or would have run
    pub fn had_work(&self) -> bool {
        self.steps.iter().any(|s| {
            matches!(
                s,
                PrepareStepResult::Ran(_) | PrepareStepResult::WouldRun(_, _)
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
                "git-submodule" => Some(Box::new(GitSubmodulePrepareProvider::new(
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

    /// Find a specific provider by ID
    pub fn find_provider(&self, id: &str) -> Option<&dyn PrepareProvider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    /// Check freshness for a specific provider (public API for --explain)
    pub fn check_provider_freshness(
        &self,
        provider: &dyn PrepareProvider,
    ) -> Result<FreshnessResult> {
        self.check_freshness(provider)
    }

    /// Check if any auto-enabled provider has stale outputs (without running)
    /// Returns the IDs and reasons of stale providers
    pub fn check_staleness(&self) -> Vec<(&str, String)> {
        self.providers
            .iter()
            .filter(|p| p.is_auto())
            .filter_map(|p| {
                let result = self.check_freshness(p.as_ref());
                match result {
                    Ok(r) if !r.is_fresh() => Some((p.id(), r.reason().to_string())),
                    _ => None,
                }
            })
            .collect()
    }

    /// Run all stale prepare steps, respecting dependency ordering
    pub async fn run(&self, opts: PrepareOptions) -> Result<PrepareResult> {
        let mut results = vec![];

        // Collect providers that need to run
        let mut to_run: Vec<PrepareJob> = vec![];
        // Track IDs of providers that are fresh/skipped (treated as already satisfied for deps)
        let mut satisfied_ids: HashSet<String> = HashSet::new();

        for provider in &self.providers {
            let id = provider.id().to_string();

            // Check auto_only filter
            if opts.auto_only && !provider.is_auto() {
                trace!("prepare step {} is not auto, skipping", id);
                results.push(PrepareStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            // Check skip list
            if opts.skip.contains(&id) {
                results.push(PrepareStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            // Check only list
            if let Some(ref only) = opts.only
                && !only.contains(&id)
            {
                results.push(PrepareStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            let freshness = if opts.force {
                FreshnessResult::Forced
            } else {
                self.check_freshness(provider.as_ref())?
            };

            if !freshness.is_fresh() {
                let reason = freshness.reason().to_string();
                let cmd = provider.prepare_command()?;
                let outputs = provider.outputs();
                let touch = provider.touch_outputs();
                let depends = provider.depends();
                let timeout = provider.timeout();

                if opts.dry_run {
                    // Just record that it would run, let CLI handle output
                    results.push(PrepareStepResult::WouldRun(id, reason));
                } else {
                    to_run.push(PrepareJob {
                        id,
                        cmd,
                        outputs,
                        touch,
                        depends,
                        timeout,
                    });
                }
            } else {
                trace!("prepare step {} is fresh, skipping", id);
                results.push(PrepareStepResult::Fresh(id.clone()));
                satisfied_ids.insert(id);
            }
        }

        // Run stale providers with dependency ordering
        if !to_run.is_empty() {
            let has_deps = to_run.iter().any(|j| !j.depends.is_empty());

            if has_deps {
                let run_results = self
                    .run_with_deps(to_run, &satisfied_ids, &opts.env)
                    .await?;
                for (step_result, outputs) in run_results {
                    for output in &outputs {
                        super::clear_output_stale(output);
                    }
                    results.push(step_result);
                }
            } else {
                // No dependencies — use simple parallel execution
                let run_results = self.run_parallel(to_run, &opts.env).await?;
                for (step_result, outputs) in run_results {
                    for output in &outputs {
                        super::clear_output_stale(output);
                    }
                    results.push(step_result);
                }
            }
        }

        Ok(PrepareResult { steps: results })
    }

    /// Simple parallel execution (no dependency ordering)
    async fn run_parallel(
        &self,
        to_run: Vec<PrepareJob>,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<Vec<(PrepareStepResult, Vec<PathBuf>)>> {
        let mpr = MultiProgressReport::get();

        let to_run_with_context: Vec<_> = to_run
            .into_iter()
            .map(|job| (job, mpr.clone(), toolset_env.clone()))
            .collect();

        crate::parallel::parallel(to_run_with_context, |(job, mpr, toolset_env)| async move {
            let pr = mpr.add(&job.cmd.description);
            match Self::execute_prepare_static(&job.cmd, &toolset_env, job.timeout) {
                Ok(()) => {
                    if job.touch {
                        Self::touch_outputs(&job.outputs);
                    }
                    pr.finish_with_message(format!("{} done", job.cmd.description));
                    Ok((PrepareStepResult::Ran(job.id), job.outputs))
                }
                Err(e) => {
                    pr.finish_with_message(format!("{} failed: {}", job.cmd.description, e));
                    Err(e)
                }
            }
        })
        .await
    }

    /// Dependency-aware execution using Kahn's algorithm
    async fn run_with_deps(
        &self,
        to_run: Vec<PrepareJob>,
        satisfied_ids: &HashSet<String>,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<Vec<StepOutput>> {
        let mpr = MultiProgressReport::get();
        let mut results: Vec<StepOutput> = vec![];
        let mut errors: Vec<(String, String)> = vec![];

        // Build jobs map for lookup
        let running_ids: HashSet<String> = to_run.iter().map(|j| j.id.clone()).collect();
        let mut jobs: HashMap<String, PrepareJob> = HashMap::new();
        let mut dep_specs: Vec<(String, Vec<String>)> = vec![];

        for job in to_run {
            // Filter depends to only those that are actually running (not fresh/skipped)
            let filtered_deps: Vec<String> = job
                .depends
                .iter()
                .filter(|dep| {
                    if satisfied_ids.contains(*dep) {
                        // Dependency is already satisfied (fresh/skipped)
                        false
                    } else if running_ids.contains(*dep) {
                        // Dependency is in the run set — need to wait
                        true
                    } else {
                        // Unknown dep — warn but don't block
                        warn!(
                            "prepare provider '{}' depends on '{}' which is not configured",
                            job.id, dep
                        );
                        false
                    }
                })
                .cloned()
                .collect();

            dep_specs.push((job.id.clone(), filtered_deps));
            jobs.insert(job.id.clone(), job);
        }

        let mut deps = PrepareDeps::new(&dep_specs)?;

        // Report blocked providers (cycles)
        for blocked_id in deps.blocked_providers() {
            warn!(
                "prepare provider '{}' is blocked due to dependency cycle",
                blocked_id
            );
            if let Some(job) = jobs.remove(&blocked_id) {
                results.push((PrepareStepResult::Skipped(job.id), vec![]));
            }
        }

        let mut rx = deps.subscribe();
        let semaphore = Arc::new(Semaphore::new(Settings::get().jobs));
        let mut join_set: JoinSet<JobOutput> = JoinSet::new();
        // Track which tokio task ID maps to which provider ID for JoinError recovery
        let mut inflight: HashMap<tokio::task::Id, String> = HashMap::new();

        loop {
            tokio::select! {
                biased;

                // Prioritize handling completed tasks
                Some(join_result) = join_set.join_next() => {
                    match join_result {
                        Ok(Ok((id, step_result, outputs))) => {
                            inflight.retain(|_, v| v != &id);
                            results.push((step_result, outputs));
                            deps.complete_success(&id);
                        }
                        Ok(Err((id, e))) => {
                            inflight.retain(|_, v| v != &id);
                            warn!("prepare provider '{}' failed: {}", id, e);
                            errors.push((id.clone(), e.to_string()));
                            results.push((PrepareStepResult::Failed(id.clone()), vec![]));
                            deps.complete_failure(&id);
                            for blocked_id in deps.blocked_providers() {
                                if let Some(job) = jobs.remove(&blocked_id) {
                                    warn!(
                                        "prepare provider '{}' skipped due to failed dependency",
                                        job.id
                                    );
                                    results.push((PrepareStepResult::Skipped(job.id), vec![]));
                                }
                            }
                        }
                        Err(e) => {
                            // JoinError — task panicked or was cancelled
                            if let Some(id) = inflight.remove(&e.id()) {
                                warn!("prepare provider '{}' panicked: {}", id, e);
                                errors.push((id.clone(), e.to_string()));
                                results.push((PrepareStepResult::Failed(id.clone()), vec![]));
                                deps.complete_failure(&id);
                                for blocked_id in deps.blocked_providers() {
                                    if let Some(job) = jobs.remove(&blocked_id) {
                                        warn!(
                                            "prepare provider '{}' skipped due to failed dependency",
                                            job.id
                                        );
                                        results.push((PrepareStepResult::Skipped(job.id), vec![]));
                                    }
                                }
                            } else {
                                warn!("prepare task join error (unknown task): {e}");
                            }
                        }
                    }
                }

                // Receive next ready provider
                Some(maybe_id) = rx.recv() => {
                    let Some(id) = maybe_id else {
                        // None = all done
                        break;
                    };

                    let Some(job) = jobs.remove(&id) else {
                        continue;
                    };

                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let mpr = mpr.clone();
                    let toolset_env = toolset_env.clone();

                    let handle = join_set.spawn(async move {
                        let pr = mpr.add(&job.cmd.description);
                        let id = job.id;
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            Self::execute_prepare_static(&job.cmd, &toolset_env, job.timeout)
                        }));
                        drop(permit);

                        match result {
                            Ok(Ok(())) => {
                                if job.touch {
                                    Self::touch_outputs(&job.outputs);
                                }
                                pr.finish_with_message(format!("{} done", job.cmd.description));
                                let step = PrepareStepResult::Ran(id.clone());
                                Ok((id, step, job.outputs))
                            }
                            Ok(Err(e)) => {
                                pr.finish_with_message(format!(
                                    "{} failed: {}",
                                    job.cmd.description, e
                                ));
                                Err((id, e))
                            }
                            Err(_) => {
                                pr.finish_with_message(format!(
                                    "{} panicked",
                                    job.cmd.description
                                ));
                                Err((id, eyre::eyre!("task panicked")))
                            }
                        }
                    });
                    inflight.insert(handle.id(), id);
                }

                else => break,
            }
        }

        // Wait for remaining in-flight tasks
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok(Ok((id, step_result, outputs))) => {
                    inflight.retain(|_, v| v != &id);
                    results.push((step_result, outputs));
                }
                Ok(Err((id, e))) => {
                    inflight.retain(|_, v| v != &id);
                    warn!("prepare provider '{}' failed: {}", id, e);
                    errors.push((id.clone(), e.to_string()));
                    results.push((PrepareStepResult::Failed(id), vec![]));
                }
                Err(e) => {
                    if let Some(id) = inflight.remove(&e.id()) {
                        warn!("prepare provider '{}' panicked: {}", id, e);
                        errors.push((id.clone(), e.to_string()));
                        results.push((PrepareStepResult::Failed(id), vec![]));
                    } else {
                        warn!("prepare task join error (unknown task): {e}");
                    }
                }
            }
        }

        if !errors.is_empty() {
            let details = errors
                .iter()
                .map(|(id, msg)| format!("  {id}: {msg}"))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(eyre::eyre!("prepare providers failed:\n{details}"));
        }
        Ok(results)
    }

    /// Check if outputs are newer than sources (stateless mtime comparison)
    /// Returns a FreshnessResult with a human-readable reason
    fn check_freshness(&self, provider: &dyn PrepareProvider) -> Result<FreshnessResult> {
        let sources = provider.sources();
        let outputs = provider.outputs();

        if outputs.is_empty() {
            return Ok(FreshnessResult::NoOutputs);
        }

        // Check if any output was created this session (before prepare ran)
        // This handles the case where venv is auto-created but packages aren't installed yet
        for output in &outputs {
            if super::is_output_stale(output) {
                return Ok(FreshnessResult::SessionStale(format!(
                    "{} created this session",
                    output.display()
                )));
            }
        }

        // Check for missing outputs
        for output in &outputs {
            if !output.exists() {
                return Ok(FreshnessResult::OutputsMissing(format!(
                    "{} does not exist",
                    output.display()
                )));
            }
        }

        let sources_mtime = Self::last_modified(&sources)?;
        let outputs_mtime = Self::last_modified(&outputs)?;

        match (sources_mtime, outputs_mtime) {
            (Some(src), Some(out)) if src > out => {
                // Find which source is newest to provide a helpful reason
                let newest_source = sources
                    .iter()
                    .filter(|p| p.exists())
                    .filter_map(|p| {
                        let mtime = if p.is_dir() {
                            Self::newest_file_in_dir(p, 3)
                        } else {
                            p.metadata().ok().and_then(|m| m.modified().ok())
                        };
                        mtime.map(|m| (p, m))
                    })
                    .max_by_key(|(_, m)| *m)
                    .map(|(p, _)| p.display().to_string())
                    .unwrap_or_else(|| "sources".to_string());
                Ok(FreshnessResult::Stale(format!(
                    "{newest_source} is newer than outputs"
                )))
            }
            (Some(_), Some(_)) => Ok(FreshnessResult::Fresh),
            (_, None) => Ok(FreshnessResult::Stale(
                "could not determine modification time of outputs".to_string(),
            )),
            (None, _) => Ok(FreshnessResult::NoSources),
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
        timeout: Option<std::time::Duration>,
    ) -> Result<()> {
        let cwd = match cmd.cwd.clone() {
            Some(dir) => dir,
            None => std::env::current_dir()?,
        };

        let mut runner = CmdLineRunner::new(&cmd.program)
            .args(&cmd.args)
            .current_dir(cwd);

        // Apply timeout if configured
        if let Some(timeout) = timeout {
            runner = runner.with_timeout(timeout);
        }

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
