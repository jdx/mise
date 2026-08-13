use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, bail};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::cmd::CmdLineRunner;
use crate::config::config_file::ConfigFile;
use crate::config::{Config, Settings};
use crate::task::monorepo_scope;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::style;

type StepOutput = (DepsStepResult, Vec<PathBuf>);
type JobOutput = Result<(String, DepsStepResult, Vec<PathBuf>), (String, eyre::Report)>;

#[derive(Debug)]
struct ScopedDepsProvider {
    inner: Box<dyn DepsProvider>,
    id: String,
    depends: Vec<String>,
    scope: String,
    scoped_ids: HashSet<String>,
    fallback_ids: HashSet<String>,
    qualified_fallback_ids: HashMap<String, String>,
}

impl ScopedDepsProvider {
    fn new(
        inner: Box<dyn DepsProvider>,
        id: String,
        scope: &str,
        scoped_ids: &HashSet<String>,
        fallback_ids: &HashSet<String>,
        qualified_fallback_ids: &HashMap<String, String>,
    ) -> Self {
        let depends = inner
            .depends()
            .into_iter()
            .map(|dep| {
                if dep.starts_with("//") {
                    if scoped_ids.contains(&dep) {
                        dep
                    } else if let Some(fallback_id) = qualified_fallback_ids.get(&dep) {
                        fallback_id.clone()
                    } else {
                        dep
                    }
                } else {
                    let scoped_dep = format!("{scope}:{dep}");
                    if scoped_ids.contains(&scoped_dep) || !fallback_ids.contains(&dep) {
                        scoped_dep
                    } else {
                        dep
                    }
                }
            })
            .collect();
        Self {
            inner,
            id,
            depends,
            scope: scope.to_string(),
            scoped_ids: scoped_ids.clone(),
            fallback_ids: fallback_ids.clone(),
            qualified_fallback_ids: qualified_fallback_ids.clone(),
        }
    }
}

impl DepsProvider for ScopedDepsProvider {
    fn base(&self) -> &super::providers::ProviderBase {
        self.inner.base()
    }

    fn rendered(&self, effective_env: &BTreeMap<String, String>) -> Result<Box<dyn DepsProvider>> {
        Ok(Box::new(Self::new(
            self.inner.rendered(effective_env)?,
            self.id.clone(),
            &self.scope,
            &self.scoped_ids,
            &self.fallback_ids,
            &self.qualified_fallback_ids,
        )))
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn sources(&self) -> Vec<PathBuf> {
        self.inner.sources()
    }

    fn outputs(&self) -> Vec<PathBuf> {
        self.inner.outputs()
    }

    fn optional_outputs(&self) -> Vec<PathBuf> {
        self.inner.optional_outputs()
    }

    fn install_command(&self) -> Result<super::DepsCommand> {
        self.inner.install_command()
    }

    fn applicability(&self) -> super::DepsProviderApplicability {
        self.inner.applicability()
    }

    fn is_auto(&self) -> bool {
        self.inner.is_auto()
    }

    fn depends(&self) -> Vec<String> {
        self.depends.clone()
    }

    fn add_command(&self, packages: &[&str], dev: bool) -> Result<super::DepsCommand> {
        self.inner.add_command(packages, dev)
    }

    fn remove_command(&self, packages: &[&str]) -> Result<super::DepsCommand> {
        self.inner.remove_command(packages)
    }
}

use super::DepsProviderApplicability::Applicable;
use super::deps_ordering::DepsOrdering;
use super::providers::{
    AubeDepsProvider, BunDepsProvider, BundlerDepsProvider, ComposerDepsProvider,
    CustomDepsProvider, DartDepsProvider, DenoDepsProvider, GitSubmoduleDepsProvider,
    GoDepsProvider, NpmDepsProvider, PipDepsProvider, PnpmDepsProvider, PoetryDepsProvider,
    UvDepsProvider, YarnDepsProvider,
};
use super::rule::BUILTIN_PROVIDERS;
use super::state::{self, DepsState};
use super::{DepsProvider, DepsProviderApplicability, FreshnessResult};

/// Options for running deps steps
#[derive(Debug, Default)]
pub struct DepsOptions {
    /// Only check if deps install is needed, don't run commands
    pub dry_run: bool,
    /// Force run all deps steps even if outputs are fresh
    pub force: bool,
    /// Run specific deps rule(s) only
    pub only: Option<Vec<String>>,
    /// Skip specific deps rule(s)
    pub skip: Vec<String>,
    /// Environment variables to pass to deps commands (e.g., toolset PATH)
    pub env: BTreeMap<String, String>,
    /// If true, only run providers with auto=true
    pub auto_only: bool,
}

/// Result of a deps step
#[derive(Debug)]
pub enum DepsStepResult {
    /// Step ran successfully
    Ran(String),
    /// Step would have run (dry-run mode), with reason why it's stale
    WouldRun(String, String),
    /// Step was skipped because outputs are fresh
    Fresh(String),
    /// Step was skipped by user request
    Skipped(String),
    /// Step failed
    Failed(String),
}

/// Result of running all deps steps
#[derive(Debug)]
pub struct DepsResult {
    pub steps: Vec<DepsStepResult>,
}

/// A deps job ready to be executed
struct DepsJob {
    id: String,
    cmd: super::DepsCommand,
    command_hash: String,
    outputs: Vec<PathBuf>,
    depends: Vec<String>,
    timeout: Option<std::time::Duration>,
}

impl DepsResult {
    /// Returns true if any steps ran or would have run
    pub fn had_work(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, DepsStepResult::Ran(_) | DepsStepResult::WouldRun(_, _)))
    }
}

