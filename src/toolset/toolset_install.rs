use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use tokio::{sync::Semaphore, task::JoinSet};

use crate::config::Config;
use crate::config::settings::Settings;
use crate::errors::Error;
use crate::hooks::{HookToolContext, Hooks};
use crate::install_context::InstallContext;
use crate::toolset::Toolset;
use crate::toolset::helpers::{get_leaf_dependencies, show_python_install_hint};
use crate::toolset::install_options::InstallOptions;
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::tool_source::ToolSource;
use crate::toolset::tool_version::ToolVersion;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{config, hooks};

impl Toolset {
    #[async_backtrace::framed]
    pub async fn install_missing_versions(
        &mut self,
        config: &mut Arc<Config>,
        opts: &InstallOptions,
    ) -> Result<Vec<ToolVersion>> {
        // If auto-install is explicitly disabled, skip all automatic installation
        if opts.skip_auto_install {
            return Ok(vec![]);
        }

        let mut versions = self
            .list_missing_versions(config)
            .await
            .into_iter()
            .filter(|tv| {
                !opts.missing_args_only
                    || matches!(self.versions[tv.ba()].source, ToolSource::Argument)
            })
            .filter(|tv| {
                if let Some(tools) = &opts.auto_install_disable_tools {
                    !tools.contains(&tv.ba().short)
                } else {
                    true
                }
            })
            .map(|tv| tv.request)
            .collect_vec();
        // Ensure options from toolset are preserved during auto-install
        self.init_request_options(&mut versions);
        let versions = self.install_all_versions(config, versions, opts).await?;
        if !versions.is_empty() {
            let ts = config.get_toolset().await?;
            config::rebuild_shims_and_runtime_symlinks(config, ts, &versions).await?;
        }
        Ok(versions)
    }

    /// sets the options on incoming requests to install to whatever is already in the toolset
    /// this handles the use-case where you run `mise use ubi:cilium/cilium-cli` (without CLi options)
    /// but this tool has options inside mise.toml
    pub(super) fn init_request_options(&self, requests: &mut Vec<ToolRequest>) {
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

        // Finish the global footer
        if !opts.dry_run {
            mpr.footer_finish();
        }
        Ok(installed)
    }

    pub(super) async fn install_some_versions(
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

                        // Run per-tool preinstall hook
                        if !opts.dry_run {
                            let tool_ctx = HookToolContext {
                                name: tv.ba().short.clone(),
                                version: tv.version.clone(),
                            };
                            hooks::run_one_hook_with_tool(
                                &config,
                                &ts,
                                Hooks::Preinstall,
                                &tool_ctx,
                            )
                            .await;
                        }

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
                        let result = ba.install_version(ctx, tv).await;

                        // Run per-tool postinstall hook (only on success)
                        if !opts.dry_run
                            && let Ok(ref installed_tv) = result
                        {
                            let tool_ctx = HookToolContext {
                                name: installed_tv.ba().short.clone(),
                                version: installed_tv.version.clone(),
                            };
                            hooks::run_one_hook_with_tool(
                                &config,
                                &ts,
                                Hooks::Postinstall,
                                &tool_ctx,
                            )
                            .await;
                        }

                        result
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

    pub async fn install_missing_bin(
        &mut self,
        config: &mut Arc<Config>,
        bin_name: &str,
    ) -> Result<Option<Vec<ToolVersion>>> {
        // Strategy: Find backends that could provide this bin by checking:
        // 1. Any currently installed versions that provide the bin
        // 2. Any requested backends with installed versions (even if not current)
        let mut plugins = IndexSet::new();

        // First check currently active installed versions
        for (p, tv) in self.list_current_installed_versions(config) {
            if let Ok(Some(_bin)) = p.which(config, &tv, bin_name).await {
                plugins.insert(p);
            }
        }

        // Also check backends that are requested but not currently active
        // This handles the case where a user has tool@v1 globally and tool@v2 locally (not installed)
        // When looking for a bin provided by the tool, we check if any installed version provides it
        let all_installed = self.list_installed_versions(config).await?;
        for (backend, _versions) in self.list_versions_by_plugin() {
            // Skip if we already found this backend
            if plugins.contains(&backend) {
                continue;
            }

            // Check if this backend has ANY installed version that provides the bin
            let backend_versions: Vec<_> = all_installed
                .iter()
                .filter(|(p, _)| p.ba() == backend.ba())
                .collect();

            for (_, tv) in backend_versions {
                if let Ok(Some(_bin)) = backend.which(config, tv, bin_name).await {
                    plugins.insert(backend.clone());
                    break;
                }
            }
        }

        // Install missing versions for backends that provide this bin
        for plugin in plugins {
            let versions = self
                .list_missing_versions(config)
                .await
                .into_iter()
                .filter(|tv| tv.ba() == &**plugin.ba())
                .filter(|tv| match &Settings::get().auto_install_disable_tools {
                    Some(disable_tools) => !disable_tools.contains(&tv.ba().short),
                    None => true,
                })
                .map(|tv| tv.request)
                .collect_vec();
            if !versions.is_empty() {
                let versions = self
                    .install_all_versions(config, versions.clone(), &InstallOptions::default())
                    .await?;
                if !versions.is_empty() {
                    let ts = config.get_toolset().await?;
                    config::rebuild_shims_and_runtime_symlinks(config, ts, &versions).await?;
                }
                return Ok(Some(versions));
            }
        }
        Ok(None)
    }
}
