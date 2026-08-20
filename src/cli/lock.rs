use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::{Config, ConfigMap};
use crate::file::display_path;
use crate::install_before::resolve_cli_minimum_release_age;
use crate::lockfile::{self, LockResolutionResult, Lockfile};
use crate::platform::Platform;
use crate::task::Task;
use crate::toolset::{
    ResolveOptions, ToolRequest, ToolSource, ToolVersionOptions, Toolset, ToolsetBuilder,
};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{cli::args::ToolArg, config::Settings};
use console::style;
use eyre::Result;
use jiff::Timestamp;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// A tool to lock for a specific lockfile target.
type LockTool = (crate::cli::args::BackendArg, crate::toolset::ToolVersion);
type ToolSelectors = (BTreeSet<String>, BTreeSet<String>);

struct LockCollectionContext<'a> {
    config: &'a Arc<Config>,
    toolset: &'a Toolset,
    config_files: &'a ConfigMap,
    tasks: &'a BTreeMap<String, Task>,
    resolve_options: &'a ResolveOptions,
}

fn request_matches(a: &ToolRequest, b: &ToolRequest) -> bool {
    a.version() == b.version() && a.options() == b.options()
}

fn lock_tool_matches(a: &LockTool, b: &LockTool) -> bool {
    a.0.full() == b.0.full()
        && a.1.version == b.1.version
        && a.1.request.options() == b.1.request.options()
}

fn push_unique_lock_tool(tools: &mut Vec<LockTool>, tool: LockTool) {
    if !tools
        .iter()
        .any(|existing| lock_tool_matches(existing, &tool))
    {
        tools.push(tool);
    }
}

fn options_for_lock_request(
    tv: &crate::toolset::ToolVersion,
    specified_request: &ToolRequest,
) -> ToolVersionOptions {
    specified_request
        .ba()
        .opts_with_config(Some(tv.request.options()))
}

fn is_known_concrete_lock_version(backend: &dyn crate::backend::Backend, version: &str) -> bool {
    if backend.is_rolling_channel(version) || crate::semver::is_npm_semver_range_query(version) {
        return false;
    }
    version.matches('.').count() >= 2
        || backend.is_exact_version(version)
        || backend
            .list_installed_versions()
            .iter()
            .any(|installed| installed == version)
}

/// Update lockfile checksums and URLs for all specified platforms
///
/// Updates checksums and download URLs for all platforms already specified in the lockfile.
/// If no lockfile exists, shows what would be created based on the current configuration,
/// including tools declared by tasks.
/// This allows you to refresh lockfile data for platforms other than the one you're currently on.
/// Operates on the lockfile in the current config root. Use TOOL arguments to target specific tools.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Lock {
    /// Tool(s) to update in lockfile
    /// e.g.: node python
    /// If not specified, all configured and task-specific tools will be updated
    #[clap(value_name = "TOOL", verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,

    /// Target only global config lockfiles (~/.config/mise/mise.lock and system config)
    /// By default, only the active project config root is locked
    #[clap(long, short, verbatim_doc_comment)]
    pub global: bool,

    /// Number of jobs to run in parallel
    /// Values below 1 are treated as 1
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

    /// Re-resolve fuzzy version selectors against the latest available versions
    ///
    /// By default, `mise lock` refreshes metadata for the currently locked versions.
    /// With this flag, selectors like "latest", "lts", or prefixes like "20" are
    /// re-resolved against the latest matching remote versions, so the lockfile
    /// advances without installing anything. Config files are never modified:
    /// exactly pinned versions resolve to themselves and stay unchanged
    /// (use `mise upgrade --bump` to rewrite pins in mise.toml).
    #[clap(long, verbatim_doc_comment)]
    pub bump: bool,

    /// Output version changes as JSON
    ///
    /// Prints an array of objects describing lockfile version changes:
    /// name, backend, lockfile, old_versions, new_versions.
    /// Version lists keep config/lockfile order; they are not sorted.
    /// Only version-level changes are reported: checksum/URL refreshes for
    /// unchanged versions produce no entries, so plain `mise lock --json`
    /// typically prints `[]` while still updating the lockfile.
    /// Suppresses the human-readable output. Combine with `--dry-run` to
    /// detect available updates without writing the lockfile.
    #[clap(long, verbatim_doc_comment)]
    pub json: bool,

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

/// A lockfile version change reported by `--json`
#[derive(serde::Serialize)]
struct LockChange {
    name: String,
    backend: Option<String>,
    lockfile: String,
    old_versions: Vec<String>,
    new_versions: Vec<String>,
}

struct LockTaskResult {
    short: String,
    version: String,
    platform: String,
    status: LockTaskStatus,
}

#[derive(Debug, Eq, PartialEq)]
enum LockTaskStatus {
    Updated,
    Unresolved,
    Failed,
    ProvenanceFailed,
}

fn classify_lock_result(
    resolution_error: Option<String>,
    error_is_fatal: bool,
    applied: bool,
) -> (LockTaskStatus, Option<String>) {
    if let Some(error) = resolution_error.filter(|_| error_is_fatal) {
        (LockTaskStatus::Failed, Some(error))
    } else if applied {
        (LockTaskStatus::Updated, None)
    } else {
        (LockTaskStatus::Unresolved, None)
    }
}

impl Lock {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::get();
        let config = Config::get().await?;
        if !self.dry_run {
            lockfile::migrate_monorepo_lockfiles(&config)?;
        }
        let before_date = self.get_before_date()?;
        let lock_resolve_options = ResolveOptions {
            before_date,
            filter_installed_versions_by_release_date: true,
            latest_versions: self.bump,
            use_locked_version: !self.bump,
            // Lock moving channels to their current concrete value without making
            // ordinary `latest` requests ignore an installed concrete version.
            resolve_rolling_channels: true,
            ..Default::default()
        };
        let monorepo_union = if !self.global && config.monorepo_lockfile_root().is_some() {
            Some(config.monorepo_union().await?)
        } else {
            None
        };
        let effective_config_files = monorepo_union
            .as_ref()
            .map(|monorepo_union| &monorepo_union.config_files)
            .unwrap_or(&config.config_files);
        let task_load_context = crate::task::TaskLoadContext::all();
        let tasks = config.tasks_with_context(Some(&task_load_context)).await?;

