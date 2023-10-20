use std::collections::HashSet;
use std::sync::Arc;

use color_eyre::eyre::{eyre, Result};
use itertools::Itertools;
use rayon::prelude::*;
use rayon::ThreadPoolBuilder;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::runtime_symlinks;
use crate::shims;
use crate::tool::Tool;
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::ProgressReport;

/// Upgrades outdated tool versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct Upgrade {
    /// Tool(s) to upgrade
    /// e.g.: node@20 python@3.10
    /// If not specified, all current tools will be upgraded
    #[clap(value_name="TOOL@VERSION", value_parser = ToolArgParser, verbatim_doc_comment)]
    pub tool: Vec<ToolArg>,
}

impl Command for Upgrade {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let mut ts = ToolsetBuilder::new()
            .with_args(&self.tool)
            .build(&mut config)?;
        let tool_set = self
            .tool
            .iter()
            .map(|t| t.plugin.clone())
            .collect::<HashSet<_>>();
        ts.versions
            .retain(|_, tvl| tool_set.is_empty() || tool_set.contains(&tvl.plugin_name));
        let outdated = ts.list_outdated_versions(&config);
        if outdated.is_empty() {
            info!("All tools are up to date");
        } else {
            self.upgrade(&mut config, outdated)?;
        }

        Ok(())
    }
}

type OutputVec = Vec<(Arc<Tool>, ToolVersion, String)>;
type GroupedToolVersions = Vec<(Arc<Tool>, Vec<(ToolVersion, String)>)>;

impl Upgrade {
    fn upgrade(&self, config: &mut Config, outdated: OutputVec) -> Result<()> {
        let mpr = MultiProgressReport::new(config.show_progress_bars());
        ThreadPoolBuilder::new()
            .num_threads(config.settings.jobs)
            .build()?
            .install(|| -> Result<()> {
                self.install_new_versions(config, &mpr, outdated)?;

                let ts = ToolsetBuilder::new().with_args(&self.tool).build(config)?;
                shims::reshim(config, &ts).map_err(|err| eyre!("failed to reshim: {}", err))?;
                runtime_symlinks::rebuild(config)?;

                Ok(())
            })
    }

    fn install_new_versions(
        &self,
        config: &Config,
        mpr: &MultiProgressReport,
        outdated: OutputVec,
    ) -> Result<()> {
        let grouped_tool_versions: GroupedToolVersions = outdated
            .into_iter()
            .group_by(|(t, _, _)| t.clone())
            .into_iter()
            .map(|(t, tvs)| (t, tvs.map(|(_, tv, latest)| (tv, latest)).collect()))
            .collect();
        grouped_tool_versions
            .into_par_iter()
            .map(|(tool, versions)| {
                for (tv, latest) in versions {
                    let mut pr = mpr.add();
                    self.install_new_version(config, &tool, &tv, latest, &mut pr)?;
                    self.uninstall_old_version(config, &tool, &tv, &mut pr)?;
                }
                Ok(())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    fn install_new_version(
        &self,
        config: &Config,
        tool: &Tool,
        tv: &ToolVersion,
        latest: String,
        pr: &mut ProgressReport,
    ) -> Result<()> {
        let mut tv = tv.clone();
        tv.version = latest;
        tool.decorate_progress_bar(pr, Some(&tv));
        match tool.install_version(config, &tv, pr, false) {
            Ok(_) => Ok(()),
            Err(err) => {
                pr.error(err.to_string());
                Err(err.wrap_err(format!("failed to install {}", tv)))
            }
        }
    }

    fn uninstall_old_version(
        &self,
        config: &Config,
        tool: &Tool,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
    ) -> Result<()> {
        tool.decorate_progress_bar(pr, Some(tv));
        match tool.uninstall_version(config, tv, pr, false) {
            Ok(_) => {
                pr.finish();
                Ok(())
            }
            Err(err) => {
                pr.error(err.to_string());
                Err(err.wrap_err(format!("failed to uninstall {}", tv)))
            }
        }
    }
}
