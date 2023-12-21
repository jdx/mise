use console::style;
use demand::DemandOption;
use std::collections::HashSet;
use std::sync::Arc;

use eyre::Result;
use eyre::WrapErr;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;

use crate::plugins::Plugin;
use crate::runtime_symlinks;
use crate::shims;
use crate::toolset::{InstallOptions, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;

/// Upgrades outdated tool versions
#[derive(Debug, clap::Args)]
#[clap(visible_alias = "up", verbatim_doc_comment)]
pub struct Upgrade {
    /// Tool(s) to upgrade
    /// e.g.: node@20 python@3.10
    /// If not specified, all current tools will be upgraded
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser, verbatim_doc_comment)]
    tool: Vec<ToolArg>,

    /// Just print what would be done, don't actually do it
    #[clap(long, short = 'n', verbatim_doc_comment)]
    dry_run: bool,

    /// Number of jobs to run in parallel
    /// [default: 4]
    #[clap(long, short, env = "RTX_JOBS", verbatim_doc_comment)]
    jobs: Option<usize>,

    /// Display multiselect menu to choose which tools to upgrade
    #[clap(long, short, verbatim_doc_comment, conflicts_with = "tool")]
    interactive: bool,

    /// Directly pipe stdin/stdout/stderr from plugin to user
    /// Sets --jobs=1
    #[clap(long, overrides_with = "jobs")]
    raw: bool,
}

impl Upgrade {
    pub fn run(self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;
        let mut outdated = ts.list_outdated_versions();
        if self.interactive && !outdated.is_empty() {
            let tvs = self.get_interactive_tool_set(&outdated)?;
            outdated.retain(|(_, tv, _)| tvs.contains(tv));
        } else {
            let tool_set = self
                .tool
                .iter()
                .map(|t| t.plugin.clone())
                .collect::<HashSet<_>>();
            outdated.retain(|(p, _, _)| tool_set.is_empty() || tool_set.contains(p.name()));
        }
        if outdated.is_empty() {
            info!("All tools are up to date");
        } else {
            self.upgrade(config, outdated)?;
        }

        Ok(())
    }

    fn upgrade(&self, config: &Config, outdated: OutputVec) -> Result<()> {
        let mpr = MultiProgressReport::get();
        let mut ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;

        let new_versions = outdated
            .iter()
            .map(|(_, tv, latest)| {
                let mut tv = tv.clone();
                tv.version = latest.clone();
                tv
            })
            .collect();

        let to_remove = outdated
            .into_iter()
            .filter(|(tool, tv, _)| tool.is_version_installed(tv))
            .map(|(tool, tv, _)| (tool, tv))
            .collect::<Vec<_>>();

        if self.dry_run {
            for (_, tv) in &to_remove {
                info!("Would uninstall {tv}");
            }
            for tv in &new_versions {
                info!("Would install {tv}");
            }
            return Ok(());
        }
        let opts = InstallOptions {
            force: false,
            jobs: self.jobs,
            raw: self.raw,
            latest_versions: true,
        };
        ts.install_versions(config, new_versions, &mpr, &opts)?;
        for (tool, tv) in to_remove {
            let prefix = format!("{}", style(&tv).cyan().for_stderr());
            let pr = mpr.add(&prefix);
            self.uninstall_old_version(tool.clone(), &tv, pr.as_ref())?;
        }

        let ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;
        shims::reshim(config, &ts).wrap_err("failed to reshim")?;
        runtime_symlinks::rebuild(config)?;
        Ok(())
    }

    fn uninstall_old_version(
        &self,
        tool: Arc<dyn Plugin>,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
    ) -> Result<()> {
        tool.uninstall_version(tv, pr, self.dry_run)
            .wrap_err_with(|| format!("failed to uninstall {tv}"))?;
        pr.finish();
        Ok(())
    }

    fn get_interactive_tool_set(&self, outdated: &OutputVec) -> Result<HashSet<ToolVersion>> {
        let mut ms = demand::MultiSelect::new("rtx upgrade")
            .description("Select tools to upgrade")
            .filterable(true)
            .min(1);
        for (_, tv, latest) in outdated {
            let label = if &tv.version == latest {
                tv.to_string()
            } else {
                format!("{tv} -> {latest}")
            };
            ms = ms.option(DemandOption::new(tv).label(&label));
        }
        Ok(ms.run()?.into_iter().cloned().collect())
    }
}

type OutputVec = Vec<(Arc<dyn Plugin>, ToolVersion, String)>;

#[cfg(test)]
pub mod tests {
    use crate::test::reset_config;
    use crate::{dirs, file};

    #[test]
    fn test_upgrade() {
        reset_config();
        file::rename(
            dirs::INSTALLS.join("tiny").join("3.1.0"),
            dirs::INSTALLS.join("tiny").join("3.0.0"),
        )
        .unwrap();
        assert_cli_snapshot!("upgrade", "--dry-run");
        assert_cli_snapshot!("upgrade");
        assert!(dirs::INSTALLS.join("tiny").join("3.1.0").exists());
    }
}