/// Engine that discovers and runs deps providers
pub struct DepsEngine {
    providers: Vec<Box<dyn DepsProvider>>,
}

impl DepsEngine {
    /// Create a new DepsEngine, discovering all configured providers.
    pub fn new(config: &Config) -> Result<Self> {
        let providers = Self::discover_providers(config)?;
        // Inactive-only config is diagnostic state and cannot run.
        if providers
            .iter()
            .any(|provider| matches!(provider.applicability(), Applicable))
        {
            Settings::get().ensure_experimental("deps")?;
        }
        Ok(Self { providers })
    }

    /// Create an engine containing providers from every explicit monorepo config root.
    ///
    /// Provider IDs are qualified with their config root (for example,
    /// `//apps/api:uv`) so the same provider name can be used in multiple roots.
    pub fn new_monorepo(
        config: &Config,
        config_files: impl IntoIterator<Item = Arc<dyn ConfigFile>>,
    ) -> Result<Self> {
        Self::new_monorepo_with_fallback(config, config_files, &HashSet::new(), &HashMap::new())
    }

    fn new_monorepo_with_fallback(
        config: &Config,
        config_files: impl IntoIterator<Item = Arc<dyn ConfigFile>>,
        fallback_ids: &HashSet<String>,
        qualified_fallback_ids: &HashMap<String, String>,
    ) -> Result<Self> {
        let monorepo_root = config
            .monorepo_root()
            .ok_or_else(|| eyre::eyre!("no config file in scope sets monorepo_root = true"))?;
        let mut scoped_providers: Vec<(Box<dyn DepsProvider>, String, String)> = vec![];
        let mut provider_indices: HashMap<String, usize> = HashMap::new();
        let config_files: Vec<_> = config_files
            .into_iter()
            .map(|cf| {
                let deps_config = cf.deps_config()?;
                Ok((cf, deps_config))
            })
            .collect::<Result<_>>()?;
        let mut disabled_by_root: HashMap<PathBuf, HashSet<String>> = HashMap::new();

        // Layered config files share one provider namespace at each config root.
        // Collect disables first so their behavior does not depend on file order.
        for (cf, deps_config) in &config_files {
            let Some(config_project_root) = cf.project_root() else {
                continue;
            };
            if !config_project_root.starts_with(&monorepo_root) {
                continue;
            }
            if let Some(deps_config) = deps_config {
                disabled_by_root
                    .entry(cf.config_root())
                    .or_default()
                    .extend(deps_config.disable.iter().cloned());
            }
        }

        for (cf, deps_config) in config_files {
            let Some(config_project_root) = cf.project_root() else {
                continue;
            };
            if !config_project_root.starts_with(&monorepo_root) {
                continue;
            }
            let Some(deps_config) = deps_config else {
                continue;
            };

            let config_root = cf.config_root();
            let Some(scope) = monorepo_scope(&monorepo_root, &config_root) else {
                continue;
            };

            for (id, provider_config) in &deps_config.providers {
                if disabled_by_root
                    .get(&config_root)
                    .is_some_and(|disabled| disabled.contains(id))
                {
                    continue;
                }

                let scoped_id = format!("{scope}:{id}");
                if let Some(provider) =
                    Self::build_provider(id, &config_root, provider_config.clone())
                {
                    if let Some(index) = provider_indices.get(&scoped_id).copied() {
                        // Before inactive providers were retained, layered
                        // discovery selected the first applicable definition.
                        // Preserve that precedence while retaining the
                        // highest-precedence inactive definition only when no
                        // applicable definition exists.
                        if !matches!(scoped_providers[index].0.applicability(), Applicable)
                            && matches!(provider.applicability(), Applicable)
                        {
                            scoped_providers[index] = (provider, scoped_id, scope.clone());
                        }
                    } else {
                        provider_indices.insert(scoped_id.clone(), scoped_providers.len());
                        scoped_providers.push((provider, scoped_id, scope.clone()));
                    }
                }
            }
        }

        let applicable_ids = scoped_providers
            .iter()
            .filter(|(provider, _, _)| matches!(provider.applicability(), Applicable))
            .map(|(_, scoped_id, _)| scoped_id.clone())
            .collect();
        let providers: Vec<Box<dyn DepsProvider>> = scoped_providers
            .into_iter()
            .map(|(provider, scoped_id, scope)| {
                Box::new(ScopedDepsProvider::new(
                    provider,
                    scoped_id,
                    &scope,
                    &applicable_ids,
                    fallback_ids,
                    qualified_fallback_ids,
                )) as Box<dyn DepsProvider>
            })
            .collect();
        if providers
            .iter()
            .any(|provider| matches!(provider.applicability(), Applicable))
        {
            Settings::get().ensure_experimental("deps")?;
        }
        Ok(Self { providers })
    }

