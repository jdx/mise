use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::duration::parse_into_timestamp;
use crate::file::display_path;
use crate::lockfile::{self, LockResolutionResult, Lockfile};
use crate::platform::Platform;
use crate::toolset::{ResolveOptions, ToolRequest, ToolSource, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{cli::args::ToolArg, config::Settings};
use console::style;
use eyre::{Result, bail};
use jiff::Timestamp;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// A tool to lock for a specific lockfile target.
type LockTool = (crate::cli::args::BackendArg, crate::toolset::ToolVersion);

/// Update lockfile checksums and URLs for all specified platforms
///
/// Updates checksums and download URLs for all platforms already specified in the lockfile.
/// If no lockfile exists, shows what would be created based on the current configuration.
/// This allows you to refresh lockfile data for platforms other than the one you're currently on.
/// Operates on the lockfile in the current config root. Use TOOL arguments to target specific tools.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Lock {
    /// Tool(s) to update in lockfile
    /// e.g.: node python
    /// If not specified, all tools in lockfile will be updated
    #[clap(value_name = "TOOL", verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,

    /// Target only global config lockfiles (~/.config/mise/mise.lock and system config)
    /// By default, only the active project config root is locked
    #[clap(long, short, verbatim_doc_comment)]
    pub global: bool,

    /// Number of jobs to run in parallel
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    pub jobs: Option<usize>,

    /// Show what would be updated without making changes
    #[clap(long, short = 'n', verbatim_doc_comment)]
    pub dry_run: bool,

    /// Comma-separated list of platforms to target
    /// e.g.: linux-x64,macos-arm64,windows-x64
    /// If not specified, all platforms already in lockfile will be updated
    #[clap(long, short, value_delimiter = ',', verbatim_doc_comment)]
    pub platform: Vec<String>,

    /// Update mise.local.lock instead of mise.lock
    /// Use for tools defined in .local.toml configs
    #[clap(long, verbatim_doc_comment)]
    pub local: bool,

    /// Only lock versions released before this age or date
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    /// This only affects fuzzy version matches like "20" or "latest".
    /// Explicitly pinned versions like "22.5.0" are not filtered.
    /// Existing matching lockfile entries are preserved and are not downgraded solely by this flag.
    #[clap(
        long,
        alias = "before",
        value_name = "MINIMUM_RELEASE_AGE",
        verbatim_doc_comment
    )]
    pub minimum_release_age: Option<String>,
}

impl Lock {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::get();
        if settings.locked {
            bail!(
                "mise lock is disabled in --locked mode\nhint: Remove --locked or unset MISE_LOCKED=1"
            );
        }
        let config = Config::get().await?;
        let before_date = self.get_before_date()?;
        let lock_resolve_options = ResolveOptions {
            before_date,
            ..Default::default()
        };

        let ts_owned;
        let ts = if before_date.is_some() {
            ts_owned = ToolsetBuilder::new()
                .with_resolve_options(lock_resolve_options.clone())
                .build(&config)
                .await?;
            &ts_owned
        } else {
            config.get_toolset().await?
        };

        let scoped_config_paths = self.config_paths_in_lock_scope(&config);
        let lockfile_targets = self.get_lockfile_targets(&config, &scoped_config_paths);
        let mut has_lock_targets = false;
        let mut all_provenance_errors: Vec<String> = Vec::new();

