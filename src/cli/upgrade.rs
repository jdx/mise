use std::sync::Arc;

use crate::backend::pipx::PIPXBackend;
use crate::cli::args::ToolArg;
use crate::config::{Config, Settings, config_file};
use crate::duration::parse_into_timestamp;
use crate::file::display_path;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{
    InstallOptions, ResolveOptions, ToolVersion, ToolsetBuilder,
    get_versions_needed_by_tracked_configs,
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
/// This will update mise.lock if it is enabled, see https://mise.jdx.dev/configuration/settings.html#lockfile
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

    /// Only upgrade to versions released before this date
    ///
    /// Supports absolute dates like "2024-06-01" and relative durations like "90d" or "1y".
    /// This can be useful for reproducibility or security purposes.
    ///
    /// This only affects fuzzy version matches like "20" or "latest".
    /// Explicitly pinned versions like "22.5.0" are not filtered.
    #[clap(long, verbatim_doc_comment)]
    before: Option<String>,

    /// Like --dry-run but exits with code 1 if there are outdated tools
    ///
    /// This is useful for scripts to check if tools need to be upgraded.
    #[clap(long, verbatim_doc_comment)]
    dry_run_code: bool,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Upgrade {
    fn is_dry_run(&self) -> bool {
        self.dry_run || self.dry_run_code
    }

    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        // Compute before_date once to ensure consistency when using relative durations
        let before_date = self.get_before_date()?;
        let opts = ResolveOptions {
            use_locked_version: false,
            latest_versions: true,
            before_date,
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
            },
            ..Default::default()
        };

        // Collect all tool requests for parallel installation
        let tool_requests: Vec<_> = outdated.iter().map(|o| o.tool_request.clone()).collect();

        // Install all tools in parallel
        let (successful_versions, install_error) =
            match ts.install_all_versions(config, tool_requests, &opts).await {
                Ok(versions) => (versions, eyre::Result::Ok(())),
                Err(e) => match e.downcast_ref::<crate::errors::Error>() {
                    Some(crate::errors::Error::InstallFailed {
                        successful_installations,
                        ..
                    }) => (successful_installations.clone(), eyre::Result::Err(e)),
                    _ => (vec![], eyre::Result::Err(e)),
                },
            };

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

        // Reset config after upgrades so tracked configs resolve with new versions
        *config = Config::reset().await?;

        // Rebuild symlinks BEFORE getting versions needed by tracked configs
        // This ensures "latest" symlinks point to the new versions, not the old ones
        runtime_symlinks::rebuild(config)
            .await
            .wrap_err("failed to rebuild runtime symlinks")?;

        // Get versions needed by tracked configs AFTER upgrade
        // This ensures we don't uninstall versions still needed by other projects
        let versions_needed_by_tracked = get_versions_needed_by_tracked_configs(config).await?;

        // Only uninstall old versions of tools that were successfully upgraded
        // and are not needed by any tracked config
        for (o, tv) in to_remove {
            if successful_versions
                .iter()
                .any(|v| v.ba() == o.tool_version.ba())
            {
                // Check if this version is still needed by another tracked config
                let version_key = (
                    o.tool_version.ba().short.to_string(),
                    o.tool_version.tv_pathname(),
                );
                if versions_needed_by_tracked.contains(&version_key) {
                    debug!(
                        "Keeping {}@{} because it's still needed by a tracked config",
                        o.name, tv
                    );
                    continue;
                }

                let pr = mpr.add(&format!("uninstall {}@{}", o.name, tv));
                if let Err(e) = self
                    .uninstall_old_version(config, &o.tool_version, pr.as_ref())
                    .await
                {
                    warn!("Failed to uninstall old version of {}: {}", o.name, e);
                }
            }
        }

        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(config, ts, &successful_versions).await?;

        if successful_versions.iter().any(|v| v.short() == "python") {
            PIPXBackend::reinstall_all(config)
                .await
                .unwrap_or_else(|err| {
                    warn!("failed to reinstall pipx tools: {err}");
                });
        }

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

    /// Get the before_date from CLI flag or settings
    fn get_before_date(&self) -> Result<Option<Timestamp>> {
        // CLI flag takes precedence over settings
        if let Some(before) = &self.before {
            return Ok(Some(parse_into_timestamp(before)?));
        }
        // Fall back to settings
        if let Some(before) = &Settings::get().install_before {
            return Ok(Some(parse_into_timestamp(before)?));
        }
        Ok(None)
    }
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
"#
);
