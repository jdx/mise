use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use crate::backend::Backend;
use crate::cli::args::BackendArg;
use crate::config::Config;
use crate::config::settings::{Settings, SettingsStatusMissingTools};
use crate::env::{PATH_KEY, TERM_WIDTH};
use crate::env_diff::EnvMap;
use crate::errors::Error;
use crate::hooks::Hooks;
use crate::install_context::InstallContext;
use crate::path_env::PathEnv;
use crate::registry::tool_enabled;
use crate::{backend, parallel};
pub use builder::ToolsetBuilder;
use console::truncate_str;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use outdated_info::OutdatedInfo;
pub use outdated_info::is_outdated_version;
use tokio::sync::OnceCell;

pub use tool_request::ToolRequest;
pub use tool_request_set::{ToolRequestSet, ToolRequestSetBuilder};
pub use tool_source::ToolSource;
pub use tool_version::{ResolveOptions, ToolVersion};
pub use tool_version_list::ToolVersionList;
pub use tool_version_options::{ToolVersionOptions, parse_tool_options};

use helpers::TVTuple;
pub use install_options::InstallOptions;

mod builder;
mod helpers;
mod install_options;
pub(crate) mod install_state;
pub(crate) mod outdated_info;
pub(crate) mod tool_request;
mod tool_request_set;
mod tool_source;
mod tool_version;
mod tool_version_list;
mod tool_version_options;
mod toolset_env;
mod toolset_install;
mod toolset_paths;

/// a toolset is a collection of tools for various plugins
///
/// one example is a .tool-versions file
/// the idea is that we start with an empty toolset, then
/// merge in other toolsets from various sources
#[derive(Debug, Default, Clone)]
pub struct Toolset {
    pub versions: IndexMap<Arc<BackendArg>, ToolVersionList>,
    pub source: Option<ToolSource>,
    tera_ctx: OnceCell<tera::Context>,
}

impl Toolset {
    pub fn new(source: ToolSource) -> Self {
        Self {
            source: Some(source),
            ..Default::default()
        }
    }

    pub fn add_version(&mut self, tvr: ToolRequest) {
        let ba = tvr.ba();
        if self.is_disabled(ba) {
            return;
        }
        let tvl = self
            .versions
            .entry(tvr.ba().clone())
            .or_insert_with(|| ToolVersionList::new(ba.clone(), self.source.clone().unwrap()));
        tvl.requests.push(tvr);
    }

    pub fn merge(&mut self, other: Toolset) {
        let mut versions = other.versions;
        for (plugin, tvl) in self.versions.clone() {
            if !versions.contains_key(&plugin) {
                versions.insert(plugin, tvl);
            }
        }
        versions.retain(|_, tvl| !self.is_disabled(&tvl.backend));
        self.versions = versions;
        self.source = other.source;
    }

    #[async_backtrace::framed]
    pub async fn resolve(&mut self, config: &Arc<Config>) -> eyre::Result<()> {
        self.resolve_with_opts(config, &Default::default()).await
    }

    #[async_backtrace::framed]
    pub async fn resolve_with_opts(
        &mut self,
        config: &Arc<Config>,
        opts: &ResolveOptions,
    ) -> eyre::Result<()> {
        self.list_missing_plugins();
        let versions = self
            .versions
            .clone()
            .into_iter()
            .map(|(ba, tvl)| (config.clone(), ba, tvl.clone(), opts.clone()))
            .collect::<Vec<_>>();
        let tvls = parallel::parallel(versions, |(config, ba, mut tvl, opts)| async move {
            if let Err(err) = tvl.resolve(&config, &opts).await {
                warn!("Failed to resolve tool version list for {ba}: {err}");
            }
            Ok((ba, tvl))
        })
        .await?;
        self.versions = tvls.into_iter().collect();
        Ok(())
    }