    /// Create a monorepo engine for task execution while preserving providers
    /// from the current project plus global and system configuration.
    pub fn new_task_monorepo(
        config: &Config,
        config_files: impl IntoIterator<Item = Arc<dyn ConfigFile>>,
    ) -> Result<Self> {
        let mut providers = Self::discover_providers(config)?;
        let fallback_ids = providers
            .iter()
            .filter(|provider| matches!(provider.applicability(), Applicable))
            .map(|provider| provider.id().to_string())
            .collect();
        let monorepo_root = config
            .monorepo_root()
            .ok_or_else(|| eyre::eyre!("no config file in scope sets monorepo_root = true"))?;
        let qualified_fallback_ids = config
            .project_root
            .as_deref()
            .and_then(|project_root| {
                let scope = monorepo_scope(&monorepo_root, project_root)?;
                Some(
                    providers
                        .iter()
                        .filter(|provider| matches!(provider.applicability(), Applicable))
                        .filter(|provider| provider.base().project_root.as_path() == project_root)
                        .map(|provider| {
                            (
                                format!("{scope}:{}", provider.id()),
                                provider.id().to_string(),
                            )
                        })
                        .collect(),
                )
            })
            .unwrap_or_default();
        let mut engine = Self::new_monorepo_with_fallback(
            config,
            config_files,
            &fallback_ids,
            &qualified_fallback_ids,
        )?;
        providers.append(&mut engine.providers);
        if providers
            .iter()
            .any(|provider| matches!(provider.applicability(), Applicable))
        {
            Settings::get().ensure_experimental("deps")?;
        }
        Ok(Self { providers })
    }