        for (lockfile_path, config_paths) in &lockfile_targets {
            let tools = self
                .get_tools_to_lock(
                    &config,
                    ts,
                    lockfile_path,
                    config_paths,
                    &lock_resolve_options,
                )
                .await;

            if tools.is_empty() {
                // `tools` can be empty either because config has no tools, or because a filter excludes all.
                // For unfiltered runs (`mise lock`), this means "prune all stale lockfile entries".
                let mut lockfile = Lockfile::read(lockfile_path)?;
                if self.dry_run {
                    let stale_tools = self.stale_entries_if_pruned(&lockfile, &tools);
                    self.show_stale_prune_message(lockfile_path, &stale_tools, true)?;
                    if !stale_tools.is_empty() {
                        has_lock_targets = true;
                    }
                } else {
                    let pruned_tools = self.prune_stale_entries_if_needed(&mut lockfile, &tools);
                    if !pruned_tools.is_empty() {
                        lockfile.write(lockfile_path)?;
                        self.show_stale_prune_message(lockfile_path, &pruned_tools, false)?;
                        has_lock_targets = true;
                    }
                }
                continue;
            }
            has_lock_targets = true;

            let target_platforms = self.determine_target_platforms(lockfile_path)?;

            miseprintln!(
                "{} Targeting {} platform(s) for {}: {}",
                style("→").cyan(),
                target_platforms.len(),
                style(display_path(lockfile_path)).cyan(),
                target_platforms
                    .iter()
                    .map(|p| p.to_key())
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            miseprintln!(
                "{} Processing {} tool(s): {}",
                style("→").cyan(),
                tools.len(),
                tools
                    .iter()
                    .map(|(ba, tv)| format!("{}@{}", ba.short, tv.version))
                    .collect::<Vec<_>>()
                    .join(", ")
            );

            if self.dry_run {
                self.show_dry_run(&tools, &target_platforms)?;
                let lockfile = Lockfile::read(lockfile_path)?;
                if self.is_unfiltered_lock_run() {
                    let stale_tools = self.stale_entries_if_pruned(&lockfile, &tools);
                    self.show_stale_prune_message(lockfile_path, &stale_tools, true)?;
                }
                let stale_versions = self.stale_versions_if_pruned(&lockfile, &tools);
                self.show_stale_version_prune_message(lockfile_path, &stale_versions, true)?;
                continue;
            }

            // Process tools and update lockfile
            let mut lockfile = Lockfile::read(lockfile_path)?;
            let stale_tools = self.prune_stale_entries_if_needed(&mut lockfile, &tools);
            self.show_stale_prune_message(lockfile_path, &stale_tools, false)?;

            // Compute stale versions BEFORE process_tools so provenance checks can
            // compare against old version entries. Actual pruning happens after.
            let stale_versions = self.stale_versions_if_pruned(&lockfile, &tools);

            let (results, provenance_errors) = self
                .process_tools(&settings, &tools, &target_platforms, &mut lockfile)
                .await?;

            // Prune stale versions AFTER provenance checks complete
            self.prune_stale_versions(&mut lockfile, &tools);
            self.show_stale_version_prune_message(lockfile_path, &stale_versions, false)?;

            // Save lockfile before raising provenance errors so non-regressing
            // tools' entries are preserved
            lockfile.write(lockfile_path)?;

            // Print summary
            let successful = results.iter().filter(|(_, _, ok)| *ok).count();
            let skipped = results.len() - successful;
            miseprintln!(
                "{} Updated {} platform entries ({} skipped)",
                style("✓").green(),
                successful,
                skipped
            );
            miseprintln!(
                "{} Lockfile written to {}",
                style("✓").green(),
                style(display_path(lockfile_path)).cyan()
            );

            all_provenance_errors.extend(provenance_errors);
        }

        if !has_lock_targets {
            miseprintln!("{} No tools configured to lock", style("!").yellow());
        }

        // Update config files when a specific version is requested that doesn't match
        // the current prefix (e.g., `mise lock tiny@3.0.1` when config has `tiny = "2"`)
        {
            use crate::toolset::outdated_info::{
                apply_config_bumps, compute_config_bumps_for_paths,
            };
            let tool_versions: Vec<(String, String)> = self
                .tool
                .iter()
                .filter_map(|t| {
                    t.tvr
                        .as_ref()
                        .map(|tvr| (t.ba.short.clone(), tvr.version()))
                })
                .collect();
            let refs: Vec<(&str, &str)> = tool_versions
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect();
            let bumps = compute_config_bumps_for_paths(&config, &refs, &scoped_config_paths);
            if self.dry_run {
                for bump in &bumps {
                    miseprintln!(
                        "Would update {} from {} to {} in {}",
                        bump.tool_name,
                        bump.old_version,
                        bump.new_version,
                        display_path(&bump.config_path)
                    );
                }
            } else {
                apply_config_bumps(&config, &bumps)?;
            }
        }

        if !all_provenance_errors.is_empty() {
            return Err(eyre::eyre!("{}", all_provenance_errors.join("\n")));
        }

        Ok(())
    }

