use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::backend::platform_target::PlatformTarget;
use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::{self, Lockfile, PlatformInfo};
use crate::platform::Platform;
use crate::toolset::Toolset;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{cli::args::ToolArg, config::Settings};
use console::style;
use eyre::Result;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Result type for lock task: (short_name, version, backend, platform, info, options)
type LockTaskResult = (
    String,
    String,
    String,
    Platform,
    Option<PlatformInfo>,
    BTreeMap<String, String>,
);

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
}

impl Lock {
    pub async fn run(self) -> Result<()> {
        let settings = Settings::get();
        let config = Config::get().await?;
        settings.ensure_experimental("lock")?;

        // Determine lockfile path based on config root
        let lockfile_path = self.get_lockfile_path(&config);

        // Determine target platforms
        let target_platforms = self.determine_target_platforms(&lockfile_path)?;

        miseprintln!(
            "{} Targeting {} platform(s): {}",
            style("→").cyan(),
            target_platforms.len(),
            target_platforms
                .iter()
                .map(|p| p.to_key())
                .collect::<Vec<_>>()
                .join(", ")
        );

        // Get toolset and resolve versions
        let ts = config.get_toolset().await?;
        let tools = self.get_tools_to_lock(&config, ts);

        if tools.is_empty() {
            miseprintln!("{} No tools configured to lock", style("!").yellow());
            return Ok(());
        }

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
            return Ok(());
        }

        // Process tools and update lockfile
        let mut lockfile = Lockfile::read(&lockfile_path)?;
        let results = self
            .process_tools(&settings, &tools, &target_platforms, &mut lockfile)
            .await?;

