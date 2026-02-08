use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eyre::Result;
use indexmap::IndexSet;
use itertools::Itertools;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

use crate::config::Config;
use crate::config::settings::Settings;
use crate::errors::Error;
use crate::hooks::{Hooks, InstalledToolInfo};
use crate::install_context::InstallContext;
use crate::plugins::PluginType;
use crate::toolset::Toolset;
use crate::toolset::helpers::show_python_install_hint;
use crate::toolset::install_options::InstallOptions;
use crate::toolset::tool_deps::ToolDeps;
use crate::toolset::tool_request::ToolRequest;
use crate::toolset::tool_source::ToolSource;
use crate::toolset::tool_version::ToolVersion;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{config, hooks};

impl Toolset {
    #[async_backtrace::framed]
    /// Installs missing tool versions and returns (installed_versions, still_missing_versions).
    /// This avoids callers needing to re-compute the missing versions list.
    pub async fn install_missing_versions(
        &mut self,
        config: &mut Arc<Config>,
        opts: &InstallOptions,
    ) -> Result<(Vec<ToolVersion>, Vec<ToolVersion>)> {
        let missing = self.list_missing_versions(config).await;

        // If auto-install is explicitly disabled, skip installation but return what's missing
        if opts.skip_auto_install {
            return Ok((vec![], missing));
        }

        let mut versions = missing
            .iter()
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
            .map(|tv| tv.request.clone())
            .collect_vec();
        // Ensure options from toolset are preserved during auto-install
        self.init_request_options(&mut versions);
        let installed = self.install_all_versions(config, versions, opts).await?;
        if !installed.is_empty() {
            let ts = config.get_toolset().await?;
            config::rebuild_shims_and_runtime_symlinks(config, ts, &installed).await?;
            // Re-check what's still missing after installation
            let still_missing = self.list_missing_versions(config).await;
            return Ok((installed, still_missing));
        }
        // Nothing was installed, the missing list is unchanged
        Ok((installed, missing))
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
                // Use config request options if available, falling back to backend arg opts.
                // This ensures tool options like postinstall from mise.toml are preserved
                // when installing with an explicit CLI version (e.g. `mise install tool@latest`).
                let options = tvl
                    .requests
                    .first()
                    .map(|r| r.options())
                    .filter(|opts| !opts.is_empty())
                    .unwrap_or_else(|| tvl.backend.opts());
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
        // Install all plugins from [plugins] config section first
        // This must happen before the empty check so plugins are installed
        // even when there are no tools to install (e.g., env-only plugins)
        Self::ensure_config_plugins_installed(config, opts.dry_run).await?;

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

        // Ensure plugins are installed before building dependency graph
        let plugin_errors = self.ensure_plugins_installed(config, &versions, opts).await;

        // Filter out tools with plugin errors
        let tools_with_plugin_errors: HashSet<_> =
            plugin_errors.iter().map(|(tr, _)| tr.clone()).collect();
        let versions_to_install: Vec<_> = versions
            .into_iter()
            .filter(|tr| !tools_with_plugin_errors.contains(tr))
            .collect();

        // Build dependency graph and install using Kahn's algorithm
        let (installed, failed) = self
            .install_with_deps(config, versions_to_install, opts)
            .await;

        // Update footer for plugin errors
        let plugin_error_count = plugin_errors.len();
        if plugin_error_count > 0 {
            mpr.footer_inc(plugin_error_count);
        }

        // Combine plugin errors with installation failures
        let mut all_failed = plugin_errors;
        all_failed.extend(failed);

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
            // Run post-install hook with installed tools info
            // Use the full resolved toolset so all installed tools are on PATH
            // Fall back to self if toolset resolution fails (e.g. due to config issues)
            let installed_tools: Vec<InstalledToolInfo> =
                installed.iter().map(InstalledToolInfo::from).collect();
            let ts = match config.get_toolset().await {
                Ok(ts) => ts,
                Err(e) => {
                    debug!("error resolving toolset for postinstall hook: {e:#}");
                    self
                }
            };
            hooks::run_one_hook_with_context(
                config,
                ts,
                Hooks::Postinstall,
                None,
                Some(&installed_tools),
            )
            .await;
        }

        // Finish the global footer
        if !opts.dry_run {
            mpr.footer_finish();
        }

