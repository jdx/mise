use std::sync::Arc;

use crate::backend::pipx::PIPXBackend;
use crate::cli::args::ToolArg;
use crate::config::{Config, config_file};
use crate::file::display_path;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{InstallOptions, ResolveOptions, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{config, ui};
use console::Term;
use demand::DemandOption;
use eyre::{Context, Result, eyre};

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
    #[clap(value_name = "TOOL@VERSION", verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Just print what would be done, don't actually do it
    #[clap(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,

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

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Upgrade {
    pub async fn run(self) -> Result<()> {
        let mut config = Config::get().await?;
        let ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&config)
            .await?;
        let mut outdated = ts.list_outdated_versions(&config, self.bump).await;
        if self.interactive && !outdated.is_empty() {
            outdated = self.get_interactive_tool_set(&outdated)?;
        } else if !self.tool.is_empty() {
            outdated.retain(|o| {
                self.tool
                    .iter()
                    .any(|t| t.ba.as_ref() == o.tool_version.ba())
            });
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
            self.upgrade(&mut config, outdated).await?;
        }

        Ok(())
    }

    async fn upgrade(&self, config: &mut Arc<Config>, outdated: Vec<OutdatedInfo>) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(config)
            .await?;

        let config_file_updates = outdated
            .iter()
            .filter_map(|o| {
                if let (Some(path), Some(_bump)) = (o.source.path(), &o.bump) {
                    match config_file::parse(path) {
                        Ok(cf) => Some((o, cf)),
                        Err(e) => {
                            warn!("failed to parse {}: {e}", display_path(path));
                            None
                        }
                    }
                } else {
                    None
                }
            })
            .filter(|(o, cf)| {
                if let Ok(trs) = cf.to_tool_request_set() {
                    if let Some(versions) = trs.tools.get(o.tool_request.ba()) {
                        if versions.len() != 1 {
                            warn!("upgrading multiple versions with --bump is not yet supported");
                            return false;
                        }
                    }
                }
                true
            })
            .collect::<Vec<_>>();

        let to_remove = outdated
            .iter()
            .filter_map(|o| o.current.as_ref().map(|current| (o, current)))
            .collect::<Vec<_>>();

        if self.dry_run {
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
            return Ok(());
        }

        let opts = InstallOptions {
            // TODO: can we remove this without breaking e2e/cli/test_upgrade? it may be causing tools to re-install
            force: true,
            jobs: self.jobs,
            raw: self.raw,
            resolve_options: ResolveOptions {
                use_locked_version: false,
                latest_versions: true,
            },
            ..Default::default()
        };

        let mut successful_versions = Vec::new();
        let mut had_errors = false;

        for outdated_info in &outdated {
            let tool_request = outdated_info.tool_request.clone();
            let tool_name = outdated_info.name.clone();

            match ts
                .install_all_versions(config, vec![tool_request], &opts)
                .await
            {
                Ok(versions) => {
                    for version in versions {
                        successful_versions.push(version);
                    }
                }
                Err(e) => {
                    had_errors = true;
                    warn!("Failed to upgrade {}: {}", tool_name, e);
                }
            }
        }

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

        // Only uninstall old versions of tools that were successfully upgraded
        for (o, tv) in to_remove {
            if successful_versions
                .iter()
                .any(|v| v.ba() == o.tool_version.ba())
            {
                let pr = mpr.add(&format!("uninstall {}@{}", o.name, tv));
                if let Err(e) = self
                    .uninstall_old_version(config, &o.tool_version, &pr)
                    .await
                {
                    warn!("Failed to uninstall old version of {}: {}", o.name, e);
                }
            }
        }

        *config = Config::reset().await?;
        let ts = config.get_toolset().await?;
        config::rebuild_shims_and_runtime_symlinks(config, ts, &successful_versions).await?;

        if successful_versions.iter().any(|v| v.short() == "python") {
            PIPXBackend::reinstall_all(config)
                .await
                .unwrap_or_else(|err| {
                    warn!("failed to reinstall pipx tools: {err}");
                });
        }

        if had_errors {
            return Err(eyre!("Some tools failed to upgrade"));
        }

        Ok(())
    }

    async fn uninstall_old_version(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
    ) -> Result<()> {
        tv.backend()?
            .uninstall_version(config, tv, pr, self.dry_run)
            .await
            .wrap_err_with(|| format!("failed to uninstall {tv}"))?;
        pr.finish();
        Ok(())
    }

    fn get_interactive_tool_set(&self, outdated: &Vec<OutdatedInfo>) -> Result<Vec<OutdatedInfo>> {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let mut ms = demand::MultiSelect::new("mise upgrade")
            .description("Select tools to upgrade")
            .filterable(true);
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

    # Show a multiselect menu to choose which tools to upgrade
    $ <bold>mise upgrade --interactive</bold>
"#
);