    pub fn list_missing_plugins(&self) -> Vec<String> {
        self.versions
            .iter()
            .filter(|(_, tvl)| {
                tvl.versions
                    .first()
                    .map(|tv| tv.request.is_os_supported())
                    .unwrap_or_default()
            })
            .map(|(ba, _)| ba)
            .flat_map(|ba| ba.backend())
            .filter(|b| b.plugin().is_some_and(|p| !p.is_installed()))
            .map(|p| p.id().into())
            .collect()
    }

    /// sets the options on incoming requests to install to whatever is already in the toolset
    /// this handles the use-case where you run `mise use ubi:cilium/cilium-cli` (without CLi options)
    /// but this tool has options inside mise.toml
    fn init_request_options(&self, requests: &mut Vec<ToolRequest>) {
        for tr in requests {
            if let Some(tvl) = self.versions.get(tr.ba()) {
                if tvl.requests.len() != 1 {
                    // TODO: handle this case with multiple versions
                    continue;
                }
                let options = tvl.backend.opts();
                // TODO: tr.options() probably should be Option<ToolVersionOptions>
                // to differentiate between no options and empty options
                // without that it might not be possible to unset the options if they are set
                if tr.options().is_empty() || tr.options() != options {
                    tr.set_options(options);
                }
            }
        }
    }

    #[async_backtrace::framed]
    pub async fn install_all_versions(
        &mut self,
        config: &mut Arc<Config>,
        mut versions: Vec<ToolRequest>,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        if versions.is_empty() {
            return Ok(vec![]);
        }

        // Initialize a footer for the entire install session once (before batching)
        let mpr = MultiProgressReport::get();
        let footer_reason = if opts.dry_run {
            format!("{} (dry-run)", opts.reason)
        } else {
            opts.reason.clone()
        };
        mpr.init_footer(opts.dry_run, &footer_reason, versions.len());

        // Skip hooks in dry-run mode
        if !opts.dry_run {
            // Run pre-install hook
            hooks::run_one_hook(config, self, Hooks::Preinstall, None).await;
        }

        self.init_request_options(&mut versions);
        show_python_install_hint(&versions);

        // Handle dependencies by installing in dependency order
        let mut installed = vec![];
        let mut leaf_deps = get_leaf_dependencies(&versions)?;

        while !leaf_deps.is_empty() {
            if leaf_deps.len() < versions.len() {
                debug!("installing {} leaf tools first", leaf_deps.len());
            }
            versions.retain(|tr| !leaf_deps.contains(tr));
            match self.install_some_versions(config, leaf_deps, opts).await {
                Ok(leaf_versions) => installed.extend(leaf_versions),
                Err(Error::InstallFailed {
                    successful_installations,
                    failed_installations,
                }) => {
                    // Count both successes and failures toward footer progress
                    mpr.footer_inc(successful_installations.len() + failed_installations.len());
                    installed.extend(successful_installations);

                    return Err(Error::InstallFailed {
                        successful_installations: installed,
                        failed_installations,
                    }
                    .into());
                }
                Err(e) => return Err(e.into()),
            }

            leaf_deps = get_leaf_dependencies(&versions)?;
        }

        // Skip config reload and resolve in dry-run mode
        if !opts.dry_run {
            // Reload config and resolve (ignoring errors like the original does)
            trace!("install: reloading config");
            *config = Config::reset().await?;
            trace!("install: resolving");
            if let Err(err) = self.resolve(config).await {
                debug!("error resolving versions after install: {err:#}");
            }
        }

        // Debug logging for successful installations
        if log::log_enabled!(log::Level::Debug) {
            for tv in installed.iter() {
                let backend = tv.backend()?;
                let bin_paths = backend
                    .list_bin_paths(config, tv)
                    .await
                    .map_err(|e| {
                        warn!("Error listing bin paths for {tv}: {e:#}");
                    })
                    .unwrap_or_default();
                debug!("[{tv}] list_bin_paths: {bin_paths:?}");
                let env = backend
                    .exec_env(config, self, tv)
                    .await
                    .map_err(|e| {
                        warn!("Error running exec-env: {e:#}");
                    })
                    .unwrap_or_default();
                if !env.is_empty() {
                    debug!("[{tv}] exec_env: {env:?}");
                }
            }
        }

        // Skip hooks in dry-run mode
        if !opts.dry_run {
            // Run post-install hook (ignoring errors)
            let _ = hooks::run_one_hook(config, self, Hooks::Postinstall, None).await;
        }

        // Finish the global footer
        if !opts.dry_run {
            mpr.footer_finish();
        }
        Ok(installed)
    }