        // Return appropriate result
        if all_failed.is_empty() {
            Ok(installed)
        } else {
            Err(Error::InstallFailed {
                successful_installations: installed,
                failed_installations: all_failed,
            }
            .into())
        }
    }

    /// Ensure all plugins for the requested tools are installed
    async fn ensure_plugins_installed(
        &self,
        config: &Arc<Config>,
        versions: &[ToolRequest],
        opts: &InstallOptions,
    ) -> Vec<(ToolRequest, eyre::Error)> {
        let mut plugin_errors = Vec::new();
        let mut checked_backends = HashSet::new();

        for tr in versions {
            let ba = tr.ba();
            if checked_backends.contains(ba) {
                continue;
            }
            checked_backends.insert(ba.clone());

            if let Ok(backend) = tr.backend()
                && let Some(plugin) = backend.plugin()
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
                    // Collect errors for all tools using this plugin
                    for tr2 in versions {
                        if tr2.ba() == ba {
                            plugin_errors.push((
                                tr2.clone(),
                                eyre::eyre!("Plugin '{}' installation failed: {}", ba.short, e),
                            ));
                        }
                    }
                }
            }
        }

        plugin_errors
    }

    /// Install tools using Kahn's algorithm for dependency ordering.
    /// Returns (successful_installations, failed_installations).
    async fn install_with_deps(
        &self,
        config: &Arc<Config>,
        versions: Vec<ToolRequest>,
        opts: &InstallOptions,
    ) -> (Vec<ToolVersion>, Vec<(ToolRequest, eyre::Error)>) {
        if versions.is_empty() {
            return (vec![], vec![]);
        }

        // Build index map to preserve original request order
        let request_order: HashMap<String, usize> = versions
            .iter()
            .enumerate()
            .map(|(i, tr)| (format!("{}@{}", tr.ba().full(), tr.version()), i))
            .collect();

        // Build dependency graph
        let tool_deps = match ToolDeps::new(versions.clone()) {
            Ok(deps) => Arc::new(Mutex::new(deps)),
            Err(e) => {
                // If we can't build the graph, return error for all versions
                let failed: Vec<_> = versions
                    .into_iter()
                    .map(|tr| (tr, eyre::eyre!("Failed to build dependency graph: {}", e)))
                    .collect();
                return (vec![], failed);
            }
        };

        let mut rx = tool_deps.lock().await.subscribe();

        let raw = opts.raw || Settings::get().raw;
        let jobs = match raw {
            true => 1,
            false => opts.jobs.unwrap_or(Settings::get().jobs),
        };
        let semaphore = Arc::new(Semaphore::new(jobs));
        let ts = Arc::new(self.clone());
        let opts = Arc::new(opts.clone());

        let mut installed = vec![];
        let mut failed = vec![];
        let mut jset: JoinSet<(ToolRequest, Result<ToolVersion>)> = JoinSet::new();
        // Track in-flight tools to recover from task panics
        let mut in_flight: HashMap<tokio::task::Id, ToolRequest> = HashMap::new();

        loop {
            tokio::select! {
                // Use `biased` to ensure completed installations are handled before starting new ones.
                // This priority ordering ensures dependency tracking stays correct: we must process
                // completions (which may unblock dependents) before spawning new installations.
                biased;

                // Handle completed installations first (higher priority)
                Some(result) = jset.join_next() => {
                    let mpr = MultiProgressReport::get();
                    match result {
                        Ok((tr, Ok(tv))) => {
                            mpr.footer_inc(1);
                            installed.push(tv);
                            tool_deps.lock().await.complete_success(&tr);
                        }
                        Ok((tr, Err(e))) => {
                            mpr.footer_inc(1);
                            failed.push((tr.clone(), e));
                            tool_deps.lock().await.complete_failure(&tr);
                        }
                        Err(e) => {
                            // Task panicked - try to recover the tool request from in_flight tracking
                            mpr.footer_inc(1);
                            if let Some(tr) = in_flight.remove(&e.id()) {
                                failed.push((tr.clone(), eyre::eyre!("Installation task panicked: {e:#}")));
                                tool_deps.lock().await.complete_failure(&tr);
                            } else {
                                warn!("Task panicked but tool request not found: {e:#}");
                            }
                        }
                    }
                }

                // Receive new tools to install
                Some(maybe_tr) = rx.recv() => {
                    match maybe_tr {
                        Some(tr) => {
                            // Spawn installation task
                            let permit = match semaphore.clone().acquire_owned().await {
                                Ok(p) => p,
                                Err(e) => {
                                    // Mark as failed and notify tool_deps so dependents are blocked
                                    MultiProgressReport::get().footer_inc(1);
                                    failed.push((tr.clone(), eyre::eyre!("Failed to acquire semaphore: {}", e)));
                                    tool_deps.lock().await.complete_failure(&tr);
                                    continue;
                                }
                            };

                            let config = config.clone();
                            let ts = ts.clone();
                            let opts = opts.clone();
                            let tr_clone = tr.clone();

                            let handle = jset.spawn(async move {
                                let _permit = permit;
                                let result = Self::install_single_tool(&config, &ts, &tr, &opts).await;
                                (tr, result)
                            });
                            in_flight.insert(handle.id(), tr_clone);
                        }
                        None => {
                            // All tools have been emitted, wait for remaining tasks
                            break;
                        }
                    }
                }

                else => break,
            }
        }

        // Wait for all remaining tasks to complete
        while let Some(result) = jset.join_next().await {
            let mpr = MultiProgressReport::get();
            match result {
                Ok((tr, Ok(tv))) => {
                    mpr.footer_inc(1);
                    installed.push(tv);
                    tool_deps.lock().await.complete_success(&tr);
                }
                Ok((tr, Err(e))) => {
                    mpr.footer_inc(1);
                    failed.push((tr.clone(), e));
                    tool_deps.lock().await.complete_failure(&tr);
                }
                Err(e) => {
                    mpr.footer_inc(1);
                    if let Some(tr) = in_flight.remove(&e.id()) {
                        failed.push((tr.clone(), eyre::eyre!("Installation task panicked: {e:#}")));
                        tool_deps.lock().await.complete_failure(&tr);
                    } else {
                        warn!("Task panicked but tool request not found: {e:#}");
                    }
                }
            }
        }

        // Add blocked tools to failures
        let blocked = tool_deps.lock().await.blocked_tools();
        for tr in blocked {
            failed.push((tr.clone(), eyre::eyre!("Skipped due to failed dependency")));
            MultiProgressReport::get().footer_inc(1);
        }

        // Sort installed versions by original request order to preserve user's intended ordering
        installed.sort_by_key(|tv| {
            let key = format!("{}@{}", tv.ba().full(), tv.request.version());
            request_order.get(&key).copied().unwrap_or(usize::MAX)
        });

        (installed, failed)
    }

    /// Install a single tool
    async fn install_single_tool(
        config: &Arc<Config>,
        ts: &Arc<Toolset>,
        tr: &ToolRequest,
        opts: &Arc<InstallOptions>,
    ) -> Result<ToolVersion> {
        let mpr = MultiProgressReport::get();

        let tv = tr.resolve(config, &opts.resolve_options).await?;
        let backend = tr.backend()?;

        let ctx = InstallContext {
            config: config.clone(),
            ts: ts.clone(),
            pr: mpr.add_with_options(&tv.style(), opts.dry_run),
            force: opts.force,
            dry_run: opts.dry_run,
            locked: opts.locked,
        };

        backend.install_version(ctx, tv).await
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

    /// Install all plugins defined in [plugins] config section
    pub async fn ensure_config_plugins_installed(
        config: &Arc<Config>,
        dry_run: bool,
    ) -> Result<()> {
        if config.repo_urls.is_empty() {
            return Ok(());
        }

        let mpr = MultiProgressReport::get();

        for (plugin_key, url) in &config.repo_urls {
            let (plugin_type, name) = Self::parse_plugin_key(plugin_key, url);

            // Skip empty plugin names (e.g., from malformed keys like "" or "vfox:")
            if name.is_empty() {
                warn!("skipping empty plugin name from key: {plugin_key}");
                continue;
            }

            let plugin = plugin_type.plugin(name.to_string());

            if !plugin.is_installed() {
                plugin
                    .ensure_installed(config, &mpr, false, dry_run)
                    .await?;
            }
        }
        Ok(())
    }

    fn parse_plugin_key<'a>(key: &'a str, url: &str) -> (PluginType, &'a str) {
        if let Some(name) = key.strip_prefix("vfox:") {
            (PluginType::Vfox, name)
        } else if let Some(name) = key.strip_prefix("vfox-backend:") {
            (PluginType::VfoxBackend, name)
        } else if let Some(name) = key.strip_prefix("asdf:") {
            (PluginType::Asdf, name)
        } else if url.contains("vfox-") {
            // Match existing behavior from config/mod.rs:226-228
            (PluginType::Vfox, key)
        } else {
            (PluginType::Asdf, key)
        }
    }
}