        let ts_owned;
        let ts = if let Some(monorepo_union) = &monorepo_union {
            let mut monorepo_ts: Toolset = monorepo_union.tool_request_set.clone().into();
            monorepo_ts
                .resolve_with_opts(&config, &lock_resolve_options)
                .await?;
            ts_owned = monorepo_ts;
            &ts_owned
        } else {
            let builder = ToolsetBuilder::new().with_resolve_options(lock_resolve_options.clone());
            ts_owned = builder.build(&config).await?;
            &ts_owned
        };

        let scoped_config_paths = self.config_paths_in_lock_scope(&config, effective_config_files);
        let lockfile_targets =
            self.get_lockfile_targets(&config, effective_config_files, &scoped_config_paths);
        let mut has_lock_targets = false;
        let mut all_resolution_errors: Vec<String> = Vec::new();
        let mut all_platform_regressions: Vec<String> = Vec::new();
        let mut all_changes: Vec<LockChange> = Vec::new();
        let collection_context = LockCollectionContext {
            config: &config,
            toolset: ts,
            config_files: effective_config_files,
            tasks: &tasks,
            resolve_options: &lock_resolve_options,
        };

        // Resolve every target before writing any lockfile. A task tool can fail to
        // resolve just like a config-level tool, and a later failure must not leave
        // earlier environment/local/monorepo lockfiles partially updated.
        let mut prepared_targets = Vec::with_capacity(lockfile_targets.len());
        for (lockfile_path, config_paths) in &lockfile_targets {
            let tools = self
                .get_tools_to_lock(&collection_context, lockfile_path, config_paths)
                .await?;
            let configured_selectors = self.configured_tool_selectors_for_target(
                &config,
                &tools,
                lockfile_path,
                config_paths,
                effective_config_files,
            );
            prepared_targets.push((
                lockfile_path.clone(),
                config_paths.clone(),
                tools,
                configured_selectors,
            ));
        }

        for (lockfile_path, _config_paths, tools, configured_selectors) in prepared_targets {
            if configured_selectors
                .as_ref()
                .is_some_and(|(configured_tools, _)| !configured_tools.is_empty())
            {
                has_lock_targets = true;
            }

            if tools.is_empty() {
                // `tools` can be empty either because config has no tools, or because a filter excludes all.
                // For unfiltered runs (`mise lock`), this means "prune all stale lockfile entries".
                if self.dry_run {
                    let lockfile = Lockfile::read(&lockfile_path)?;
                    if self.json {
                        all_changes.extend(self.compute_version_changes(
                            &lockfile,
                            &tools,
                            &lockfile_path,
                        ));
                    }
                    let stale_tools =
                        self.stale_entries_if_pruned(&lockfile, configured_selectors.as_ref());
                    self.show_stale_prune_message(&lockfile_path, &stale_tools, true)?;
                    if !stale_tools.is_empty() {
                        has_lock_targets = true;
                    }
                } else {
                    let _lock = crate::lock_file::LockFile::new(&lockfile_path)
                        .with_callback(|l| debug!("waiting for lock on {}", display_path(l)))
                        .lock()?;
                    let mut lockfile = Lockfile::read(&lockfile_path)?;
                    if self.json {
                        all_changes.extend(self.compute_version_changes(
                            &lockfile,
                            &tools,
                            &lockfile_path,
                        ));
                    }
                    let pruned_tools = self.prune_stale_entries_if_needed(
                        &mut lockfile,
                        configured_selectors.as_ref(),
                    );
                    if !pruned_tools.is_empty() {
                        lockfile.write(&lockfile_path)?;
                        self.show_stale_prune_message(&lockfile_path, &pruned_tools, false)?;
                        has_lock_targets = true;
                    }
                }
                continue;
            }
            has_lock_targets = true;

            let target_platforms = self.determine_target_platforms(&lockfile_path)?;

            if !self.json {
                miseprintln!(
                    "{} Targeting {} platform(s) for {}: {}",
                    style("→").cyan(),
                    target_platforms.len(),
                    style(display_path(&lockfile_path)).cyan(),
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
            }

            if self.dry_run {
                self.show_dry_run(&tools, &target_platforms)?;
                let lockfile = Lockfile::read(&lockfile_path)?;
                if self.json {
                    all_changes.extend(self.compute_version_changes(
                        &lockfile,
                        &tools,
                        &lockfile_path,
                    ));
                }
                if self.is_unfiltered_lock_run() {
                    let stale_tools =
                        self.stale_entries_if_pruned(&lockfile, configured_selectors.as_ref());
                    self.show_stale_prune_message(&lockfile_path, &stale_tools, true)?;
                }
                let stale_versions = self.stale_versions_if_pruned(&lockfile, &tools);
                self.show_stale_version_prune_message(&lockfile_path, &stale_versions, true)?;
                continue;
            }

            // Process tools and update lockfile
            let _lock = crate::lock_file::LockFile::new(&lockfile_path)
                .with_callback(|l| debug!("waiting for lock on {}", display_path(l)))
                .lock()?;
            let mut lockfile = Lockfile::read(&lockfile_path)?;
            if self.json {
                all_changes.extend(self.compute_version_changes(&lockfile, &tools, &lockfile_path));
            }
            let stale_tools =
                self.prune_stale_entries_if_needed(&mut lockfile, configured_selectors.as_ref());
            self.show_stale_prune_message(&lockfile_path, &stale_tools, false)?;

            // Compute stale versions BEFORE process_tools so provenance checks can
            // compare against old version entries. Actual pruning happens after.
            let stale_versions = self.stale_versions_if_pruned(&lockfile, &tools);

            let (results, resolution_errors) = self
                .process_tools(&settings, &tools, &target_platforms, &mut lockfile)
                .await?;
            all_resolution_errors.extend(resolution_errors);

            let platform_regressions =
                self.platform_regression_errors(&lockfile, &stale_versions, &results);
            if !platform_regressions.is_empty() {
                all_platform_regressions.extend(platform_regressions);
                continue;
            }

            // Prune stale versions AFTER provenance checks complete
            self.prune_stale_versions(&mut lockfile, &tools);
            self.show_stale_version_prune_message(&lockfile_path, &stale_versions, false)?;

            // Save lockfile before raising resolution errors so non-regressing
            // tools' entries are preserved
            lockfile.write(&lockfile_path)?;

            // Print summary
            if !self.json {
                let successful = results
                    .iter()
                    .filter(|result| matches!(result.status, LockTaskStatus::Updated))
                    .count();
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
                    style(display_path(&lockfile_path)).cyan()
                );
            }
        }