    async fn install_some_versions(
        &mut self,
        config: &Arc<Config>,
        versions: Vec<ToolRequest>,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>, Error> {
        debug!("install_some_versions: {}", versions.iter().join(" "));

        // Group versions by backend
        let versions_clone = versions.clone();
        let queue: Result<Vec<_>> = versions
            .into_iter()
            .rev()
            .chunk_by(|v| v.ba().clone())
            .into_iter()
            .map(|(ba, v)| Ok((ba.backend()?, v.collect_vec())))
            .collect();

        let queue = match queue {
            Ok(q) => q,
            Err(e) => {
                // If we can't build the queue, return error for all versions
                let failed_installations: Vec<_> = versions_clone
                    .into_iter()
                    .map(|tr| (tr, eyre::eyre!("{}", e)))
                    .collect();
                return Err(Error::InstallFailed {
                    successful_installations: vec![],
                    failed_installations,
                });
            }
        };

        // Don't initialize header here - it's already done in install_all_versions

        // Track plugin installation errors to avoid early returns
        let mut plugin_errors = Vec::new();

        // Ensure plugins are installed
        for (backend, trs) in &queue {
            if let Some(plugin) = backend.plugin()
                && !plugin.is_installed()
            {
                let mpr = MultiProgressReport::get();
                if let Err(e) = plugin
                    .ensure_installed(config, &mpr, false, opts.dry_run)
                    .await
                    .or_else(|err| {
                        if let Some(&Error::PluginNotInstalled(_)) = err.downcast_ref::<Error>() {
                            Ok(())
                        } else {
                            Err(err)
                        }
                    })
                {
                    // Collect plugin installation errors instead of returning early
                    let plugin_name = backend.ba().short.clone();
                    for tr in trs {
                        plugin_errors.push((
                            tr.clone(),
                            eyre::eyre!("Plugin '{}' installation failed: {}", plugin_name, e),
                        ));
                    }
                }
            }
        }

        let raw = opts.raw || Settings::get().raw;
        let jobs = match raw {
            true => 1,
            false => opts.jobs.unwrap_or(Settings::get().jobs),
        };
        let semaphore = Arc::new(Semaphore::new(jobs));
        let ts = Arc::new(self.clone());
        let mut tset: JoinSet<Vec<(ToolRequest, Result<ToolVersion>)>> = JoinSet::new();
        let opts = Arc::new(opts.clone());

        // Track semaphore acquisition errors
        let mut semaphore_errors = Vec::new();

        // Track which tools are being processed by each task for better error reporting
        // Use a HashMap to map task IDs to their tools
        let mut task_tools: HashMap<usize, Vec<ToolRequest>> = HashMap::new();

        // Track which tools already have plugin errors to avoid duplicate reporting
        let mut tools_with_plugin_errors: HashSet<ToolRequest> = HashSet::new();
        for (tr, _) in &plugin_errors {
            tools_with_plugin_errors.insert(tr.clone());
        }

        for (ba, trs) in queue {
            let ts = ts.clone();
            let permit = match semaphore.clone().acquire_owned().await {
                Ok(p) => p,
                Err(e) => {
                    // Collect semaphore acquisition errors instead of returning early
                    for tr in trs {
                        semaphore_errors
                            .push((tr, eyre::eyre!("Failed to acquire semaphore: {}", e)));
                    }
                    continue;
                }
            };
            let opts = opts.clone();
            let ba = ba.clone();
            let config = config.clone();

            // Filter out tools that already have plugin errors
            let filtered_trs: Vec<ToolRequest> = trs
                .into_iter()
                .filter(|tr| !tools_with_plugin_errors.contains(tr))
                .collect();

            // Skip spawning task if no tools remain after filtering
            if filtered_trs.is_empty() {
                continue;
            }

            // Track the tools for this task using the task ID
            let task_id = tset.len();
            task_tools.insert(task_id, filtered_trs.clone());

            tset.spawn(async move {
                let _permit = permit;
                let mpr = MultiProgressReport::get();
                let mut results = vec![];

                for tr in filtered_trs {
                    let result = async {
                        let tv = tr.resolve(&config, &opts.resolve_options).await?;
                        let ctx = InstallContext {
                            config: config.clone(),
                            ts: ts.clone(),
                            pr: mpr.add_with_options(&tv.style(), opts.dry_run),
                            force: opts.force,
                            dry_run: opts.dry_run,
                            locked: opts.locked,
                        };
                        // Avoid wrapping the backend error here so the error location
                        // points to the backend implementation (more helpful for debugging).
                        ba.install_version(ctx, tv).await
                    }
                    .await;

                    results.push((tr, result));
                    // Bump footer for each completed tool
                    MultiProgressReport::get().footer_inc(1);
                }
                results
            });
        }

        let mut task_results = vec![];

        // Collect results from spawned tasks
        while let Some(res) = tset.join_next().await {
            match res {
                Ok(results) => task_results.extend(results),
                Err(e) => panic!("task join error: {e:#}"),
            }
        }

        // Reverse task results to maintain original order (since we reversed when building queue)
        task_results.reverse();

        let mut all_results = vec![];

        // Add plugin errors first (in original order)
        all_results.extend(plugin_errors.into_iter().map(|(tr, e)| (tr, Err(e))));

        // Add semaphore errors (in original order)
        all_results.extend(semaphore_errors.into_iter().map(|(tr, e)| (tr, Err(e))));

        // Add task results (already in correct order after reversal)
        all_results.extend(task_results);

        // Process results and separate successes from failures
        let mut successful_installations = vec![];
        let mut failed_installations = vec![];

        for (tr, result) in all_results {
            match result {
                Ok(tv) => successful_installations.push(tv),
                Err(e) => failed_installations.push((tr, e)),
            }
        }

        // Return appropriate result
        if failed_installations.is_empty() {
            Ok(successful_installations)
        } else {
            Err(Error::InstallFailed {
                successful_installations,
                failed_installations,
            })
        }
    }

    pub async fn list_missing_versions(&self, config: &Arc<Config>) -> Vec<ToolVersion> {
        trace!("list_missing_versions");
        measure!("toolset::list_missing_versions", {
            self.list_current_versions()
                .into_iter()
                .filter(|(p, tv)| !p.is_version_installed(config, tv, true))
                .map(|(_, tv)| tv)
                .collect()
        })
    }

    pub async fn list_installed_versions(&self, config: &Arc<Config>) -> Result<Vec<TVTuple>> {
        let current_versions: HashMap<(String, String), TVTuple> = self
            .list_current_versions()
            .into_iter()
            .map(|(p, tv)| ((p.id().into(), tv.version.clone()), (p.clone(), tv)))
            .collect();
        let current_versions = Arc::new(current_versions);
        let mut versions = vec![];
        for b in backend::list().into_iter() {
            for v in b.list_installed_versions() {
                if let Some((p, tv)) = current_versions.get(&(b.id().into(), v.clone())) {
                    versions.push((p.clone(), tv.clone()));
                } else {
                    let tv = ToolRequest::new(b.ba().clone(), &v, ToolSource::Unknown)?
                        .resolve(config, &Default::default())
                        .await?;
                    versions.push((b.clone(), tv));
                }
            }
        }
        Ok(versions)
    }

    pub fn list_current_requests(&self) -> Vec<&ToolRequest> {
        self.versions
            .values()
            .flat_map(|tvl| &tvl.requests)
            .collect()
    }

    pub fn list_versions_by_plugin(&self) -> Vec<(Arc<dyn Backend>, &Vec<ToolVersion>)> {
        self.versions
            .iter()
            .flat_map(|(ba, v)| eyre::Ok((ba.backend()?, &v.versions)))
            .collect()
    }

    pub fn list_current_versions(&self) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        trace!("list_current_versions");
        self.list_versions_by_plugin()
            .iter()
            .flat_map(|(p, v)| {
                v.iter().filter(|v| v.request.is_os_supported()).map(|v| {
                    // map cargo backend specific prefixes to ref
                    let tv = match v.version.split_once(':') {
                        Some((ref_type @ ("tag" | "branch" | "rev"), r)) => {
                            let request = ToolRequest::Ref {
                                backend: p.ba().clone(),
                                ref_: r.to_string(),
                                ref_type: ref_type.to_string(),
                                options: v.request.options().clone(),
                                source: v.request.source().clone(),
                            };
                            let version = format!("ref:{r}");
                            ToolVersion::new(request, version)
                        }
                        _ => v.clone(),
                    };
                    (p.clone(), tv)
                })
            })
            .collect()
    }

