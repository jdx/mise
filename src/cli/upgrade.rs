use crate::cli::args::ToolArg;
use crate::config::{config_file, Config};
use crate::file::display_path;
use crate::toolset::{install_state, InstallOptions, OutdatedInfo, ResolveOptions, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{lockfile, runtime_symlinks, shims, ui};
use demand::DemandOption;
use eyre::{Context, Result};

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
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let ts = ToolsetBuilder::new().with_args(&self.tool).build(&config)?;
        let mut outdated = ts.list_outdated_versions(self.bump);
        if self.interactive && !outdated.is_empty() {
            outdated = self.get_interactive_tool_set(&outdated)?;
        } else if !self.tool.is_empty() {
            outdated.retain(|o| self.tool.iter().any(|t| &t.ba == o.tool_version.ba()));
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
            self.upgrade(&config, outdated)?;
        }

        Ok(())
    }

    fn upgrade(&self, config: &Config, outdated: Vec<OutdatedInfo>) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;

        let config_file_updates = outdated
            .iter()
            .filter_map(|o| {
                if let (Some(path), Some(bump)) = (o.source.path(), &o.bump) {
                    match config_file::parse(path) {
                        Ok(cf) => Some((o, bump.clone(), cf)),
                        Err(e) => {
                            warn!("failed to parse {}: {e}", display_path(path));
                            None
                        }
                    }
                } else {
                    None
                }
            })
            .filter(|(o, _bump, cf)| {
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
                info!("Would uninstall {}@{}", o.name, current);
            }
            for o in &outdated {
                info!("Would install {}@{}", o.name, o.latest);
            }
            for (o, bump, cf) in &config_file_updates {
                info!(
                    "Would bump {}@{} in {}",
                    o.name,
                    bump,
                    display_path(cf.get_path())
                );
            }
            return Ok(());
        }
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            resolve_options: ResolveOptions {
                use_locked_version: false,
                latest_versions: true,
            },
        };
        let new_versions = outdated.iter().map(|o| o.tool_request.clone()).collect();
        let versions = ts.install_versions(config, new_versions, &mpr, &opts)?;

        for (o, bump, mut cf) in config_file_updates {
            cf.replace_versions(o.tool_request.ba(), &[(bump, o.tool_request.options())])?;
            cf.save()?;
        }

        for (o, tv) in to_remove {
            let pr = mpr.add(&format!("Uninstalling {}@{}", o.name, tv));
            self.uninstall_old_version(&o.tool_version, pr.as_ref())?;
        }

        install_state::reset();
        lockfile::update_lockfiles(&versions).wrap_err("failed to update lockfiles")?;
        let ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;
        shims::reshim(&ts, false).wrap_err("failed to reshim")?;
        runtime_symlinks::rebuild(config)?;
        Ok(())
    }

    fn uninstall_old_version(&self, tv: &ToolVersion, pr: &dyn SingleReport) -> Result<()> {
        tv.backend()?
            .uninstall_version(tv, pr, self.dry_run)
            .wrap_err_with(|| format!("failed to uninstall {tv}"))?;
        pr.finish();
        Ok(())
    }

    fn get_interactive_tool_set(&self, outdated: &Vec<OutdatedInfo>) -> Result<Vec<OutdatedInfo>> {
        ui::ctrlc::show_cursor_after_ctrl_c();
        let mut ms = demand::MultiSelect::new("mise upgrade")
            .description("Select tools to upgrade")
            .filterable(true)
            .min(1);
        for out in outdated {
            ms = ms.option(DemandOption::new(out.clone()));
        }
        Ok(ms.run()?.into_iter().collect())
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

#[cfg(test)]
pub mod tests {
    use crate::dirs;
    use crate::test::{change_installed_version, reset};

    #[test]
    fn test_upgrade() {
        reset();
        change_installed_version("tiny", "3.1.0", "3.0.0");
        assert_cli_snapshot!("upgrade", "--dry-run");
        assert_cli_snapshot!("upgrade");
        assert!(dirs::INSTALLS.join("tiny").join("3.1.0").exists());
    }

    #[test]
    fn test_upgrade_bump() {
        reset();
        change_installed_version("tiny", "3.1.0", "3.0.0");
        assert_cli_snapshot!("upgrade", "--dry-run", "--bump");
        assert_cli_snapshot!("upgrade");
        assert!(dirs::INSTALLS.join("tiny").join("3.1.0").exists());
    }
}