        if !has_lock_targets && !self.json {
            miseprintln!("{} No tools configured to lock", style("!").yellow());
        }

        // Update config files when a specific version is requested that doesn't match
        // the current prefix (e.g., `mise lock tiny@3.0.1` when config has `tiny = "2"`).
        // Never under --bump, which is documented to leave config files untouched.
        if !self.bump {
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
                if !self.json {
                    for bump in &bumps {
                        miseprintln!(
                            "Would update {} from {} to {} in {}",
                            bump.tool_name,
                            bump.old_version,
                            bump.new_version,
                            display_path(&bump.config_path)
                        );
                    }
                }
            } else {
                apply_config_bumps(&config, &bumps)?;
            }
        }

        if self.json {
            miseprintln!("{}", serde_json::to_string_pretty(&all_changes)?);
        }

        all_platform_regressions.extend(all_resolution_errors);
        if !all_platform_regressions.is_empty() {
            return Err(eyre::eyre!(all_platform_regressions.join("\n")));
        }

        Ok(())
    }

    /// Compare the versions currently in the lockfile against the freshly
    /// resolved tool versions and report the differences. Only tools targeted
    /// by this run are compared; on unfiltered runs, lockfile tools that are
    /// no longer configured (and will be pruned) are reported with an empty
    /// `new_versions`.
    ///
    /// Versions are never sorted here: `new_versions` keeps resolution order
    /// (which follows config declaration order) and `old_versions` keeps
    /// lockfile entry order. mise does not impose orderings on version
    /// strings — see "DO NOT ASSUME SEMVER" in the repo guide.
    fn compute_version_changes(
        &self,
        lockfile: &Lockfile,
        tools: &[LockTool],
        lockfile_path: &Path,
    ) -> Vec<LockChange> {
        let mut new_versions: indexmap::IndexMap<String, (Option<String>, Vec<String>)> =
            indexmap::IndexMap::new();
        for (ba, tv) in tools {
            let entry = new_versions
                .entry(ba.short.clone())
                .or_insert_with(|| (Some(ba.full()), Vec::new()));
            if !entry.1.contains(&tv.version) {
                entry.1.push(tv.version.clone());
            }
        }
        let mut shorts: Vec<String> = new_versions.keys().cloned().collect();
        if self.is_unfiltered_lock_run() {
            for short in lockfile.tools().keys() {
                if !new_versions.contains_key(short) {
                    shorts.push(short.clone());
                }
            }
        }
        let mut changes = vec![];
        for short in shorts {
            let old_entries = lockfile.tools().get(&short);
            let mut old_versions: Vec<String> = vec![];
            for entry in old_entries.into_iter().flatten() {
                if !old_versions.contains(&entry.version) {
                    old_versions.push(entry.version.clone());
                }
            }
            let (backend, versions) = new_versions.shift_remove(&short).unwrap_or_else(|| {
                let backend =
                    old_entries.and_then(|entries| entries.iter().find_map(|t| t.backend.clone()));
                (backend, Vec::new())
            });
            let old_set: BTreeSet<&String> = old_versions.iter().collect();
            let new_set: BTreeSet<&String> = versions.iter().collect();
            if old_set != new_set {
                changes.push(LockChange {
                    name: short,
                    backend,
                    lockfile: display_path(lockfile_path),
                    old_versions,
                    new_versions: versions,
                });
            }
        }
        changes
    }

    /// Get the before_date from the CLI --minimum-release-age flag only.
    /// Per-tool and global setting fallbacks are handled during tool request resolution.
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        resolve_cli_minimum_release_age(self.minimum_release_age.as_deref())
    }

    fn is_unfiltered_lock_run(&self) -> bool {
        self.tool.is_empty()
    }

    fn prune_stale_entries_if_needed(
        &self,
        lockfile: &mut Lockfile,
        configured_selectors: Option<&ToolSelectors>,
    ) -> BTreeSet<String> {
        let Some((configured_tools, configured_backends)) = configured_selectors else {
            return BTreeSet::new();
        };
        if !self.is_unfiltered_lock_run() {
            return BTreeSet::new();
        }
        let stale_tools =
            self.stale_entries_for_selectors(lockfile, configured_tools, configured_backends);
        if !stale_tools.is_empty() {
            lockfile.retain_tools_by_short_or_backend(configured_tools, configured_backends);
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
        configured_selectors: Option<&ToolSelectors>,
    ) -> BTreeSet<String> {
        let Some((configured_tools, configured_backends)) = configured_selectors else {
            return BTreeSet::new();
        };
        if !self.is_unfiltered_lock_run() {
            return BTreeSet::new();
        }
        self.stale_entries_for_selectors(lockfile, configured_tools, configured_backends)
    }

    fn stale_versions_if_pruned(
        &self,
        lockfile: &Lockfile,
        tools: &[LockTool],
    ) -> BTreeMap<String, Vec<String>> {
        let current_versions = self.current_tool_versions(tools);
        self.stale_versions_for_current(lockfile, &current_versions)
    }

    fn platform_regression_errors(
        &self,
        lockfile: &Lockfile,
        stale_versions: &BTreeMap<String, Vec<String>>,
        results: &[LockTaskResult],
    ) -> Vec<String> {
        // Cross-platform locking is best-effort because many tools intentionally
        // support only a subset of the targeted platforms. A skipped platform is
        // only fatal when pruning the stale version would remove an entry that was
        // previously resolvable. Multiple current versions are ambiguous because
        // an unresolved result cannot be paired reliably with a particular stale
        // version, so preserve the best-effort behavior in that case.
        let current_versions = results.iter().fold(
            BTreeMap::<&str, BTreeSet<&str>>::new(),
            |mut versions, result| {
                versions
                    .entry(&result.short)
                    .or_default()
                    .insert(&result.version);
                versions
            },
        );
        results
            .iter()
            .filter(|result| !matches!(result.status, LockTaskStatus::Updated))
            .filter_map(|result| {
                if current_versions.get(result.short.as_str())?.len() != 1 {
                    return None;
                }
                let stale_versions = stale_versions.get(&result.short)?;
                let locked_tools = lockfile.tools().get(&result.short)?;
                if locked_tools.iter().any(|tool| {
                    tool.version == result.version
                        && tool.platforms.contains_key(&result.platform)
                }) {
                    return None;
                }
                let lost_versions = locked_tools
                    .iter()
                    .filter(|tool| {
                        stale_versions.contains(&tool.version)
                            && tool.platforms.contains_key(&result.platform)
                    })
                    .map(|tool| tool.version.as_str())
                    .collect::<Vec<_>>();
                if lost_versions.is_empty() {
                    return None;
                }
                Some(format!(
                    "failed to resolve {}@{} for {}; refusing to replace locked version(s) {} that support this platform",
                    result.short,
                    result.version,
                    result.platform,
                    lost_versions.join(", ")
                ))
            })
            .collect()
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
        if stale_versions.is_empty() || self.json {
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
    ) -> ToolSelectors {
        let configured_tools: BTreeSet<String> =
            tools.iter().map(|(ba, _)| ba.short.clone()).collect();
        let configured_backends: BTreeSet<String> = tools.iter().map(|(ba, _)| ba.full()).collect();
        (configured_tools, configured_backends)
    }

    fn configured_tool_selectors_for_target(
        &self,
        config: &Config,
        tools: &[LockTool],
        target_lockfile_path: &Path,
        config_paths: &[PathBuf],
        effective_config_files: &ConfigMap,
    ) -> Option<ToolSelectors> {
        let (mut configured_tools, mut configured_backends) = self.configured_tool_selectors(tools);
        let config_paths: BTreeSet<&PathBuf> = config_paths.iter().collect();

        for (path, cf) in effective_config_files {
            let source = cf.source();
            let source_lockfile_matches = lockfile::lockfile_path_for_tool_source(config, &source)
                .is_some_and(|(source_lockfile, _)| source_lockfile == target_lockfile_path);
            if !(config_paths.contains(path)
                || source.is_idiomatic_version_file() && source_lockfile_matches)
            {
                continue;
            }
            let trs = match cf.to_tool_request_set() {
                Ok(trs) => trs,
                Err(err) => {
                    debug!(
                        "skipping stale-tool pruning for {} because {} could not be parsed: {err}",
                        display_path(target_lockfile_path),
                        display_path(path)
                    );
                    return None;
                }
            };
            for (ba, _, _) in trs.iter() {
                // Pruning answers whether the tool is still declared, not whether its
                // backend can resolve on this machine. In particular, OS-restricted
                // tools may be intentionally unavailable on the current platform.
                configured_tools.insert(ba.short.clone());
                configured_backends.insert(ba.full());
            }
        }

        Some((configured_tools, configured_backends))
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
        if stale_tools.is_empty() || self.json {
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

    fn config_paths_in_lock_scope(
        &self,
        config: &Config,
        effective_config_files: &ConfigMap,
    ) -> BTreeSet<PathBuf> {
        if self.global {
            return effective_config_files
                .keys()
                .filter(|path| crate::config::is_global_config(path))
                .cloned()
                .collect();
        }
        if let Some(monorepo_root) = config.monorepo_lockfile_root() {
            return effective_config_files
                .keys()
                .filter(|path| {
                    !crate::config::is_global_config(path) && path.starts_with(&monorepo_root)
                })
                .cloned()
                .collect();
        }
        let target_root = Self::target_lock_scope_root(config);

        effective_config_files
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
        effective_config_files: &ConfigMap,
        scoped_config_paths: &BTreeSet<PathBuf>,
    ) -> indexmap::IndexMap<PathBuf, Vec<PathBuf>> {
        let mut targets: indexmap::IndexMap<PathBuf, Vec<PathBuf>> = indexmap::IndexMap::new();
        for (path, cf) in effective_config_files.iter() {
            if !scoped_config_paths.contains(path) {
                continue;
            }
            if !cf.source().is_mise_toml() {
                continue;
            }
            let (lockfile_path, is_local) = lockfile::lockfile_path_for_config(
                path,
                config.monorepo_lockfile_root().as_deref(),
            );
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
        context: &LockCollectionContext<'_>,
        target_lockfile_path: &Path,
        config_paths: &[PathBuf],
    ) -> Result<Vec<LockTool>> {
        let config = context.config;
        let ts = context.toolset;
        let config_paths_set: BTreeSet<&PathBuf> = config_paths.iter().collect();

        let mut all_tools: Vec<LockTool> = Vec::new();

        // First pass: tools from the resolved toolset whose source maps to this lockfile
        for (backend, tv) in ts.list_current_versions() {
            if let Some((source_lockfile, _)) =
                lockfile::lockfile_path_for_tool_source(config, tv.request.source())
            {
                if source_lockfile != target_lockfile_path {
                    continue;
                }
            } else if tv.request.source().path().is_some() {
                // Path-backed sources that do not map to a mise lockfile, such
                // as .tool-versions and tool stubs, should not be folded into
                // an arbitrary project mise.lock.
                continue;
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
            if tv.version == "latest"
                || backend
                    .ba()
                    .backend()
                    .is_ok_and(|backend| backend.is_rolling_channel(&tv.version))
            {
                continue;
            }
            push_unique_lock_tool(&mut all_tools, (backend.ba().as_ref().clone(), tv));
        }

        // Second pass: iterate config files matching this lockfile to catch
        // tools that were overridden by a higher-priority config
        for (path, cf) in context.config_files.iter() {
            let source = cf.source();
            let source_lockfile_matches = lockfile::lockfile_path_for_tool_source(config, &source)
                .is_some_and(|(source_lockfile, _)| source_lockfile == target_lockfile_path);
            if !(config_paths_set.contains(path)
                || source.is_idiomatic_version_file() && source_lockfile_matches)
            {
                continue;
            }
            if let Ok(trs) = cf.to_tool_request_set() {
                for (ba, requests, source) in trs.iter() {
                    for request in requests {
                        if ba.backend().is_ok() {
                            // Check if the resolved toolset has a matching request.
                            let mut matched_resolved = false;
                            if let Some(resolved_tv) = ts.versions.get(ba.as_ref()) {
                                for tv in &resolved_tv.versions {
                                    if request_matches(&tv.request, request)
                                        && tv.version != "latest"
                                        && !ba.backend().is_ok_and(|backend| {
                                            backend.is_rolling_channel(&tv.version)
                                        })
                                    {
                                        matched_resolved = true;
                                        push_unique_lock_tool(
                                            &mut all_tools,
                                            (ba.as_ref().clone(), tv.clone()),
                                        );
                                    }
                                }
                            }
                            let requested_tool = self.tool.is_empty()
                                || self.tool.iter().any(|tool| tool.ba.short == ba.short);
                            let active_unresolved = requested_tool
                                && ts.versions.get(ba.as_ref()).is_some_and(|tvl| {
                                    tvl.requests
                                        .iter()
                                        .any(|active| request_matches(active, request))
                                });
                            // Resolve overridden requests through the same path as active
                            // tools when the request cannot be copied from the resolved
                            // toolset. Keep this broad only for idiomatic version files;
                            // other sources preserve the previous latest-only behavior.
                            let should_resolve_overridden = active_unresolved
                                || request.version() == "latest"
                                || source.is_idiomatic_version_file();
                            if !matched_resolved && should_resolve_overridden {
                                let mut resolve_options = match request
                                    .resolve_options(context.resolve_options)
                                {
                                    Ok(opts) => opts,
                                    Err(err) => {
                                        if active_unresolved {
                                            return Err(err.wrap_err(format!(
                                                    "failed to resolve options for {request} for lockfile {}",
                                                    display_path(target_lockfile_path)
                                                )));
                                        } else {
                                            debug!(
                                                "failed to resolve options for {request}: {err}"
                                            );
                                            continue;
                                        }
                                    }
                                };
                                resolve_options.use_locked_version = false;
                                if resolve_options.before_date.is_some() {
                                    resolve_options.latest_versions = true;
                                }
                                match request.resolve(config, &resolve_options).await {
                                    Ok(tv) => {
                                        push_unique_lock_tool(
                                            &mut all_tools,
                                            (ba.as_ref().clone(), tv),
                                        );
                                    }
                                    Err(err) => {
                                        if active_unresolved {
                                            return Err(err.wrap_err(format!(
                                                "failed to resolve {request} for lockfile {}",
                                                display_path(target_lockfile_path)
                                            )));
                                        } else {
                                            debug!("failed to resolve overridden {request}: {err}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        self.add_task_tools_to_lock(
            config,
            context.tasks,
            target_lockfile_path,
            config_paths,
            context,
            &mut all_tools,
        )
        .await?;

        if self.tool.is_empty() {
            Ok(all_tools)
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
                    let request_options = options_for_lock_request(&tv, request);
                    let backend = crate::backend::get(&ba);
                    let effective_version = match &backend {
                        Some(backend) => config.resolve_alias(backend, &version).await?,
                        None => version.clone(),
                    };
                    let request = ToolRequest::new_opts(
                        Arc::new(ba.clone()),
                        &effective_version,
                        request_options.clone(),
                        ToolSource::Argument,
                    )?;
                    let mut resolve_options = request.resolve_options(context.resolve_options)?;
                    resolve_options.use_locked_version = false;
                    if self.bump || resolve_options.before_date.is_some() {
                        resolve_options.latest_versions = true;
                    }
                    let resolved_tv =
                        request
                            .resolve(config, &resolve_options)
                            .await
                            .map_err(|err| {
                                err.wrap_err(format!("failed to resolve specified {request}"))
                            })?;
                    let mut concrete = backend.as_ref().is_none_or(|backend| {
                        is_known_concrete_lock_version(backend.as_ref(), &resolved_tv.version)
                    });
                    let offline = Settings::get().offline() || resolve_options.offline;
                    if !concrete
                        && !offline
                        && let Some(backend) = &backend
                        && !backend.is_rolling_channel(&resolved_tv.version)
                        && !crate::semver::is_npm_semver_range_query(&resolved_tv.version)
                    {
                        concrete = backend
                            .list_versions_matching_with_selection_options(
                                config,
                                &resolved_tv.version,
                                &request.options(),
                                None,
                                resolve_options.refresh_remote_versions,
                            )
                            .await?
                            .contains(&resolved_tv.version);
                    }
                    if !concrete {
                        eyre::bail!("failed to resolve specified {request} to a concrete version");
                    }
                    tv = resolved_tv;
                }
                tools.push((ba, tv));
            }
            // Deduplicate after potential "latest" -> concrete-version resolution.
            let mut unique_tools = Vec::with_capacity(tools.len());
            for tool in tools {
                push_unique_lock_tool(&mut unique_tools, tool);
            }
            Ok(unique_tools)
        }
    }

    async fn add_task_tools_to_lock(
        &self,
        config: &Arc<Config>,
        tasks: &BTreeMap<String, Task>,
        target_lockfile_path: &Path,
        config_paths: &[PathBuf],
        context: &LockCollectionContext<'_>,
        all_tools: &mut Vec<LockTool>,
    ) -> Result<()> {
        for task in tasks.values() {
            let Some(config_path) =
                self.task_config_path(task, config, config_paths, context.config_files)
            else {
                continue;
            };
            let (task_lockfile_path, _) = lockfile::lockfile_path_for_config(
                &config_path,
                config.monorepo_lockfile_root().as_deref(),
            );
            if task_lockfile_path != target_lockfile_path {
                continue;
            }

            for tool in task.tool_args().map_err(|err| {
                err.wrap_err(format!("failed to parse tools for task `{}`", task.name))
            })? {
                if !self.tool.is_empty()
                    && !self
                        .tool
                        .iter()
                        .any(|requested| requested.ba.short == tool.ba.short)
                {
                    continue;
                }
                let Some(request) = tool.tvr else {
                    continue;
                };
                let request = ToolRequest::new_opts(
                    tool.ba.clone(),
                    &request.version(),
                    request.options(),
                    ToolSource::MiseToml(config_path.clone()),
                )
                .map_err(|err| {
                    err.wrap_err(format!(
                        "failed to prepare tool `{}` for task `{}`",
                        tool.ba.short, task.name
                    ))
                })?;
                let mut resolve_options = request
                    .resolve_options(context.resolve_options)
                    .map_err(|err| {
                        err.wrap_err(format!(
                            "failed to resolve options for task tool `{}` in task `{}`",
                            tool.ba.short, task.name
                        ))
                    })?;
                if self.bump || resolve_options.before_date.is_some() {
                    resolve_options.use_locked_version = false;
                    resolve_options.latest_versions = true;
                }
                let tv = request
                    .resolve(config, &resolve_options)
                    .await
                    .map_err(|err| {
                        err.wrap_err(format!(
                            "failed to resolve task tool `{}` for task `{}`",
                            tool.ba.short, task.name
                        ))
                    })?;
                push_unique_lock_tool(all_tools, (tool.ba.as_ref().clone(), tv));
            }
        }
        Ok(())
    }

    fn task_config_path(
        &self,
        task: &Task,
        config: &Config,
        config_paths: &[PathBuf],
        effective_config_files: &ConfigMap,
    ) -> Option<PathBuf> {
        if let Some(cf) = task.cf(config) {
            let path = cf.get_path();
            return config_paths
                .iter()
                .any(|candidate| candidate == path)
                .then(|| path.to_path_buf());
        }

        let config_root = task.config_root.as_ref()?;
        effective_config_files
            .iter()
            .filter(|(path, cf)| {
                config_paths.iter().any(|candidate| candidate == *path)
                    && cf.source().is_mise_toml()
                    && cf
                        .project_root()
                        .unwrap_or_else(|| cf.config_root())
                        .as_path()
                        == config_root
            })
            .max_by_key(|(path, _)| {
                let (lockfile_path, _) = lockfile::lockfile_path_for_config(
                    path,
                    config.monorepo_lockfile_root().as_deref(),
                );
                let is_base = lockfile_path
                    .file_name()
                    .is_some_and(|name| name == "mise.lock");
                (is_base, path.components().count())
            })
            .map(|(path, _)| path.clone())
    }

    fn show_dry_run(&self, tools: &[LockTool], platforms: &[Platform]) -> Result<()> {
        if self.json {
            return Ok(());
        }
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
    ) -> Result<(Vec<LockTaskResult>, Vec<String>)> {
        let jobs = crate::jobs::resolve(settings.jobs, self.jobs);
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
        // Defer resolution errors until after all results are applied so unaffected
        // tools' entries aren't lost.
        let mut completed = 0;
        let mut resolution_errors: Vec<String> = Vec::new();
        while let Some(result) = jset.join_next().await {
            completed += 1;
            match result {
                Ok(resolution) => {
                    let short = resolution.0.clone();
                    let version = resolution.1.clone();
                    let platform_key = resolution.3.to_key();
                    let resolution_error = resolution.4.as_ref().err().cloned();
                    if let Some(msg) = &resolution_error {
                        debug!("{msg}");
                    }
                    let error_is_fatal = resolution.8;
                    pr.set_message(format!("{}@{} {}", short, version, platform_key));
                    pr.set_position(completed);
                    match lockfile::apply_lock_result(lockfile, resolution) {
                        Err(e) => {
                            resolution_errors.push(e.to_string());
                            results.push(LockTaskResult {
                                short,
                                version,
                                platform: platform_key,
                                status: LockTaskStatus::ProvenanceFailed,
                            });
                        }
                        // A resolution that wrote nothing is a skip, not an
                        // update — backends that can't resolve metadata without
                        // installing return empty info rather than an error.
                        Ok(applied) => {
                            let (status, resolution_error) =
                                classify_lock_result(resolution_error, error_is_fatal, applied);
                            if let Some(error) = resolution_error {
                                resolution_errors.push(error);
                            }
                            results.push(LockTaskResult {
                                short,
                                version,
                                platform: platform_key,
                                status,
                            });
                        }
                    }
                }
                Err(e) => {
                    warn!("Task failed: {}", e);
                }
            }
        }

        // Report entries actually written, not tasks attempted
        let updated = results
            .iter()
            .filter(|result| matches!(result.status, LockTaskStatus::Updated))
            .count();
        pr.finish_with_message(format!("{} platform entries", updated));

        Ok((results, resolution_errors))
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise lock</bold>                       # update lockfile for all common platforms
    $ <bold>mise lock node python</bold>           # update only node and python
    $ <bold>mise lock --platform linux-x64</bold>  # update only linux-x64 platform
    $ <bold>mise lock --dry-run</bold>             # show what would be updated
    $ <bold>mise lock --bump</bold>                # re-resolve selectors like "latest" or "20" to the latest matching versions
    $ <bold>mise lock --bump --dry-run --json</bold>   # list available updates as JSON without writing
    $ <bold>mise lock --minimum-release-age 2024-01-01</bold>   # lock latest/fuzzy versions released before 2024-01-01
    $ <bold>mise lock --local</bold>               # update mise.local.lock for local configs
    $ <bold>mise lock --global</bold>              # update only global config lockfiles
"#
);

#[cfg(test)]
mod tests {
    use super::{
        Lock, LockTaskResult, LockTaskStatus, classify_lock_result, is_known_concrete_lock_version,
        options_for_lock_request, push_unique_lock_tool,
    };
    use crate::backend::test_helpers::RemoteVersionsBackend;
    use crate::cli::args::{BackendArg, ToolArg};
    use crate::lockfile::{Lockfile, PlatformInfo, apply_lock_result};
    use crate::platform::Platform;
    use crate::toolset::{ToolRequest, ToolSource, ToolVersion, ToolVersionOptions};
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
            bump: false,
            json: false,
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

    #[test]
    fn test_resolution_error_is_fatal_instead_of_skipped() {
        let error = "conda solve failed".to_string();
        let (status, returned_error) = classify_lock_result(Some(error.clone()), true, false);

        assert_eq!(status, LockTaskStatus::Failed);
        assert_eq!(returned_error, Some(error));
    }

    #[test]
    fn test_conda_package_resolution_error_is_fatal_and_not_applied() {
        let error = "failed to resolve conda packages".to_string();
        let resolution = (
            "ffmpeg".to_string(),
            "7.1.1".to_string(),
            "conda:ffmpeg".to_string(),
            Platform::parse("linux-x64").unwrap(),
            Err(error.clone()),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            true,
        );
        let mut lockfile = Lockfile::default();
        let resolution_error = resolution.4.as_ref().err().cloned();
        let error_is_fatal = resolution.8;

        let applied = apply_lock_result(&mut lockfile, resolution).unwrap();
        let (status, returned_error) =
            classify_lock_result(resolution_error, error_is_fatal, applied);

        assert_eq!(status, LockTaskStatus::Failed);
        assert_eq!(returned_error, Some(error));
        assert!(!lockfile.tools().contains_key("ffmpeg"));
        assert!(
            lockfile
                .get_conda_package("linux-x64", "incomplete-package")
                .is_none()
        );
    }

    #[test]
    fn test_empty_success_remains_an_unsupported_platform_skip() {
        let (status, returned_error) = classify_lock_result(None, false, false);

        assert_eq!(status, LockTaskStatus::Unresolved);
        assert_eq!(returned_error, None);
    }

    #[test]
    fn test_nonfatal_resolution_error_remains_an_unsupported_platform_skip() {
        let (status, returned_error) =
            classify_lock_result(Some("no URL for target".to_string()), false, false);

        assert_eq!(status, LockTaskStatus::Unresolved);
        assert_eq!(returned_error, None);
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

    fn lock_tool_with_options(
        short: &str,
        backend: &str,
        version: &str,
        option: Option<(&str, &str)>,
    ) -> (BackendArg, ToolVersion) {
        let ba = BackendArg::new(short.to_string(), Some(backend.to_string()));
        let mut options = ToolVersionOptions::default();
        if let Some((key, value)) = option {
            options
                .insert_option(key.to_string(), toml::Value::String(value.to_string()))
                .unwrap();
        }
        let request =
            ToolRequest::new_opts(Arc::new(ba.clone()), version, options, ToolSource::Argument)
                .unwrap();
        let tv = ToolVersion::new(request, version.to_string());
        (ba, tv)
    }

    #[test]
    fn test_lock_tool_identity_includes_backend_and_options() {
        let mut tools = Vec::new();
        push_unique_lock_tool(
            &mut tools,
            lock_tool_with_options("dummy", "http:one", "1.0.0", Some(("exe", "one"))),
        );
        push_unique_lock_tool(
            &mut tools,
            lock_tool_with_options("dummy", "http:one", "1.0.0", Some(("exe", "one"))),
        );
        push_unique_lock_tool(
            &mut tools,
            lock_tool_with_options("dummy", "http:one", "1.0.0", Some(("exe", "two"))),
        );
        push_unique_lock_tool(
            &mut tools,
            lock_tool_with_options("dummy", "http:two", "1.0.0", Some(("exe", "one"))),
        );

        assert_eq!(tools.len(), 3);
        assert_eq!(tools[0].0.full(), "http:one");
        assert_eq!(tools[1].0.full(), "http:one");
        assert_eq!(tools[2].0.full(), "http:two");
        assert_ne!(tools[0].1.request.options(), tools[1].1.request.options());
    }

    #[tokio::test]
    async fn lock_request_options_preserve_config_and_inline_precedence() {
        let _config = crate::config::Config::get().await.unwrap();
        let plain_tool = ToolArg::from_str("vfox:tiny@latest").unwrap();
        let inline_tool = ToolArg::from_str("vfox:tiny[prerelease=true]@latest").unwrap();
        let mut config_options = ToolVersionOptions::default();
        config_options
            .opts
            .insert("prerelease".to_string(), toml::Value::Boolean(false));
        let configured_request =
            ToolRequest::new_opts(plain_tool.ba, "1.0.0", config_options, ToolSource::Argument)
                .unwrap();
        let configured_tv = ToolVersion::new(configured_request, "1.0.0".to_string());

        let plain_options = options_for_lock_request(&configured_tv, &plain_tool.tvr.unwrap());
        let inline_options = options_for_lock_request(&configured_tv, &inline_tool.tvr.unwrap());

        assert_eq!(
            plain_options.get_string("prerelease").as_deref(),
            Some("false")
        );
        assert_eq!(
            inline_options.get_string("prerelease").as_deref(),
            Some("true")
        );
    }

    #[test]
    fn symbolic_lock_versions_are_not_concrete_even_when_installed() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut ba = BackendArg::from("lock-symbolic-version-test");
        ba.installs_path = temp_dir.path().join("installs");
        std::fs::create_dir_all(ba.installs_path.join("edge")).unwrap();
        let backend = RemoteVersionsBackend::new(Arc::new(ba), vec![], None)
            .with_rolling_channel("edge", None);

        assert!(!is_known_concrete_lock_version(&backend, "edge"));
        assert!(!is_known_concrete_lock_version(&backend, "^1.2.3"));
        assert!(!is_known_concrete_lock_version(&backend, "1.2.x"));
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
        let configured_selectors = (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        );
        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, Some(&configured_selectors));
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
        let configured_selectors = (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        );
        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, Some(&configured_selectors));
        assert!(pruned.is_empty());
        assert_eq!(
            lockfile.all_platform_keys(),
            std::collections::BTreeSet::from(["linux-x64".to_string()])
        );
    }

    #[test]
    fn test_prune_stale_entries_without_selectors_keeps_existing_entries() {
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy();

        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, None);

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
        let configured_selectors = cmd.configured_tool_selectors(&tools);

        let pruned = cmd.prune_stale_entries_if_needed(&mut lockfile, Some(&configured_selectors));
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

    #[test]
    fn test_platform_regression_rejects_unresolved_version_bump() {
        let cmd = lock_cmd(&[]);
        let lockfile = lockfile_with_dummy();
        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![LockTaskResult {
            short: "dummy".to_string(),
            version: "2.0.0".to_string(),
            platform: "linux-x64".to_string(),
            status: LockTaskStatus::Unresolved,
        }];

        let errors = cmd.platform_regression_errors(&lockfile, &stale_versions, &results);

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("failed to resolve dummy@2.0.0 for linux-x64"));
        assert!(errors[0].contains("locked version(s) 1.0.0"));
    }

    #[test]
    fn test_platform_regression_rejects_empty_stub_version_bump() {
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy();
        let resolution = (
            "dummy".to_string(),
            "2.0.0".to_string(),
            "asdf:dummy".to_string(),
            Platform::parse("linux-x64").unwrap(),
            Ok(PlatformInfo::default()),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            false,
        );

        let applied = apply_lock_result(&mut lockfile, resolution).unwrap();
        let (status, error) = classify_lock_result(None, false, applied);
        assert!(!applied);
        assert_eq!(status, LockTaskStatus::Unresolved);
        assert!(error.is_none());

        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![LockTaskResult {
            short: "dummy".to_string(),
            version: "2.0.0".to_string(),
            platform: "linux-x64".to_string(),
            status,
        }];

        assert_eq!(
            cmd.platform_regression_errors(&lockfile, &stale_versions, &results)
                .len(),
            1
        );
    }

    #[test]
    fn test_platform_regression_allows_new_unsupported_platform() {
        let cmd = lock_cmd(&[]);
        let lockfile = lockfile_with_dummy();
        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![LockTaskResult {
            short: "dummy".to_string(),
            version: "2.0.0".to_string(),
            platform: "macos-arm64".to_string(),
            status: LockTaskStatus::Unresolved,
        }];

        let errors = cmd.platform_regression_errors(&lockfile, &stale_versions, &results);

        assert!(errors.is_empty());
    }

    #[test]
    fn test_platform_regression_allows_existing_current_platform() {
        let cmd = lock_cmd(&[]);
        let mut lockfile = lockfile_with_dummy();
        lockfile.set_platform_info(
            "dummy",
            "2.0.0",
            None,
            &BTreeMap::new(),
            "linux-x64",
            PlatformInfo {
                checksum: Some("sha256:current".to_string()),
                ..Default::default()
            },
        );
        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![LockTaskResult {
            short: "dummy".to_string(),
            version: "2.0.0".to_string(),
            platform: "linux-x64".to_string(),
            status: LockTaskStatus::Unresolved,
        }];

        let errors = cmd.platform_regression_errors(&lockfile, &stale_versions, &results);

        assert!(errors.is_empty());
    }

    #[test]
    fn test_platform_regression_rejects_provenance_failure() {
        let cmd = lock_cmd(&[]);
        let lockfile = lockfile_with_dummy();
        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![LockTaskResult {
            short: "dummy".to_string(),
            version: "2.0.0".to_string(),
            platform: "linux-x64".to_string(),
            status: LockTaskStatus::ProvenanceFailed,
        }];

        let errors = cmd.platform_regression_errors(&lockfile, &stale_versions, &results);

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("failed to resolve dummy@2.0.0 for linux-x64"));
        assert!(errors[0].contains("locked version(s) 1.0.0"));
    }

    #[test]
    fn test_platform_regression_allows_ambiguous_multi_version_bump() {
        let cmd = lock_cmd(&[]);
        let lockfile = lockfile_with_dummy();
        let stale_versions = BTreeMap::from([("dummy".to_string(), vec!["1.0.0".to_string()])]);
        let results = vec![
            LockTaskResult {
                short: "dummy".to_string(),
                version: "2.0.0".to_string(),
                platform: "linux-x64".to_string(),
                status: LockTaskStatus::Unresolved,
            },
            LockTaskResult {
                short: "dummy".to_string(),
                version: "3.0.0".to_string(),
                platform: "linux-x64".to_string(),
                status: LockTaskStatus::Updated,
            },
        ];

        let errors = cmd.platform_regression_errors(&lockfile, &stale_versions, &results);

        assert!(errors.is_empty());
    }
}