    pub async fn list_all_versions(
        &self,
        config: &Arc<Config>,
    ) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        use itertools::Itertools;
        let versions = self
            .list_current_versions()
            .into_iter()
            .chain(self.list_installed_versions(config).await?)
            .unique_by(|(ba, tv)| (ba.clone(), tv.tv_pathname().to_string()))
            .collect();
        Ok(versions)
    }

    pub fn list_current_installed_versions(
        &self,
        config: &Arc<Config>,
    ) -> Vec<(Arc<dyn Backend>, ToolVersion)> {
        self.list_current_versions()
            .into_iter()
            .filter(|(p, tv)| p.is_version_installed(config, tv, true))
            .collect()
    }

    pub async fn list_outdated_versions(
        &self,
        config: &Arc<Config>,
        bump: bool,
        opts: &ResolveOptions,
    ) -> Vec<OutdatedInfo> {
        self.list_outdated_versions_filtered(config, bump, opts, None)
            .await
    }

    pub async fn list_outdated_versions_filtered(
        &self,
        config: &Arc<Config>,
        bump: bool,
        opts: &ResolveOptions,
        filter_tools: Option<&[crate::cli::args::ToolArg]>,
    ) -> Vec<OutdatedInfo> {
        let versions = self
            .list_current_versions()
            .into_iter()
            // Filter to only check specified tools if provided
            .filter(|(_, tv)| {
                if let Some(tools) = filter_tools {
                    tools.iter().any(|t| t.ba.as_ref() == tv.ba())
                } else {
                    true
                }
            })
            .map(|(t, tv)| (config.clone(), t, tv, bump, opts.clone()))
            .collect::<Vec<_>>();
        let outdated = parallel::parallel(versions, |(config, t, tv, bump, opts)| async move {
            let mut outdated = vec![];
            match t.outdated_info(&config, &tv, bump, &opts).await {
                Ok(Some(oi)) => outdated.push(oi),
                Ok(None) => {}
                Err(e) => {
                    warn!("Error getting outdated info for {tv}: {e:#}");
                }
            }
            if t.symlink_path(&tv).is_some() {
                trace!("skipping symlinked version {tv}");
                // do not consider symlinked versions to be outdated
                return Ok(outdated);
            }
            match OutdatedInfo::resolve(&config, tv.clone(), bump, &opts).await {
                Ok(Some(oi)) => outdated.push(oi),
                Ok(None) => {}
                Err(e) => {
                    warn!("Error creating OutdatedInfo for {tv}: {e:#}");
                }
            }
            Ok(outdated)
        })
        .await
        .unwrap_or_else(|e| {
            warn!("Error in parallel outdated version check: {e:#}");
            vec![]
        });
        outdated.into_iter().flatten().collect()
    }

    pub async fn tera_ctx(&self, config: &Arc<Config>) -> Result<&tera::Context> {
        self.tera_ctx
            .get_or_try_init(async || {
                let env = self.full_env(config).await?;
                let mut ctx = config.tera_ctx.clone();
                ctx.insert("env", &env);
                Ok(ctx)
            })
            .await
    }

    pub async fn which(
        &self,
        config: &Arc<Config>,
        bin_name: &str,
    ) -> Option<(Arc<dyn Backend>, ToolVersion)> {
        for (p, tv) in self.list_current_installed_versions(config) {
            match Box::pin(p.which(config, &tv, bin_name)).await {
                Ok(Some(_bin)) => return Some((p, tv)),
                Ok(None) => {}
                Err(e) => {
                    debug!("Error running which: {:#}", e);
                }
            }
        }
        None
    }

    pub async fn which_bin(&self, config: &Arc<Config>, bin_name: &str) -> Option<PathBuf> {
        for (p, tv) in self.list_current_installed_versions(config) {
            if let Ok(Some(bin)) = Box::pin(p.which(config, &tv, bin_name)).await {
                return Some(bin);
            }
        }
        None
    }

    pub async fn list_rtvs_with_bin(
        &self,
        config: &Arc<Config>,
        bin_name: &str,
    ) -> Result<Vec<ToolVersion>> {
        let mut rtvs = vec![];
        for (p, tv) in self.list_installed_versions(config).await? {
            match p.which(config, &tv, bin_name).await {
                Ok(Some(_bin)) => rtvs.push(tv),
                Ok(None) => {}
                Err(e) => {
                    warn!("Error running which: {:#}", e);
                }
            }
        }
        Ok(rtvs)
    }

    pub async fn notify_if_versions_missing(&self, config: &Arc<Config>) {
        if Settings::get().status.missing_tools() == SettingsStatusMissingTools::Never {
            return;
        }
        let mut missing = vec![];
        let missing_versions = self.list_missing_versions(config).await;
        for tv in missing_versions.into_iter() {
            if Settings::get().status.missing_tools() == SettingsStatusMissingTools::Always {
                missing.push(tv);
                continue;
            }
            if let Ok(backend) = tv.backend() {
                let installed = backend.list_installed_versions();
                if !installed.is_empty() {
                    missing.push(tv);
                }
            }
        }
        if missing.is_empty() || *crate::env::__MISE_SHIM {
            return;
        }
        let versions = missing
            .iter()
            .map(|tv| tv.style())
            .collect::<Vec<_>>()
            .join(" ");
        warn!(
            "missing: {}",
            truncate_str(&versions, *TERM_WIDTH - 14, "…"),
        );
    }

    fn is_disabled(&self, ba: &BackendArg) -> bool {
        !ba.is_os_supported()
            || !tool_enabled(
                &Settings::get().enable_tools(),
                &Settings::get().disable_tools(),
                &ba.short.to_string(),
            )
    }
}

impl Display for Toolset {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let plugins = &self
            .versions
            .iter()
            .map(|(_, v)| v.requests.iter().map(|tvr| tvr.to_string()).join(" "))
            .collect_vec();
        write!(f, "{}", plugins.join(", "))
    }
}

impl From<ToolRequestSet> for Toolset {
    fn from(trs: ToolRequestSet) -> Self {
        let mut ts = Toolset::default();
        for (ba, versions, source) in trs.into_iter() {
            ts.source = Some(source.clone());
            let mut tvl = ToolVersionList::new(ba.clone(), source);
            for tr in versions {
                tvl.requests.push(tr);
            }
            ts.versions.insert(ba, tvl);
        }
        ts
    }
}