    /// Discover all configured deps providers for the current project.
    ///
    /// Each config file's deps providers are scoped to that config file's directory.
    /// For example, a `[deps.pnpm]` defined in the root `mise.toml` only applies when
    /// running from the root directory, not from subdirectories.
    fn discover_providers(config: &Config) -> Result<Vec<Box<dyn DepsProvider>>> {
        let project_root = config
            .project_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let mut providers: Vec<Box<dyn DepsProvider>> = vec![];
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut disabled: Vec<String> = vec![];

        // Process each config file's deps config independently, using that
        // config file's directory as the project root for its providers.
        // Only include config files that belong to the current project root
        // (skip config files outside the current project root, e.g. from parent directories).
        for cf in config.config_files.values() {
            let Some(deps_config) = cf.deps_config()? else {
                continue;
            };

            // Skip config files from parent directories - deps providers
            // should only run from the directory where they are defined.
            // Global/system configs (project_root() == None) are always included.
            if let Some(cf_project_root) = cf.project_root()
                && cf_project_root != project_root
            {
                continue;
            }

            // Collect disable list scoped to this project root
            disabled.extend(deps_config.disable.iter().cloned());

            let config_root = cf.config_root();

            for (id, provider_config) in &deps_config.providers {
                // Skip duplicate provider IDs (first config file wins)
                if !seen_ids.insert(id.clone()) {
                    continue;
                }

                if let Some(provider) =
                    Self::build_provider(id, &config_root, provider_config.clone())
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
    pub(super) fn build_provider(
        id: &str,
        config_root: &Path,
        provider_config: super::rule::DepsProviderConfig,
    ) -> Option<Box<dyn DepsProvider>> {
        if BUILTIN_PROVIDERS.contains(&id) {
            match id {
                "npm" => Some(Box::new(NpmDepsProvider::new(config_root, provider_config))),
                "yarn" => Some(Box::new(YarnDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "pnpm" => Some(Box::new(PnpmDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "bun" => Some(Box::new(BunDepsProvider::new(config_root, provider_config))),
                "aube" => Some(Box::new(AubeDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "deno" => Some(Box::new(DenoDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "go" => Some(Box::new(GoDepsProvider::new(config_root, provider_config))),
                "pip" => Some(Box::new(PipDepsProvider::new(config_root, provider_config))),
                "poetry" => Some(Box::new(PoetryDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "uv" => Some(Box::new(UvDepsProvider::new(config_root, provider_config))),
                "bundler" => Some(Box::new(BundlerDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "composer" => Some(Box::new(ComposerDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                "dart" => Some(Box::new(DartDepsProvider::new(
                    "dart",
                    config_root,
                    provider_config,
                ))),
                "flutter" => Some(Box::new(DartDepsProvider::new(
                    "flutter",
                    config_root,
                    provider_config,
                ))),
                "git-submodule" => Some(Box::new(GitSubmoduleDepsProvider::new(
                    config_root,
                    provider_config,
                ))),
                _ => None,
            }
        } else {
            Some(Box::new(CustomDepsProvider::new(
                id.to_string(),
                provider_config,
                config_root,
            )))
        }
    }

    /// List all discovered providers, including inactive providers.
    pub fn list_providers(&self) -> Vec<&dyn DepsProvider> {
        self.providers.iter().map(|p| p.as_ref()).collect()
    }

    /// Find a specific provider by ID
    pub fn find_provider(&self, id: &str) -> Option<&dyn DepsProvider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    /// Check freshness for a specific provider (public API for --explain)
    pub fn check_provider_freshness(
        &self,
        provider: &dyn DepsProvider,
        effective_env: &BTreeMap<String, String>,
    ) -> Result<FreshnessResult> {
        let provider = provider.rendered(effective_env)?;
        let command_hash = provider.install_command()?.freshness_hash();
        self.check_freshness(provider.as_ref(), &command_hash)
    }

    /// Check if any auto-enabled provider has stale outputs (without running)
    /// Returns the IDs and reasons of stale providers
    pub fn check_staleness(&self, effective_env: &BTreeMap<String, String>) -> Vec<(&str, String)> {
        self.providers
            .iter()
            .filter(|p| p.is_auto())
            .filter(|p| matches!(p.applicability(), Applicable))
            .filter_map(|p| {
                let result = p.rendered(effective_env).and_then(|rendered| {
                    let command_hash = rendered.install_command()?.freshness_hash();
                    self.check_freshness(rendered.as_ref(), &command_hash)
                });
                match result {
                    Ok(r) if !r.is_fresh() => Some((p.id(), r.reason().to_string())),
                    _ => None,
                }
            })
            .collect()
    }

    /// Reject explicitly selected providers that cannot run.
    pub fn validate_selection(&self, only: Option<&[String]>, skip: &[String]) -> Result<()> {
        Self::validate_provider_selection(&self.providers, only, skip)
    }

    fn validate_provider_selection(
        providers: &[Box<dyn DepsProvider>],
        only: Option<&[String]>,
        skip: &[String],
    ) -> Result<()> {
        let Some(only) = only else {
            return Ok(());
        };
        for id in only {
            if skip.contains(id) {
                continue;
            }
            let Some(provider) = providers.iter().find(|provider| provider.id() == id) else {
                continue;
            };
            if let DepsProviderApplicability::Inactive(reason) = provider.applicability() {
                bail!("provider '{}' is inactive: {reason}", provider.id());
            }
        }
        Ok(())
    }

    /// Run all stale deps steps, respecting dependency ordering
    pub async fn run(&self, opts: DepsOptions) -> Result<DepsResult> {
        let mut results = vec![];

        Self::validate_provider_selection(&self.providers, opts.only.as_deref(), &opts.skip)?;

        let is_selected = |provider: &dyn DepsProvider| {
            (!opts.auto_only || provider.is_auto())
                && !opts.skip.iter().any(|id| id == provider.id())
                && opts
                    .only
                    .as_ref()
                    .is_none_or(|only| only.iter().any(|id| id == provider.id()))
        };
        let inactive_ids: HashMap<String, String> = self
            .providers
            .iter()
            .filter(|provider| is_selected(provider.as_ref()))
            .filter_map(|provider| match provider.applicability() {
                Applicable => None,
                DepsProviderApplicability::Inactive(reason) => {
                    Some((provider.id().to_string(), reason))
                }
            })
            .collect();

        // Collect stale providers before applying dependency blocking so a
        // fresh provider remains satisfied even if one of its deps is inactive.
        let mut candidates: Vec<(DepsJob, String)> = vec![];
        // Track IDs of providers that are fresh/skipped (treated as already satisfied for deps)
        let mut satisfied_ids: HashSet<String> = HashSet::new();
        let mut providers: Vec<Box<dyn DepsProvider>> = vec![];

        for provider in &self.providers {
            let id = provider.id().to_string();

            // Check auto_only filter
            if opts.auto_only && !provider.is_auto() {
                trace!("deps step {} is not auto, skipping", id);
                results.push(DepsStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            // Check skip list
            if opts.skip.contains(&id) {
                results.push(DepsStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            // Check only list
            if let Some(ref only) = opts.only
                && !only.contains(&id)
            {
                results.push(DepsStepResult::Skipped(id.clone()));
                satisfied_ids.insert(id);
                continue;
            }

            if let DepsProviderApplicability::Inactive(_) = provider.applicability() {
                trace!("deps step {} is inactive, skipping", id);
                results.push(DepsStepResult::Skipped(id.clone()));
                continue;
            }

            let provider = provider.rendered(&opts.env)?;
            let cmd = provider.install_command()?;
            let command_hash = cmd.freshness_hash();
            let freshness = if opts.force {
                FreshnessResult::Forced
            } else {
                self.check_freshness(provider.as_ref(), &command_hash)?
            };

            if !freshness.is_fresh() {
                // Carry both required and optional outputs so session-staleness
                // can be cleared on whichever paths actually exist after the run.
                let outputs: Vec<PathBuf> = provider
                    .outputs()
                    .into_iter()
                    .chain(provider.optional_outputs())
                    .collect();
                let depends = provider.depends();
                let timeout = provider.timeout();
                let reason = freshness.reason().to_string();

                candidates.push((
                    DepsJob {
                        id,
                        cmd,
                        command_hash,
                        outputs,
                        depends,
                        timeout,
                    },
                    reason,
                ));
            } else {
                trace!("deps step {} is fresh, skipping", id);
                results.push(DepsStepResult::Fresh(id.clone()));
                satisfied_ids.insert(id);
            }
            providers.push(provider);
        }

        let mut inactive_blocked_ids: HashSet<String> = inactive_ids.keys().cloned().collect();
        loop {
            let previous_len = inactive_blocked_ids.len();
            for (job, _) in &candidates {
                if job
                    .depends
                    .iter()
                    .any(|dependency| inactive_blocked_ids.contains(dependency))
                {
                    inactive_blocked_ids.insert(job.id.clone());
                }
            }
            if inactive_blocked_ids.len() == previous_len {
                break;
            }
        }

        let mut to_run: Vec<DepsJob> = vec![];
        for (job, reason) in candidates {
            if inactive_blocked_ids.contains(&job.id) {
                if let Some((dependency, reason)) = job
                    .depends
                    .iter()
                    .find_map(|dependency| inactive_ids.get(dependency).map(|r| (dependency, r)))
                {
                    warn!(
                        "deps provider '{}' skipped because dependency '{}' is inactive: {}",
                        job.id, dependency, reason
                    );
                } else {
                    warn!(
                        "deps provider '{}' skipped due to inactive dependency",
                        job.id
                    );
                }
                results.push(DepsStepResult::Skipped(job.id));
            } else if opts.dry_run {
                results.push(DepsStepResult::WouldRun(job.id, reason));
            } else {
                to_run.push(job);
            }
        }

        // Run stale providers with dependency ordering
        if !to_run.is_empty() {
            // Retain the fingerprint of the exact command that will run. Rebuilding
            // commands after execution could observe a concurrently changed config.
            let command_hashes: HashMap<String, String> = to_run
                .iter()
                .map(|job| (job.id.clone(), job.command_hash.clone()))
                .collect();
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

            // Save content hashes and existing optional outputs for each
            // successfully ran provider.
            for step in &results {
                if let DepsStepResult::Ran(id) = step
                    && let Some(provider) = providers.iter().find(|p| p.id() == id)
                {
                    let project_root = &provider.base().project_root;
                    let sources = provider.sources();
                    if let Ok(hashes) = state::hash_sources(&sources, project_root) {
                        let seen: Vec<String> = provider
                            .optional_outputs()
                            .iter()
                            .filter(|p| p.exists())
                            .map(|p| state::relative_str(p, project_root))
                            .collect();
                        let mut st = DepsState::load(project_root);
                        let provider_id = provider.base().id.as_str();
                        st.set_hashes(provider_id, hashes);
                        st.set_seen_outputs(provider_id, seen);
                        if let Some(command_hash) = command_hashes.get(id) {
                            st.set_command_hash(provider_id, command_hash.clone());
                        }
                        if let Err(e) = st.save(project_root) {
                            warn!("failed to save deps state: {e}");
                        }
                    }
                }
            }
        }

        Ok(DepsResult { steps: results })
    }

    /// Simple parallel execution (no dependency ordering)
    async fn run_parallel(
        &self,
        to_run: Vec<DepsJob>,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<Vec<(DepsStepResult, Vec<PathBuf>)>> {
        let mpr = MultiProgressReport::get();

        let to_run_with_context: Vec<_> = to_run
            .into_iter()
            .map(|job| (job, mpr.clone(), toolset_env.clone()))
            .collect();

        crate::parallel::parallel(to_run_with_context, |(job, mpr, toolset_env)| async move {
            let (stdout_prefix, stderr_prefix) = Self::deps_prefixes(&job.id);
            let pr = mpr.add(&stderr_prefix);
            match Self::execute_command(
                &job.cmd,
                &toolset_env,
                job.timeout,
                Some((&stdout_prefix, &stderr_prefix)),
                Some(pr.as_ref()),
            ) {
                Ok(()) => {
                    pr.finish_with_message("done".to_string());
                    Ok((DepsStepResult::Ran(job.id), job.outputs))
                }
                Err(e) => {
                    pr.finish_with_message(format!("failed: {e}"));
                    Err(e)
                }
            }
        })
        .await
    }

    /// Dependency-aware execution using Kahn's algorithm
    async fn run_with_deps(
        &self,
        to_run: Vec<DepsJob>,
        satisfied_ids: &HashSet<String>,
        toolset_env: &BTreeMap<String, String>,
    ) -> Result<Vec<StepOutput>> {
        let mpr = MultiProgressReport::get();
        let mut results: Vec<StepOutput> = vec![];
        let mut errors: Vec<(String, String)> = vec![];

        // Build jobs map for lookup
        let running_ids: HashSet<String> = to_run.iter().map(|j| j.id.clone()).collect();
        let mut jobs: HashMap<String, DepsJob> = HashMap::new();
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
                            "deps provider '{}' depends on '{}' which is not configured",
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

        let mut deps = DepsOrdering::new(&dep_specs)?;

        // Report blocked providers (cycles)
        for blocked_id in deps.blocked_providers() {
            warn!(
                "deps provider '{}' is blocked due to dependency cycle",
                blocked_id
            );
            if let Some(job) = jobs.remove(&blocked_id) {
                results.push((DepsStepResult::Skipped(job.id), vec![]));
            }
        }

        let mut rx = deps.subscribe();
        let semaphore = Arc::new(Semaphore::new(crate::jobs::normalize(Settings::get().jobs)));
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
                            warn!("deps provider '{}' failed: {}", id, e);
                            errors.push((id.clone(), e.to_string()));
                            results.push((DepsStepResult::Failed(id.clone()), vec![]));
                            deps.complete_failure(&id);
                            for blocked_id in deps.blocked_providers() {
                                if let Some(job) = jobs.remove(&blocked_id) {
                                    warn!(
                                        "deps provider '{}' skipped due to failed dependency",
                                        job.id
                                    );
                                    results.push((DepsStepResult::Skipped(job.id), vec![]));
                                }
                            }
                        }
                        Err(e) => {
                            // JoinError — task panicked or was cancelled
                            if let Some(id) = inflight.remove(&e.id()) {
                                warn!("deps provider '{}' panicked: {}", id, e);
                                errors.push((id.clone(), e.to_string()));
                                results.push((DepsStepResult::Failed(id.clone()), vec![]));
                                deps.complete_failure(&id);
                                for blocked_id in deps.blocked_providers() {
                                    if let Some(job) = jobs.remove(&blocked_id) {
                                        warn!(
                                            "deps provider '{}' skipped due to failed dependency",
                                            job.id
                                        );
                                        results.push((DepsStepResult::Skipped(job.id), vec![]));
                                    }
                                }
                            } else {
                                warn!("deps task join error (unknown task): {e}");
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
                        let id = job.id;
                        let (stdout_prefix, stderr_prefix) = Self::deps_prefixes(&id);
                        let pr = mpr.add(&stderr_prefix);
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            Self::execute_command(
                                &job.cmd,
                                &toolset_env,
                                job.timeout,
                                Some((&stdout_prefix, &stderr_prefix)),
                                Some(pr.as_ref()),
                            )
                        }));
                        drop(permit);

                        match result {
                            Ok(Ok(())) => {
                                pr.finish_with_message("done".to_string());
                                let step = DepsStepResult::Ran(id.clone());
                                Ok((id, step, job.outputs))
                            }
                            Ok(Err(e)) => {
                                pr.finish_with_message(format!("failed: {e}"));
                                Err((id, e))
                            }
                            Err(_) => {
                                pr.finish_with_message("panicked".to_string());
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
                    warn!("deps provider '{}' failed: {}", id, e);
                    errors.push((id.clone(), e.to_string()));
                    results.push((DepsStepResult::Failed(id), vec![]));
                }
                Err(e) => {
                    if let Some(id) = inflight.remove(&e.id()) {
                        warn!("deps provider '{}' panicked: {}", id, e);
                        errors.push((id.clone(), e.to_string()));
                        results.push((DepsStepResult::Failed(id), vec![]));
                    } else {
                        warn!("deps task join error (unknown task): {e}");
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
            return Err(eyre::eyre!("deps providers failed:\n{details}"));
        }
        Ok(results)
    }

    /// Check if a provider's outputs are fresh relative to its sources.
    ///
    /// Required outputs (`provider.outputs()`) must always exist. Optional
    /// outputs (`provider.optional_outputs()`) are only enforced once they've
    /// been observed on a previous successful run — this lets built-in
    /// providers declare a canonical output (`.venv`, `vendor/bundle`, etc.)
    /// that detects post-install deletion without forcing a re-run for
    /// projects that never produce that output.
    ///
    /// Uses blake3 content hashing with persistent state. On first run (no
    /// stored hashes), the provider is always considered stale.
    pub fn check_freshness(
        &self,
        provider: &dyn DepsProvider,
        command_hash: &str,
    ) -> Result<FreshnessResult> {
        if let DepsProviderApplicability::Inactive(reason) = provider.applicability() {
            return Err(eyre::eyre!(
                "deps provider '{}' is inactive: {reason}",
                provider.id()
            ));
        }

        let sources = provider.sources();
        let outputs = provider.outputs();
        let optional_outputs = provider.optional_outputs();

        let project_root = &provider.base().project_root;
        let st = DepsState::load(project_root);
        let provider_id = provider.base().id.as_str();

        // Session-stale check applies to any output that currently exists,
        // regardless of whether it was required or optional.
        for output in outputs.iter().chain(optional_outputs.iter()) {
            if super::is_output_stale(output) {
                return Ok(FreshnessResult::Stale(
                    "output created this session".to_string(),
                ));
            }
        }

        // Required outputs must exist whenever they are declared.
        for output in &outputs {
            if !output.exists() {
                return Ok(FreshnessResult::OutputsMissing);
            }
        }

        // Optional outputs are enforced only for paths that existed at the
        // last successful run (recorded in state). This catches deletion
        // (e.g. `rm -rf .venv` after `uv sync`) without forcing a re-run for
        // providers whose canonical output is intentionally absent.
        if let Some(seen) = st.get_seen_outputs(provider_id) {
            for output in &optional_outputs {
                let rel = state::relative_str(output, project_root);
                if seen.iter().any(|p| p == &rel) && !output.exists() {
                    return Ok(FreshnessResult::OutputsMissing);
                }
            }
        }

        // A provider with neither sources nor outputs has no freshness signal —
        // always run it (matches pre-PR custom-hook behavior).
        if sources.is_empty() && outputs.is_empty() && optional_outputs.is_empty() {
            return Ok(FreshnessResult::Stale(
                "no sources or outputs defined".to_string(),
            ));
        }

        // Existing outputs without sources are fresh by definition. Command fingerprints only
        // invalidate providers whose source state can otherwise be compared across runs.
        if sources.is_empty() {
            return Ok(FreshnessResult::NoSources);
        }

        match st.get_command_hash(provider_id) {
            Some(stored_hash) if stored_hash != command_hash => {
                return Ok(FreshnessResult::Stale(
                    "provider command changed".to_string(),
                ));
            }
            Some(_) => {}
            None if st.get_hashes(provider_id).is_some()
                || st.get_seen_outputs(provider_id).is_some() =>
            {
                // State written by mise versions before command hashing cannot prove
                // that the provider definition is unchanged. Re-run it once to migrate.
                return Ok(FreshnessResult::Stale(
                    "provider command changed".to_string(),
                ));
            }
            None => {
                return Ok(FreshnessResult::Stale("no previous state".to_string()));
            }
        }

        let current_hashes = state::hash_sources(&sources, project_root)?;

        match st.get_hashes(provider_id) {
            Some(stored_hashes) => {
                // Check for changed files
                for (path, hash) in &current_hashes {
                    match stored_hashes.get(path.as_str()) {
                        Some(stored_hash) if stored_hash == hash => {}
                        Some(_) => {
                            return Ok(FreshnessResult::Stale(format!("{path} changed")));
                        }
                        None => {
                            return Ok(FreshnessResult::Stale(format!("{path} added")));
                        }
                    }
                }
                // Check for removed files
                for path in stored_hashes.keys() {
                    if !current_hashes.contains_key(path) {
                        return Ok(FreshnessResult::Stale(format!("{path} removed")));
                    }
                }
                Ok(FreshnessResult::Fresh)
            }
            None => {
                // No stored state — first run, consider stale
                Ok(FreshnessResult::Stale("no previous state".to_string()))
            }
        }
    }

    /// Execute a deps command (static version for parallel execution)
    pub(crate) fn execute_command(
        cmd: &super::DepsCommand,
        toolset_env: &BTreeMap<String, String>,
        timeout: Option<std::time::Duration>,
        prefixes: Option<(&str, &str)>,
        progress: Option<&dyn SingleReport>,
    ) -> Result<()> {
        let cwd = match cmd.cwd.clone() {
            Some(dir) => dir,
            None => std::env::current_dir()?,
        };

        // cmd.args is [shell flags.., run]; route the run body through
        // cmd_body_args so an inner-quoted command survives `cmd /c` on Windows.
        // On Unix / non-cmd shells this is byte-identical to .args(&cmd.args).
        let mut runner = CmdLineRunner::new(&cmd.program);
        runner = match cmd.args.split_last() {
            Some((body, flags)) => runner.cmd_body_args(flags, body),
            None => runner,
        };
        runner = runner.current_dir(cwd);

        // Apply timeout if configured
        if let Some(timeout) = timeout {
            runner = runner.with_timeout(timeout);
        }

        // Apply toolset environment (includes PATH with installed tools)
        for (k, v) in toolset_env {
            runner = runner.env(k, v);
        }

        // Apply command-specific environment (can override toolset env).
        // Config-file templates have already been rendered with that file's
        // config_root and environment context during provider discovery.
        for (k, v) in &cmd.env {
            runner = runner.env(k, v);
        }

        // Use raw output for better UX during dependency installation
        if Settings::get().raw {
            runner = runner.raw(true);
        }

        if let Some((stdout_prefix, stderr_prefix)) = prefixes
            && !Settings::get().raw
        {
            let stdout_prefix = stdout_prefix.to_string();
            let stderr_prefix = stderr_prefix.to_string();
            // Suppress provider command stream output when -q/--quiet is set,
            // matching `mise install -q` behavior. The progress indicator and
            // logger-routed status messages still respect the quiet level.
            let quiet = Settings::get().quiet;
            runner = runner
                .with_on_stdout(move |line| {
                    if let Some(progress) = progress {
                        progress.set_message(line.clone());
                    }
                    if quiet {
                        return;
                    }
                    if console::colors_enabled() {
                        prefix_println!(stdout_prefix, "{line}\x1b[0m");
                    } else {
                        prefix_println!(stdout_prefix, "{line}");
                    }
                })
                .with_on_stderr(move |line| {
                    if quiet {
                        return;
                    }
                    if console::colors_enabled_stderr() {
                        prefix_eprintln!(stderr_prefix, "{line}\x1b[0m");
                    } else {
                        prefix_eprintln!(stderr_prefix, "{line}");
                    }
                });
        }

        runner.execute()?;
        Ok(())
    }

    fn deps_prefixes(id: &str) -> (String, String) {
        let name = format!("deps.{id}");
        let prefix = format!("[{name}]");
        (
            style::prefix(&prefix, &name, false),
            style::prefix(prefix, name, true),
        )
    }
}