    /// Get the before_date from the CLI --minimum-release-age flag only.
    /// Per-tool and global setting fallbacks are handled during tool request resolution.
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        if let Some(minimum_release_age) = &self.minimum_release_age {
            return Ok(Some(parse_into_timestamp(minimum_release_age)?));
        }
        Ok(None)
    }

    fn is_unfiltered_lock_run(&self) -> bool {
        self.tool.is_empty()
    }

    fn prune_stale_entries_if_needed(
        &self,
        lockfile: &mut Lockfile,
        tools: &[(crate::cli::args::BackendArg, crate::toolset::ToolVersion)],
    ) -> BTreeSet<String> {
        if !self.is_unfiltered_lock_run() {
            return BTreeSet::new();
        }
        let (configured_tools, configured_backends) = self.configured_tool_selectors(tools);
        let stale_tools =
            self.stale_entries_for_selectors(lockfile, &configured_tools, &configured_backends);
        if !stale_tools.is_empty() {
            lockfile.retain_tools_by_short_or_backend(&configured_tools, &configured_backends);
        }
        stale_tools
    }

    /// Prune lockfile entries whose version no longer matches any resolved version
    /// of the tool. This prevents stale version entries from accumulating when a
    /// tool's resolved version changes.
    ///
    /// Note: This must be called AFTER process_tools() so that provenance checks
    /// can compare against the old version entries before they are removed.
    fn prune_stale_versions(&self, lockfile: &mut Lockfile, tools: &[LockTool]) {
        let current_versions = self.current_tool_versions(tools);
        for (short, versions) in &current_versions {
            lockfile.retain_tool_versions(short, versions);
        }
    }

    fn stale_entries_if_pruned(
        &self,
        lockfile: &Lockfile,
        tools: &[(crate::cli::args::BackendArg, crate::toolset::ToolVersion)],
    ) -> BTreeSet<String> {
        if !self.is_unfiltered_lock_run() {
            return BTreeSet::new();
        }
        let (configured_tools, configured_backends) = self.configured_tool_selectors(tools);
        self.stale_entries_for_selectors(lockfile, &configured_tools, &configured_backends)
    }

    fn stale_versions_if_pruned(
        &self,
        lockfile: &Lockfile,
        tools: &[LockTool],
    ) -> BTreeMap<String, Vec<String>> {
        let current_versions = self.current_tool_versions(tools);
        self.stale_versions_for_current(lockfile, &current_versions)
    }

    fn stale_versions_for_current(
        &self,
        lockfile: &Lockfile,
        current_versions: &BTreeMap<String, BTreeSet<String>>,
    ) -> BTreeMap<String, Vec<String>> {
        let mut stale: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (short, versions) in current_versions {
            let stale_versions = lockfile.stale_tool_versions(short, versions);
            if !stale_versions.is_empty() {
                stale.insert(short.clone(), stale_versions);
            }
        }
        stale
    }

    fn show_stale_version_prune_message(
        &self,
        lockfile_path: &Path,
        stale_versions: &BTreeMap<String, Vec<String>>,
        dry_run: bool,
    ) -> Result<()> {
        if stale_versions.is_empty() {
            return Ok(());
        }
        let total: usize = stale_versions.values().map(|v| v.len()).sum();
        let entry_word = if total == 1 { "entry" } else { "entries" };
        let (icon, message) = if dry_run {
            (style("→").yellow(), "Dry run - would prune")
        } else {
            (style("✓").green(), "Pruned")
        };
        let details: Vec<String> = stale_versions
            .iter()
            .flat_map(|(short, versions)| versions.iter().map(move |v| format!("{short}@{v}")))
            .collect();
        miseprintln!(
            "{} {} {} stale version {} from {}: {}",
            icon,
            message,
            total,
            entry_word,
            style(display_path(lockfile_path)).cyan(),
            details.join(", ")
        );
        Ok(())
    }

    fn configured_tool_selectors(
        &self,
        tools: &[(crate::cli::args::BackendArg, crate::toolset::ToolVersion)],
    ) -> (BTreeSet<String>, BTreeSet<String>) {
        let configured_tools: BTreeSet<String> =
            tools.iter().map(|(ba, _)| ba.short.clone()).collect();
        let configured_backends: BTreeSet<String> = tools.iter().map(|(ba, _)| ba.full()).collect();
        (configured_tools, configured_backends)
    }

    fn current_tool_versions(&self, tools: &[LockTool]) -> BTreeMap<String, BTreeSet<String>> {
        let mut current_versions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (ba, tv) in tools {
            current_versions
                .entry(ba.short.clone())
                .or_default()
                .insert(tv.version.clone());
        }
        current_versions
    }

    fn stale_entries_for_selectors(
        &self,
        lockfile: &Lockfile,
        configured_tools: &BTreeSet<String>,
        configured_backends: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        lockfile.stale_tool_shorts(configured_tools, configured_backends)
    }

    fn show_stale_prune_message(
        &self,
        lockfile_path: &Path,
        stale_tools: &BTreeSet<String>,
        dry_run: bool,
    ) -> Result<()> {
        if stale_tools.is_empty() {
            return Ok(());
        }
        let entry_word = if stale_tools.len() == 1 {
            "entry"
        } else {
            "entries"
        };
        let (icon, message) = if dry_run {
            (style("→").yellow(), "Dry run - would prune")
        } else {
            (style("✓").green(), "Pruned")
        };
        miseprintln!(
            "{} {} {} stale tool {} from {}: {}",
            icon,
            message,
            stale_tools.len(),
            entry_word,
            style(display_path(lockfile_path)).cyan(),
            stale_tools.iter().cloned().collect::<Vec<_>>().join(", ")
        );
        Ok(())
    }

    fn config_paths_in_lock_scope(&self, config: &Config) -> BTreeSet<PathBuf> {
        if self.global {
            return config
                .config_files
                .keys()
                .filter(|path| crate::config::is_global_config(path))
                .cloned()
                .collect();
        }
        let target_root = Self::target_lock_scope_root(config);

        config
            .config_files
            .iter()
            .filter_map(|(path, cf)| {
                if crate::config::is_global_config(path) {
                    return None;
                }
                let target_root = target_root.as_ref()?;
                (cf.project_root()
                    .unwrap_or_else(|| cf.config_root())
                    .as_path()
                    == target_root)
                    .then(|| path.clone())
            })
            .collect()
    }

    fn target_lock_scope_root(config: &Config) -> Option<PathBuf> {
        config.project_root.clone().or_else(|| {
            config
                .config_files
                .iter()
                .find(|(path, cf)| {
                    cf.source().is_mise_toml() && !crate::config::is_global_config(path)
                })
                .map(|(_, cf)| cf.config_root())
        })
    }

    /// Collect distinct lockfile targets from config files.
    /// Returns an ordered map of lockfile_path -> list of config paths that contribute to it.
    fn get_lockfile_targets(
        &self,
        config: &Config,
        scoped_config_paths: &BTreeSet<PathBuf>,
    ) -> indexmap::IndexMap<PathBuf, Vec<PathBuf>> {
        let mut targets: indexmap::IndexMap<PathBuf, Vec<PathBuf>> = indexmap::IndexMap::new();
        for (path, cf) in config.config_files.iter() {
            if !scoped_config_paths.contains(path) {
                continue;
            }
            if !cf.source().is_mise_toml() {
                continue;
            }
            let (lockfile_path, is_local) = lockfile::lockfile_path_for_config(path);
            if self.local && !is_local {
                continue;
            }
            targets.entry(lockfile_path).or_default().push(path.clone());
        }
        targets
    }

    fn determine_target_platforms(&self, lockfile_path: &Path) -> Result<Vec<Platform>> {
        if !self.platform.is_empty() {
            // User specified platforms explicitly
            return Platform::parse_multiple(&self.platform);
        }

        lockfile::determine_existing_platforms(lockfile_path)
    }

    /// Collect tools that belong to a given lockfile target.
    /// Only includes tools whose source config maps to the target lockfile path.
    async fn get_tools_to_lock(
        &self,
        config: &Arc<Config>,
        ts: &Toolset,
        target_lockfile_path: &Path,
        config_paths: &[PathBuf],
        base_resolve_options: &ResolveOptions,
    ) -> Vec<LockTool> {
        let config_paths_set: BTreeSet<&PathBuf> = config_paths.iter().collect();

        let mut all_tools: Vec<LockTool> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

        // First pass: tools from the resolved toolset whose source maps to this lockfile
        for (backend, tv) in ts.list_current_versions() {
            if let Some(source_path) = tv.request.source().path() {
                let (source_lockfile, _) = lockfile::lockfile_path_for_config(source_path);
                if source_lockfile != target_lockfile_path {
                    continue;
                }
            } else {
                // Tools without a source path (env vars, CLI args) go to mise.lock only
                let is_base_lockfile = target_lockfile_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n == "mise.lock");
                if !is_base_lockfile {
                    continue;
                }
            }
            // Skip unresolved symbolic versions (e.g., a lockfile poisoned with "latest"
            // as the version). Pass 2's fallback will resolve these to a concrete version.
            if tv.version == "latest" {
                continue;
            }
            let key = (backend.ba().short.clone(), tv.version.clone());
            if seen.insert(key) {
                all_tools.push((backend.ba().as_ref().clone(), tv));
            }
        }

        // Second pass: iterate config files matching this lockfile to catch
        // tools that were overridden by a higher-priority config
        for (path, cf) in config.config_files.iter() {
            if !config_paths_set.contains(path) {
                continue;
            }
            if let Ok(trs) = cf.to_tool_request_set() {
                for (ba, requests, _source) in trs.iter() {
                    for request in requests {
                        if ba.backend().is_ok() {
                            // Check if the resolved toolset has a matching request.
                            let mut matched_resolved = false;
                            if let Some(resolved_tv) = ts.versions.get(ba.as_ref()) {
                                for tv in &resolved_tv.versions {
                                    if tv.request.version() == request.version()
                                        && tv.request.options() == request.options()
                                        && tv.version != "latest"
                                    {
                                        matched_resolved = true;
                                        let key = (ba.short.clone(), tv.version.clone());
                                        if seen.insert(key) {
                                            all_tools.push((ba.as_ref().clone(), tv.clone()));
                                        }
                                    }
                                }
                            }
                            // Resolve overridden `latest` requests through the same path as
                            // active tools. When an install-before cutoff is active, bypass
                            // installed-version selection so the fallback still uses release
                            // dates from the remote version metadata.
                            if !matched_resolved && request.version() == "latest" {
                                let mut resolve_options = match request
                                    .resolve_options(base_resolve_options)
                                {
                                    Ok(opts) => opts,
                                    Err(err) => {
                                        debug!("failed to resolve options for {request}: {err}");
                                        continue;
                                    }
                                };
                                resolve_options.use_locked_version = false;
                                if resolve_options.before_date.is_some() {
                                    resolve_options.latest_versions = true;
                                }
                                match request.resolve(config, &resolve_options).await {
                                    Ok(tv) => {
                                        let key = (ba.short.clone(), tv.version.clone());
                                        if seen.insert(key) {
                                            all_tools.push((ba.as_ref().clone(), tv));
                                        }
                                    }
                                    Err(err) => {
                                        debug!("failed to resolve overridden {request}: {err}");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if self.tool.is_empty() {
            all_tools
        } else {
            // Build map of tool args with explicit versions
            let specified_versions: std::collections::HashMap<String, Option<ToolRequest>> = self
                .tool
                .iter()
                .map(|t| (t.ba.short.clone(), t.tvr.clone()))
                .collect();
            // For `tool@latest`, we want upgrade semantics: resolve "latest" to an
            // installed concrete version and lock that. Writing the literal "latest"
            // string to the lockfile would be a bug. Use the backend's own resolver so
            // we don't impose a semver ordering on tools that don't follow semver.
            let mut tools: Vec<LockTool> = Vec::new();
            for (ba, mut tv) in all_tools
                .into_iter()
                .filter(|(ba, _)| specified_versions.contains_key(&ba.short))
            {
                if let Some(Some(request)) = specified_versions.get(&ba.short) {
                    let version = request.version();
                    let request = ToolRequest::new_opts(
                        Arc::new(ba.clone()),
                        &version,
                        tv.request.options(),
                        ToolSource::Argument,
                    );
                    let resolve_options = request
                        .as_ref()
                        .ok()
                        .and_then(|request| request.resolve_options(base_resolve_options).ok());
                    if let (Ok(request), Some(mut resolve_options)) = (request, resolve_options)
                        && resolve_options.before_date.is_some()
                    {
                        resolve_options.use_locked_version = false;
                        resolve_options.latest_versions = true;
                        match request.resolve(config, &resolve_options).await {
                            Ok(resolved_tv) => tv = resolved_tv,
                            Err(err) => debug!("failed to resolve specified {request}: {err}"),
                        }
                    } else if version == "latest" {
                        if let Some(latest_version) = crate::backend::get(&ba)
                            .and_then(|b| {
                                b.latest_installed_version(Some("latest".to_string())).ok()
                            })
                            .flatten()
                        {
                            tv.version = latest_version;
                        }
                    } else {
                        tv.version = version;
                    }
                }
                tools.push((ba, tv));
            }
            // Deduplicate after potential "latest" -> concrete-version resolution.
            let mut seen_after: BTreeSet<(String, String)> = BTreeSet::new();
            tools.retain(|(ba, tv)| seen_after.insert((ba.short.clone(), tv.version.clone())));
            tools
        }
    }

    fn show_dry_run(&self, tools: &[LockTool], platforms: &[Platform]) -> Result<()> {
        miseprintln!("{} Dry run - would update:", style("→").yellow());
        for (ba, tv) in tools {
            let backend = crate::backend::get(ba);
            for platform in platforms {
                // Expand platform variants just like process_tools does
                let variants = if let Some(ref backend) = backend {
                    backend.platform_variants(platform)
                } else {
                    vec![platform.clone()]
                };
                for variant in variants {
                    miseprintln!(
                        "  {} {}@{} for {}",
                        style("✓").green(),
                        style(&ba.short).bold(),
                        tv.version,
                        style(variant.to_key()).blue()
                    );
                }
            }
        }
        Ok(())
    }

    async fn process_tools(
        &self,
        settings: &Settings,
        tools: &[LockTool],
        platforms: &[Platform],
        lockfile: &mut Lockfile,
    ) -> Result<(Vec<(String, String, bool)>, Vec<String>)> {
        let jobs = self.jobs.unwrap_or(settings.jobs);
        let semaphore = Arc::new(Semaphore::new(jobs));
        let mut jset: JoinSet<LockResolutionResult> = JoinSet::new();
        let mut results = Vec::new();

        let mpr = MultiProgressReport::get();

        // Collect all platform variants for each tool/platform combination
        let mut all_tasks: Vec<(
            crate::cli::args::BackendArg,
            crate::toolset::ToolVersion,
            Platform,
        )> = Vec::new();
        for (ba, tv) in tools {
            let backend = crate::backend::get(ba);
            for platform in platforms {
                // Get all variants for this platform from the backend
                let variants = if let Some(ref backend) = backend {
                    backend.platform_variants(platform)
                } else {
                    vec![platform.clone()]
                };
                for variant in variants {
                    all_tasks.push((ba.clone(), tv.clone(), variant));
                }
            }
        }

        let total_tasks = all_tasks.len();
        let pr = mpr.add("lock");
        pr.set_length(total_tasks as u64);

        // Spawn tasks for each tool/platform variant combination
        for (ba, tv, platform) in all_tasks {
            let semaphore = semaphore.clone();
            let backend = crate::backend::get(&ba);

            jset.spawn(async move {
                let _permit = semaphore.acquire().await;
                lockfile::resolve_tool_lock_info(ba, tv, platform, backend).await
            });
        }

        // Collect all results
        // Defer provenance errors until after all results are applied so unaffected
        // tools' entries aren't lost.
        let mut completed = 0;
        let mut provenance_errors: Vec<String> = Vec::new();
        while let Some(result) = jset.join_next().await {
            completed += 1;
            match result {
                Ok(resolution) => {
                    let short = resolution.0.clone();
                    let version = resolution.1.clone();
                    let platform_key = resolution.3.to_key();
                    let ok = resolution.4.is_ok();
                    if let Err(msg) = &resolution.4 {
                        debug!("{msg}");
                    }
                    pr.set_message(format!("{}@{} {}", short, version, platform_key));
                    pr.set_position(completed);
                    if let Err(e) = lockfile::apply_lock_result(lockfile, resolution) {
                        provenance_errors.push(e.to_string());
                        results.push((short, platform_key, false));
                    } else {
                        results.push((short, platform_key, ok));
                    }
                }
                Err(e) => {
                    warn!("Task failed: {}", e);
                }
            }
        }

        pr.finish_with_message(format!("{} platform entries", total_tasks));

        Ok((results, provenance_errors))
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise lock</bold>                       # update lockfile for all common platforms
    $ <bold>mise lock node python</bold>           # update only node and python
    $ <bold>mise lock --platform linux-x64</bold>  # update only linux-x64 platform
    $ <bold>mise lock --dry-run</bold>             # show what would be updated
    $ <bold>mise lock --minimum-release-age 2024-01-01</bold>   # lock latest/fuzzy versions released before 2024-01-01
    $ <bold>mise lock --local</bold>               # update mise.local.lock for local configs
    $ <bold>mise lock --global</bold>              # update only global config lockfiles
"#
);

#[cfg(test)]
mod tests {
    use super::Lock;
    use crate::cli::args::ToolArg;
    use crate::lockfile::{Lockfile, PlatformInfo};
    use crate::toolset::{ToolRequest, ToolSource, ToolVersion};
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use std::sync::Arc;

    fn lock_cmd(tool_filters: &[&str]) -> Lock {
        Lock {
            tool: tool_filters
                .iter()
                .map(|tool| ToolArg::from_str(tool).unwrap())
                .collect(),
            jobs: None,
            dry_run: false,
            platform: vec![],
            local: false,
            global: false,
            minimum_release_age: None,
        }
    }

    fn lockfile_with_dummy() -> Lockfile {
        let mut lockfile = Lockfile::default();
        lockfile.set_platform_info(
            "dummy",
            "1.0.0",
            Some("asdf:dummy"),
            &BTreeMap::new(),
            "linux-x64",
            PlatformInfo {
                checksum: Some("sha256:dummy".to_string()),
                ..Default::default()
            },
        );
        lockfile
    }

    fn lockfile_with_legacy_aqua_jq() -> Lockfile {
        let mut lockfile = Lockfile::default();
        lockfile.set_platform_info(
            "jq",
            "1.7.1",
            Some("aqua:jqlang/jq"),
            &BTreeMap::new(),
            "linux-x64",
            PlatformInfo {
                checksum: Some("sha256:jq".to_string()),
                ..Default::default()
            },
        );
        lockfile
    }

    fn configured_tool(
        backend: &str,
        version: &str,
    ) -> (crate::cli::args::BackendArg, ToolVersion) {
        let ba = crate::cli::args::BackendArg::new(backend.to_string(), Some(backend.to_string()));
        let request =
            ToolRequest::new(Arc::new(ba.clone()), version, ToolSource::Argument).unwrap();
        let tv = ToolVersion::new(request, version.to_string());
        (ba, tv)
    }

    #[test]
    fn test_is_unfiltered_lock_run_without_tool_filter() {
        let cmd = lock_cmd(&[]);
        assert!(cmd.is_unfiltered_lock_run());
    }

    #[test]
    fn test_is_not_unfiltered_lock_run_with_tool_filter() {
        let cmd = lock_cmd(&["tiny"]);
        assert!(!cmd.is_unfiltered_lock_run());
    }

    #[test]
    fn test_prune_stale_entries_with_empty_tools_prunes_all_entries() {
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy();
        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, &[]);
        assert_eq!(
            pruned,
            std::collections::BTreeSet::from(["dummy".to_string()])
        );
        assert!(lockfile.all_platform_keys().is_empty());
    }

    #[test]
    fn test_prune_stale_entries_with_filter_keeps_existing_entries() {
        let cmd = lock_cmd(&["tiny"]);
        let mut lockfile = lockfile_with_dummy();
        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, &[]);
        assert!(pruned.is_empty());
        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["linux-x64".to_string()])
        );
    }

    #[test]
    fn test_prune_stale_entries_preserves_legacy_keyed_backend_match() {
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_legacy_aqua_jq();
        let tools = vec![configured_tool("aqua:jqlang/jq", "1.7.1")];

        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, &tools);
        assert!(pruned.is_empty());

        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["linux-x64".to_string()])
        );
    }

    #[test]
    fn test_filtered_run_prunes_stale_version() {
        // Simulate: lockfile has dummy@1.0.0, resolved version is now 2.0.0
        let cmd = lock_cmd(&["dummy"]);
        let mut lockfile = lockfile_with_dummy(); // has dummy@1.0.0
        let tools = vec![configured_tool("dummy", "2.0.0")];

        cmd.prune_stale_versions(&mut lockfile, &tools);

        // Old version entry should be removed
        assert!(lockfile.all_platform_keys().is_empty());
    }

    #[test]
    fn test_filtered_run_preserves_current_version() {
        // Simulate: lockfile has dummy@1.0.0, resolved version is still 1.0.0
        let cmd = lock_cmd(&["dummy"]);
        let mut lockfile = lockfile_with_dummy(); // has dummy@1.0.0
        let tools = vec![configured_tool("dummy", "1.0.0")];

        cmd.prune_stale_versions(&mut lockfile, &tools);

        // Entry should still be there
        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["linux-x64".to_string()])
        );
    }

    #[test]
    fn test_filtered_run_preserves_non_targeted_tools() {
        // Simulate: lockfile has dummy@1.0.0 and jq@1.7.1, filter targets only dummy
        let cmd = lock_cmd(&["dummy"]);
        let mut lockfile = lockfile_with_dummy(); // has dummy@1.0.0
        lockfile.set_platform_info(
            "jq",
            "1.7.1",
            Some("aqua:jqlang/jq"),
            &BTreeMap::new(),
            "macos-x64",
            PlatformInfo {
                checksum: Some("sha256:jq".to_string()),
                ..Default::default()
            },
        );
        // Resolve dummy to a new version; jq is not targeted
        let tools = vec![configured_tool("dummy", "2.0.0")];

        cmd.prune_stale_versions(&mut lockfile, &tools);

        // dummy@1.0.0 (linux-x64) should be removed, jq@1.7.1 (macos-x64) should remain
        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["macos-x64".to_string()])
        );
    }

    #[test]
    fn test_unfiltered_run_prunes_stale_version() {
        // Unfiltered runs should prune stale versions just like filtered runs
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy(); // has dummy@1.0.0
        let tools = vec![configured_tool("dummy", "2.0.0")];

        cmd.prune_stale_versions(&mut lockfile, &tools);

        // Old version entry should be removed
        assert!(lockfile.all_platform_keys().is_empty());
    }

    #[test]
    fn test_unfiltered_run_preserves_current_version() {
        // Unfiltered runs should preserve current versions
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy(); // has dummy@1.0.0
        let tools = vec![configured_tool("dummy", "1.0.0")];

        cmd.prune_stale_versions(&mut lockfile, &tools);

        // Entry should still be there
        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["linux-x64".to_string()])
        );
    }
}