        // Save lockfile
        lockfile.write(&lockfile_path)?;

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
            style(display_path(&lockfile_path)).cyan()
        );

        Ok(())
    }

    fn get_lockfile_path(&self, config: &Config) -> PathBuf {
        // Get lockfile path from the first config file
        if let Some(config_path) = config.config_files.keys().next() {
            let (lockfile_path, _) = lockfile::lockfile_path_for_config(config_path);
            if self.local {
                // Replace mise.lock with mise.local.lock
                lockfile_path.with_file_name("mise.local.lock")
            } else {
                lockfile_path
            }
        } else {
            // Fallback to current dir
            let lockfile_name = if self.local {
                "mise.local.lock"
            } else {
                "mise.lock"
            };
            std::env::current_dir()
                .unwrap_or_default()
                .join(lockfile_name)
        }
    }

    fn determine_target_platforms(&self, lockfile_path: &PathBuf) -> Result<Vec<Platform>> {
        if !self.platform.is_empty() {
            // User specified platforms explicitly
            return Platform::parse_multiple(&self.platform);
        }

        // Default: 5 common platforms + existing in lockfile + current platform
        let mut platforms: BTreeSet<Platform> = Platform::common_platforms().into_iter().collect();
        platforms.insert(Platform::current());

        // Add any existing platforms from lockfile (only valid ones)
        if let Ok(lockfile) = Lockfile::read(lockfile_path) {
            for platform_key in lockfile.all_platform_keys() {
                if let Ok(p) = Platform::parse(&platform_key) {
                    // Skip invalid platforms (e.g., tool-specific qualifiers like "wait-for-gh-rate-limit")
                    if p.validate().is_ok() {
                        platforms.insert(p);
                    }
                }
            }
        }

        Ok(platforms.into_iter().collect())
    }

    fn get_tools_to_lock(
        &self,
        config: &Config,
        ts: &Toolset,
    ) -> Vec<(crate::cli::args::BackendArg, crate::toolset::ToolVersion)> {
        // Calculate target lockfile directory (same logic as get_lockfile_path)
        let target_lockfile_dir = config
            .config_files
            .keys()
            .next()
            .map(|p| {
                let (lockfile_path, _) = lockfile::lockfile_path_for_config(p);
                lockfile_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default()
            })
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Collect tools from config files that share the same lockfile directory
        let mut all_tools: Vec<_> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

        // Helper to get lockfile directory for a config path
        let get_lockfile_dir = |path: &std::path::Path| -> PathBuf {
            let (lockfile_path, _) = lockfile::lockfile_path_for_config(path);
            lockfile_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        };

        // First, get all tools from the resolved toolset (these are the "current" versions)
        // but only if they come from a config file with the same lockfile directory
        for (backend, tv) in ts.list_current_versions() {
            // Check if this tool's source shares the same lockfile directory
            if let Some(source_path) = tv.request.source().path()
                && get_lockfile_dir(source_path) != target_lockfile_dir
            {
                continue;
            }
            let key = (backend.ba().short.clone(), tv.version.clone());
            if seen.insert(key) {
                all_tools.push((backend.ba().as_ref().clone(), tv));
            }
        }

        // Then, iterate config files with the same lockfile directory to find tools that may have been overridden
        for (path, cf) in config.config_files.iter() {
            // Skip config files that don't share the same lockfile directory
            if get_lockfile_dir(path) != target_lockfile_dir {
                continue;
            }
            if let Ok(trs) = cf.to_tool_request_set() {
                for (ba, requests, _source) in trs.iter() {
                    for request in requests {
                        // Try to get a resolved version for this request
                        if let Ok(backend) = ba.backend() {
                            // Check if we already have this tool+version in toolset
                            if let Some(resolved_tv) = ts.versions.get(ba.as_ref()) {
                                for tv in &resolved_tv.versions {
                                    if tv.request.version() == request.version() {
                                        let key = (ba.short.clone(), tv.version.clone());
                                        if seen.insert(key) {
                                            all_tools.push((ba.as_ref().clone(), tv.clone()));
                                        }
                                    }
                                }
                            }
                            // For "latest" requests, find the highest installed version
                            if request.version() == "latest" {
                                let installed = backend.list_installed_versions();
                                if let Some(latest_version) = installed.iter().max_by(|a, b| {
                                    versions::Versioning::new(a).cmp(&versions::Versioning::new(b))
                                }) {
                                    let key = (ba.short.clone(), latest_version.clone());
                                    if seen.insert(key.clone()) {
                                        let tv = crate::toolset::ToolVersion::new(
                                            request.clone(),
                                            latest_version.clone(),
                                        );
                                        all_tools.push((ba.as_ref().clone(), tv));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if self.tool.is_empty() {
            // Lock all tools
            all_tools
        } else {
            // Filter to specified tools
            let specified: BTreeSet<String> =
                self.tool.iter().map(|t| t.ba.short.clone()).collect();
            all_tools
                .into_iter()
                .filter(|(ba, _)| specified.contains(&ba.short))
                .collect()
        }
    }

    fn show_dry_run(
        &self,
        tools: &[(crate::cli::args::BackendArg, crate::toolset::ToolVersion)],
        platforms: &[Platform],
    ) -> Result<()> {
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
        tools: &[(crate::cli::args::BackendArg, crate::toolset::ToolVersion)],
        platforms: &[Platform],
        lockfile: &mut Lockfile,
    ) -> Result<Vec<(String, String, bool)>> {
        let jobs = self.jobs.unwrap_or(settings.jobs);
        let semaphore = Arc::new(Semaphore::new(jobs));
        let mut jset: JoinSet<LockTaskResult> = JoinSet::new();
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

            jset.spawn(async move {
                let _permit = semaphore.acquire().await;
                let target = PlatformTarget::new(platform.clone());
                let backend = crate::backend::get(&ba);

                let (info, options) = if let Some(backend) = backend {
                    let options = backend.resolve_lockfile_options(&tv.request, &target);
                    match backend.resolve_lock_info(&tv, &target).await {
                        Ok(info) => (Some(info), options),
                        Err(e) => {
                            warn!(
                                "Failed to resolve {} for {}: {}",
                                ba.short,
                                platform.to_key(),
                                e
                            );
                            (None, options)
                        }
                    }
                } else {
                    warn!("Backend not found for {}", ba.short);
                    (None, BTreeMap::new())
                };

                (
                    ba.short.clone(),
                    tv.version.clone(),
                    ba.full(),
                    platform,
                    info,
                    options,
                )
            });
        }

        // Collect all results
        let mut completed = 0;
        while let Some(result) = jset.join_next().await {
            completed += 1;
            match result {
                Ok((short, version, backend, platform, info, options)) => {
                    let platform_key = platform.to_key();
                    pr.set_message(format!("{}@{} {}", short, version, platform_key));
                    pr.set_position(completed);
                    let ok = info.is_some();
                    if let Some(info) = info {
                        lockfile.set_platform_info(
                            &short,
                            &version,
                            Some(&backend),
                            &options,
                            &platform_key,
                            info,
                        );
                    }
                    results.push((short, platform_key, ok));
                }
                Err(e) => {
                    warn!("Task failed: {}", e);
                }
            }
        }

        pr.finish_with_message(format!("{} platform entries", total_tasks));
        Ok(results)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise lock</bold>                       # update lockfile for all common platforms
    $ <bold>mise lock node python</bold>           # update only node and python
    $ <bold>mise lock --platform linux-x64</bold>  # update only linux-x64 platform
    $ <bold>mise lock --dry-run</bold>             # show what would be updated
    $ <bold>mise lock --local</bold>               # update mise.local.lock for local configs
"#
);
