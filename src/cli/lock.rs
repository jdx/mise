use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::config::Config;
use crate::file::display_path;
use crate::lockfile::{self, LockResolutionResult, Lockfile};
use crate::platform::Platform;
use crate::toolset::Toolset;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{cli::args::ToolArg, config::Settings};
use console::style;
use eyre::Result;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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

        let ts = config.get_toolset().await?;

        // Two-pass approach: first non-local (mise.lock), then local (mise.local.lock).
        // With --local, only the local pass runs.
        let passes: &[bool] = if self.local { &[true] } else { &[false, true] };
        let mut any_tools = false;

        for &is_local in passes {
            let lockfile_path = self.get_lockfile_path(&config, is_local);
            let tools = self.get_tools_to_lock(&config, ts, is_local);

            if tools.is_empty() {
                continue;
            }
            any_tools = true;

            let target_platforms = self.determine_target_platforms(&lockfile_path)?;

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

            if self.dry_run {
                self.show_dry_run(&tools, &target_platforms)?;
                continue;
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
        }

        if !any_tools {
            miseprintln!("{} No tools configured to lock", style("!").yellow());
        }

        Ok(())
    }

    /// Get the lockfile path for either the local or non-local pass.
    fn get_lockfile_path(&self, config: &Config, is_local: bool) -> PathBuf {
        let lockfile_name = if is_local {
            "mise.local.lock"
        } else {
            "mise.lock"
        };
        if let Some(config_path) = config.config_files.keys().next() {
            let (lockfile_path, _) = lockfile::lockfile_path_for_config(config_path);
            lockfile_path.with_file_name(lockfile_name)
        } else {
            std::env::current_dir()
                .unwrap_or_default()
                .join(lockfile_name)
        }
    }

    fn determine_target_platforms(&self, lockfile_path: &Path) -> Result<Vec<Platform>> {
        if !self.platform.is_empty() {
            // User specified platforms explicitly
            return Platform::parse_multiple(&self.platform);
        }

        Ok(lockfile::determine_target_platforms(lockfile_path))
    }

    /// Collect tools that belong to a given lockfile pass (local or non-local).
    /// Only includes tools whose source config matches the requested locality.
    fn get_tools_to_lock(
        &self,
        config: &Config,
        ts: &Toolset,
        is_local: bool,
    ) -> Vec<(crate::cli::args::BackendArg, crate::toolset::ToolVersion)> {
        // Determine the reference lockfile directory from the first config file.
        // Used to filter out tools from unrelated directories (e.g. global config).
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

        let get_lockfile_dir = |path: &std::path::Path| -> PathBuf {
            let (lockfile_path, _) = lockfile::lockfile_path_for_config(path);
            lockfile_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        };

        let mut all_tools: Vec<_> = Vec::new();
        let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

        // First pass: tools from the resolved toolset whose source matches this locality
        for (backend, tv) in ts.list_current_versions() {
            if let Some(source_path) = tv.request.source().path() {
                if get_lockfile_dir(source_path) != target_lockfile_dir {
                    continue;
                }
                let (_, source_is_local) = lockfile::lockfile_path_for_config(source_path);
                if source_is_local != is_local {
                    continue;
                }
            } else {
                // Tools without a source path (env vars, CLI args) go to non-local only
                if is_local {
                    continue;
                }
            }
            let key = (backend.ba().short.clone(), tv.version.clone());
            if seen.insert(key) {
                all_tools.push((backend.ba().as_ref().clone(), tv));
            }
        }

        // Second pass: iterate config files matching this locality to catch
        // tools that were overridden by a higher-priority config
        for (path, cf) in config.config_files.iter() {
            if get_lockfile_dir(path) != target_lockfile_dir {
                continue;
            }
            let (_, config_is_local) = lockfile::lockfile_path_for_config(path);
            if config_is_local != is_local {
                continue;
            }
            if let Ok(trs) = cf.to_tool_request_set() {
                for (ba, requests, _source) in trs.iter() {
                    for request in requests {
                        if let Ok(backend) = ba.backend() {
                            // Check if the resolved toolset has a matching version
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
                            // For "latest" or prefix requests not yet matched, find the
                            // best installed version (handles overridden tools)
                            if request.version() == "latest" {
                                let installed = backend.list_installed_versions();
                                if let Some(latest_version) = installed.iter().max_by(|a, b| {
                                    versions::Versioning::new(a).cmp(&versions::Versioning::new(b))
                                }) {
                                    let key = (ba.short.clone(), latest_version.clone());
                                    if seen.insert(key) {
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
            all_tools
        } else {
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
        let mut completed = 0;
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
                    lockfile::apply_lock_result(lockfile, resolution);
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
