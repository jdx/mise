use std::collections::HashSet;
use std::sync::Arc;

use crate::backend::pipx::PIPXBackend;
use crate::cli::args::{BackendArg, ToolArg};
use crate::config::{Config, config_file};
use crate::errors::split_install_result;
use crate::file::display_path;
use crate::install_before::resolve_cli_minimum_release_age;
use crate::semver::split_version_prefix;
use crate::toolset::is_outdated_version;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::outdated_info::prefixed_latest_query;
use crate::toolset::{
    ConfigScope, InstallOptions, ResolveOptions, ToolSource, ToolVersion, ToolsetBuilder,
    get_versions_needed_by_tracked_configs_excluding_locks,
};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{config, exit, runtime_symlinks, ui};
use console::Term;
use demand::DemandOption;
use eyre::{Context, Result, eyre};
use jiff::Timestamp;

/// Upgrades outdated tools
///
/// By default, this keeps the range specified in mise.toml. So if you have node@20 set, it will
/// upgrade to the latest 20.x.x version available. See the `--bump` flag to use the latest version
/// and bump the version in mise.toml.
///
/// This will update mise.lock if it is enabled, see https://mise.en.dev/configuration/settings.html#lockfile
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "up", verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Upgrade {
    /// Tool(s) to upgrade
    /// e.g.: node@20 python@3.10
    /// If not specified, all current tools will be upgraded
    #[clap(value_name = "INSTALLED_TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Display multiselect menu to choose which tools to upgrade
    #[clap(long, short, verbatim_doc_comment, conflicts_with = "tool")]
    interactive: bool,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "MISE_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Upgrades to the latest version available, bumping the version in mise.toml
    ///
    /// For example, if you have `node = "20.0.0"` in your mise.toml but 22.1.0 is the latest available,
    /// this will install 22.1.0 and set `node = "22.1.0"` in your config.
    ///
    /// It keeps the same precision as what was there before, so if you instead had `node = "20"`, it
    /// would change your config to `node = "22"`.
    #[clap(long, short = 'l', verbatim_doc_comment)]
    bump: bool,

    /// Just print what would be done, don't actually do it
    #[clap(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,

    /// Tool(s) to exclude from upgrading
    /// e.g.: go python
    #[clap(long, short = 'x', value_name = "INSTALLED_TOOL", verbatim_doc_comment)]
    exclude: Vec<ToolArg>,

    /// Like --dry-run but exits with code 1 if there are outdated tools
    ///
    /// This is useful for scripts to check if tools need to be upgraded.
    #[clap(long, verbatim_doc_comment)]
    dry_run_code: bool,

    /// Upgrade all tools, including installed-but-inactive tools not present in the current config
    #[clap(long, verbatim_doc_comment, conflicts_with = "local")]
    inactive: bool,

    /// Only upgrade tools defined in local config files
    ///
    /// This will only upgrade tools that are defined in project-local mise.toml and
    /// will skip tools defined in the global config (~/.config/mise/config.toml).
    #[clap(long, verbatim_doc_comment)]
    local: bool,

    /// Only upgrade to versions released before this date or older than this duration
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    /// This can be useful for reproducibility or security purposes.
    ///
    /// This only affects fuzzy version matches like "20" or "latest".
    /// Explicitly pinned versions like "22.5.0" are not filtered.
    #[clap(long, alias = "before", verbatim_doc_comment)]
    minimum_release_age: Option<String>,

    /// Connect backend install command stdin/stdout/stderr directly to the terminal
    /// Implies --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Upgrade {
    fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    fn scope(&self) -> ConfigScope {
        if self.local {
            ConfigScope::LocalOnly
        } else {
            ConfigScope::All
        }
    }

    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_scope(self.scope())
            .build(&config)
            .await?;
        // Compute before_date once to ensure consistency when using relative durations
        let before_date = self.get_before_date()?;
        let opts = ResolveOptions {
            use_locked_version: false,
            latest_versions: true,
            before_date,
            before_date_from_default: false,
            offline: false,
            refresh_remote_versions: false,
            inactive: self.inactive,
        };
        // Filter tools to check before doing expensive version lookups
        let filter_tools = if !self.interactive && !self.tool.is_empty() {
            Some(self.tool.as_slice())
        } else {
            None
        };
        let exclude_tools = if !self.exclude.is_empty() {
            Some(self.exclude.as_slice())
        } else {
            None
        };
        let mut outdated = ts
            .list_outdated_versions_filtered(&config, self.bump, &opts, filter_tools, exclude_tools)
            .await;
        self.warn_if_newer_versions_hidden_by_minimum_release_age(
            &config,
            &ts,
            &opts,
            filter_tools,
            exclude_tools,
        )
        .await;
        if self.interactive && !outdated.is_empty() {
            outdated = self.get_interactive_tool_set(&outdated)?;
        }
        if outdated.is_empty() {
            info!("All tools are up to date");
            if !self.bump {
                hint!(
                    "outdated_bump",
                    r#"By default, `mise upgrade` only upgrades versions that match your config. Use `mise upgrade --bump` to upgrade all new versions."#,
                    ""
                );
            }
        } else {
            self.upgrade(&mut config, outdated, before_date).await?;
        }

        Ok(())
    }

    async fn upgrade(
        &self,
        config: &mut Arc<Config>,
        outdated: Vec<OutdatedInfo>,
        before_date: Option<Timestamp>,
    ) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .with_scope(self.scope())
            .build(config)
            .await?;

        let mut outdated_with_config_files: Vec<(&OutdatedInfo, Arc<dyn config_file::ConfigFile>)> =
            vec![];
        for o in outdated.iter() {
            if let (Some(path), Some(_bump)) = (o.source.path(), &o.bump) {
                match config_file::parse(path).await {
                    Ok(cf) => outdated_with_config_files.push((o, cf)),
                    Err(e) => warn!("failed to parse {}: {e}", display_path(path)),
                }
            }
        }
        let config_file_updates = outdated_with_config_files
            .iter()
            .filter(|(o, cf)| {
                if let Ok(trs) = cf.to_tool_request_set()
                    && let Some(versions) = trs.tools.get(o.tool_request.ba())
                    && versions.len() != 1
                {
                    warn!("upgrading multiple versions with --bump is not yet supported");
                    return false;
                }
                true
            })
            .collect::<Vec<_>>();

        // Determine which old versions should be uninstalled after upgrade
        // Skip uninstall when current == latest (channel-based versions that update in-place)
        let to_remove: Vec<_> = outdated
            .iter()
            .filter_map(|o| {
                o.current.as_ref().and_then(|current| {
                    // Skip if current and latest version strings are identical
                    // This handles channels like "nightly", "stable", "beta" that update in-place
                    if &o.latest == current {
                        return None;
                    }
                    Some((o, current.clone()))
                })
            })
            .collect();

        if self.is_dry_run() {
            for (o, current) in &to_remove {
                miseprintln!("Would uninstall {}@{}", o.name, current);
            }
            for o in &outdated {
                miseprintln!("Would install {}@{}", o.name, o.latest);
            }
            for (o, cf) in &config_file_updates {
                miseprintln!(
                    "Would bump {}@{} in {}",
                    o.name,
                    o.tool_request.version(),
                    display_path(cf.get_path())
                );
            }
            if !self.bump {
                use crate::toolset::outdated_info::compute_config_bumps;
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
                let bumps = compute_config_bumps(config, &refs);
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
            if self.dry_run_code {
                exit::exit(1);
            }
            return Ok(());
        }

        let opts = InstallOptions {
            reason: "upgrade".to_string(),
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            resolve_options: ResolveOptions {
                use_locked_version: false,
                latest_versions: true,
                before_date,
                before_date_from_default: false,
                offline: false,
                refresh_remote_versions: false,
                inactive: self.inactive,
            },
            locked: false,
            ..Default::default()
        };

        // Collect all tool requests for parallel installation
        let tool_requests: Vec<_> = outdated.iter().map(|o| o.tool_request.clone()).collect();

        // Install all tools in parallel
        let (mut successful_versions, install_error) =
            split_install_result(ts.install_all_versions(config, tool_requests, &opts).await);

        // Only update config files for tools that were successfully installed
        for (o, cf) in config_file_updates {
            if successful_versions
                .iter()
                .any(|v| v.ba() == o.tool_version.ba())
            {
                if let Err(e) =
                    cf.replace_versions(o.tool_request.ba(), vec![o.tool_request.clone()])
                {
                    return Err(eyre!("Failed to update config for {}: {}", o.name, e));
                }

                if let Err(e) = cf.save() {
                    return Err(eyre!("Failed to save config for {}: {}", o.name, e));
                }
            }
        }

        // When a specific version is provided via CLI (e.g., `mise upgrade tiny@3.0.1`),
        // update the config file prefix if the new version doesn't match the current specifier.
        // Skip if --bump was used since it already handles config updates.
        if !self.bump {
            use crate::toolset::outdated_info::{apply_config_bumps, compute_config_bumps};
            let tool_versions: Vec<(String, String)> = self
                .tool
                .iter()
                .filter_map(|t| {
                    t.tvr.as_ref().and_then(|tvr| {
                        let name = t.ba.short.clone();
                        // Only process tools that were successfully installed
                        if successful_versions.iter().any(|v| v.ba().short == name) {
                            Some((name, tvr.version()))
                        } else {
                            None
                        }
                    })
                })
                .collect();
            let refs: Vec<(&str, &str)> = tool_versions
                .iter()
                .map(|(n, v)| (n.as_str(), v.as_str()))
                .collect();
            let bumps = compute_config_bumps(config, &refs);
            apply_config_bumps(config, &bumps)?;
        }

        // Reset config after upgrades so tracked configs resolve with new versions
        *config = Config::reset().await?;

        // Rebuild symlinks BEFORE getting versions needed by tracked configs
        // This ensures "latest" symlinks point to the new versions, not the old ones
        let ts = config.get_toolset().await?;
        runtime_symlinks::rebuild_for_toolset(config, ts)
            .await
            .wrap_err("failed to rebuild runtime symlinks")?;

        // Get versions needed by tracked configs AFTER upgrade. Preserve lockfile pins
        // from other projects, but ignore stale pre-upgrade locks for configs we just
        // upgraded so their old versions can still be removed.
        let successful_backends: HashSet<_> = successful_versions
            .iter()
            .flat_map(|v| {
                [
                    v.ba().short.clone(),
                    v.ba().tool_name.clone(),
                    v.ba().full(),
                    v.ba().full_without_opts(),
                ]
            })
            .collect();
        let mut upgraded_config_paths: HashSet<_> = outdated
            .iter()
            .filter(|o| backend_matches(&successful_backends, o.tool_version.ba()))
            .filter_map(|o| o.source.path().map(|path| path.to_path_buf()))
            .collect();
        for tvl in ts.versions.values() {
            if backend_matches(&successful_backends, &tvl.backend)
                && let Some(path) = tvl.source.path()
            {
                upgraded_config_paths.insert(path.to_path_buf());
            }
        }
        for (path, cf) in config.config_files.iter() {
            let Ok(trs) = cf.to_tool_request_set() else {
                continue;
            };
            if trs
                .tools
                .keys()
                .any(|ba| backend_matches(&successful_backends, ba))
            {
                upgraded_config_paths.insert(path.clone());
            }
        }
        let versions_needed_by_tracked = get_versions_needed_by_tracked_configs_excluding_locks(
            config,
            true,
            false,
            &upgraded_config_paths,
        )
        .await?;

        // Only uninstall old versions of tools that were successfully upgraded
        // and are not needed by any tracked config
        for (o, old_version) in to_remove {
            if successful_versions
                .iter()
                .any(|v| v.ba() == o.tool_version.ba())
            {
                // Build a ToolVersion that targets the actual installed old version
                // (e.g., "1.0.0"), not the resolved latest (e.g., "2.0.0").
                // When minimum_release_age forces a remote lookup for "latest",
                // the toolset resolves to the remote version, and tv_pathname()
                // on the toolset version would give the wrong key.
                let old_tv = ToolVersion::new(o.tool_version.request.clone(), old_version.clone());
                let version_key = (old_tv.ba().short.to_string(), old_tv.tv_pathname());
                if versions_needed_by_tracked.contains(&version_key) {
                    debug!(
                        "Keeping {}@{} because it's still needed by a tracked config",
                        o.name, old_version
                    );
                    continue;
                }

                let pr = mpr.add(&format!("uninstall {}@{}", o.name, old_version));
                if let Err(e) = self
                    .uninstall_old_version(config, &old_tv, pr.as_ref())
                    .await
                {
                    warn!("Failed to uninstall old version of {}: {}", o.name, e);
                }
            }
        }

        mpr.finish_progress();

        // Fix up sources and requests for lockfile update - CLI args produce
        // ToolSource::Argument but lockfile update only processes ToolSource::MiseToml.
        // Also copy the config's request version (e.g., "latest") so the lockfile update
        // correctly replaces the old entry instead of adding a duplicate.
        for tv in &mut successful_versions {
            if matches!(tv.request.source(), ToolSource::Argument)
                && let Some(tvl) = ts.versions.get(tv.ba())
                && matches!(&tvl.source, ToolSource::MiseToml(_))
            {
                // Use the config's request (preserves version specifier like "latest")
                // but keep the resolved version from the upgrade
                if let Some(config_tv) = tvl.versions.first() {
                    tv.request = config_tv.request.clone();
                } else {
                    tv.request.set_source(tvl.source.clone());
                }
            }
        }

        config::rebuild_shims_and_runtime_symlinks(
            config,
            ts,
            &successful_versions,
            crate::lockfile::LockfileUpdateMode::AllowLocked,
        )
        .await?;

        if successful_versions.iter().any(|v| v.short() == "python") {
            PIPXBackend::reinstall_all(config)
                .await
                .unwrap_or_else(|err| {
                    warn!("failed to reinstall pipx tools: {err}");
                });
        }

        mpr.finish_progress();
        Self::print_summary(&outdated, &successful_versions)?;

        install_error
    }

    async fn uninstall_old_version(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<()> {
        tv.backend()?
            .uninstall_version(config, tv, pr, self.dry_run)
            .await
            .wrap_err_with(|| format!("failed to uninstall {tv}"))?;
        pr.finish();
        Ok(())
    }

    fn print_summary(outdated: &[OutdatedInfo], successful_versions: &[ToolVersion]) -> Result<()> {
        let upgraded: Vec<_> = outdated
            .iter()
            .filter(|o| {
                successful_versions
                    .iter()
                    .any(|v| v.ba() == o.tool_version.ba() && v.version == o.latest)
            })
            .collect();
        if !upgraded.is_empty() {
            let s = if upgraded.len() == 1 { "" } else { "s" };
            miseprintln!("\nUpgraded {} tool{}:", upgraded.len(), s);
            for o in &upgraded {
                let from = o.current.as_deref().unwrap_or("(none)");
                miseprintln!("  {} {} → {}", o.name, from, o.latest);
            }
        }
        Ok(())
    }

    fn get_interactive_tool_set(&self, outdated: &Vec<OutdatedInfo>) -> Result<Vec<OutdatedInfo>> {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let theme = crate::ui::theme::get_theme();
        let mut ms = demand::MultiSelect::new("mise upgrade")
            .description("Select tools to upgrade")
            .filterable(true)
            .theme(&theme);
        for out in outdated {
            ms = ms.option(DemandOption::new(out.clone()));
        }
        match ms.run() {
            Ok(selected) => Ok(selected.into_iter().collect()),
            Err(e) => {
                Term::stderr().show_cursor()?;
                Err(eyre!(e))
            }
        }
    }

    /// Get the minimum_release_age cutoff from the CLI --minimum-release-age flag only.
    /// Per-tool and global setting fallbacks are handled in ToolRequest::resolve.
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        resolve_cli_minimum_release_age(self.minimum_release_age.as_deref())
    }

    async fn warn_if_newer_versions_hidden_by_minimum_release_age(
        &self,
        config: &Arc<Config>,
        ts: &crate::toolset::Toolset,
        opts: &ResolveOptions,
        filter_tools: Option<&[ToolArg]>,
        exclude_tools: Option<&[ToolArg]>,
    ) {
        let list_versions = if opts.inactive {
            match ts.list_all_versions(config).await {
                Ok(v) => v,
                Err(err) => {
                    warn!("Failed to list all versions: {err:#}");
                    vec![]
                }
            }
        } else {
            ts.list_current_versions()
        };
        let mut warned = HashSet::new();
        for (_, tv) in list_versions {
            if let Some(exclude) = exclude_tools
                && exclude.iter().any(|t| t.ba.as_ref() == tv.ba())
            {
                continue;
            }
            if let Some(tools) = filter_tools
                && !tools.iter().any(|t| t.ba.as_ref() == tv.ba())
            {
                continue;
            }
            let warning_key = format!("{}@{}", tv.ba().short, tv.request.version());
            if !warned.insert(warning_key) {
                continue;
            }
            let mut opts_with_effective_before_date = opts.clone();
            if let Err(err) = opts_with_effective_before_date
                .apply_before_date_for_tool(tv.ba(), tv.request.options().minimum_release_age())
            {
                warn!(
                    "Error resolving minimum_release_age for {}: {err:#}",
                    tv.ba()
                );
                continue;
            }
            if opts_with_effective_before_date.before_date.is_none() {
                continue;
            }
            let eligible_latest = self
                .latest_for_upgrade(config, &tv, &opts_with_effective_before_date)
                .await;
            let eligible_latest = match eligible_latest {
                Ok(latest) => latest,
                Err(err) => {
                    warn!("Error getting latest version for {}: {err:#}", tv.ba());
                    continue;
                }
            };
            let baseline_latest = match self.baseline_latest_for_upgrade(config, &tv, opts).await {
                Ok(latest) => latest,
                Err(err) => {
                    warn!("Error getting latest version for {}: {err:#}", tv.ba());
                    continue;
                }
            };
            match (eligible_latest, baseline_latest) {
                (Some(eligible), Some(baseline)) if is_outdated_version(&eligible, &baseline) => {
                    warn!(
                        "newer {} release {baseline} ignored by minimum_release_age; latest eligible release is {eligible}",
                        tv.ba().short
                    );
                }
                (None, Some(baseline)) => {
                    warn!(
                        "newer {} release {baseline} ignored by minimum_release_age; no eligible release found",
                        tv.ba().short
                    );
                }
                _ => {}
            }
        }
    }

    async fn latest_for_upgrade(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        opts: &ResolveOptions,
    ) -> Result<Option<String>> {
        let backend = tv.backend()?;
        if self.bump || (opts.inactive && tv.request.source() == &ToolSource::Unknown) {
            let (prefix, prefix_version) = split_version_prefix(&tv.request.version());
            backend
                .latest_version(
                    config,
                    prefixed_latest_query(&prefix, &prefix_version),
                    opts.before_date,
                )
                .await
        } else {
            tv.latest_version_with_opts(config, opts).await.map(Some)
        }
    }

    async fn baseline_latest_for_upgrade(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        opts: &ResolveOptions,
    ) -> Result<Option<String>> {
        let backend = tv.backend()?;
        let query = if self.bump || (opts.inactive && tv.request.source() == &ToolSource::Unknown) {
            let (prefix, prefix_version) = split_version_prefix(&tv.request.version());
            prefixed_latest_query(&prefix, &prefix_version)
        } else {
            Some(tv.request.version())
        };
        backend.latest_version_unfiltered(config, query).await
    }
}

fn backend_matches(backends: &HashSet<String>, ba: &BackendArg) -> bool {
    backends.contains(&ba.short)
        || backends.contains(&ba.tool_name)
        || backends.contains(&ba.full())
        || backends.contains(&ba.full_without_opts())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Upgrades node to the latest version matching the range in mise.toml
    $ <bold>mise upgrade node</bold>

    # Upgrades node to the latest version and bumps the version in mise.toml
    $ <bold>mise upgrade node --bump</bold>

    # Upgrades all tools to the latest versions
    $ <bold>mise upgrade</bold>

    # Upgrades all tools to the latest versions and bumps the version in mise.toml
    $ <bold>mise upgrade --bump</bold>

    # Just print what would be done, don't actually do it
    $ <bold>mise upgrade --dry-run</bold>

    # Upgrades node and python to the latest versions
    $ <bold>mise upgrade node python</bold>

    # Upgrade all tools except go
    $ <bold>mise upgrade --exclude go</bold>

    # Show a multiselect menu to choose which tools to upgrade
    $ <bold>mise upgrade --interactive</bold>

    # Only upgrade tools defined in local mise.toml, not global ones
    $ <bold>mise upgrade --local</bold>
"#
);
