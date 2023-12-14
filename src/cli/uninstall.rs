use console::style;
use eyre::{Result, WrapErr};
use itertools::Itertools;
use rayon::prelude::*;
use std::sync::Arc;

use crate::cli::args::tool::{ToolArg, ToolArgParser};
use crate::config::Config;

use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, ToolVersionRequest, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{runtime_symlinks, shims};

/// Removes runtime versions
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// Tool(s) to remove
    #[clap(value_name = "TOOL@VERSION", value_parser = ToolArgParser, required_unless_present = "all")]
    installed_tool: Vec<ToolArg>,

    /// Delete all installed versions
    #[clap(long, short)]
    all: bool,

    /// Do not actually delete anything
    #[clap(long, short = 'n')]
    dry_run: bool,
}

impl Uninstall {
    pub fn run(self, config: &Config) -> Result<()> {
        let tool_versions = if self.installed_tool.is_empty() && self.all {
            self.get_all_tool_versions(config)?
        } else {
            self.get_requested_tool_versions(config)?
        };
        let tool_versions = tool_versions
            .into_iter()
            .unique()
            .sorted()
            .collect::<Vec<_>>();
        if !self.all && tool_versions.len() > 1 {
            bail!("multiple tools specified, use --all to uninstall all versions");
        }

        let mpr = MultiProgressReport::new();
        for (plugin, tv) in tool_versions {
            if !plugin.is_version_installed(&tv) {
                warn!("{} is not installed", style(&tv).cyan().for_stderr());
                continue;
            }

            let prefix = format!("{}", style(&tv).cyan().for_stderr());
            let pr = mpr.add(&prefix);
            if let Err(err) = plugin.uninstall_version(&tv, pr.as_ref(), self.dry_run) {
                pr.error(err.to_string());
                return Err(eyre!(err).wrap_err(format!("failed to uninstall {tv}")));
            }
            if self.dry_run {
                pr.finish_with_message("uninstalled (dry-run)".into());
            } else {
                pr.finish_with_message("uninstalled".into());
            }
        }

        let ts = ToolsetBuilder::new().build(config)?;
        shims::reshim(config, &ts).wrap_err("failed to reshim")?;
        runtime_symlinks::rebuild(config)?;

        Ok(())
    }

    fn get_all_tool_versions(
        &self,
        config: &Config,
    ) -> Result<Vec<(Arc<dyn Plugin>, ToolVersion)>> {
        let ts = ToolsetBuilder::new().build(config)?;
        let tool_versions = ts
            .list_installed_versions(config)?
            .into_iter()
            .collect::<Vec<_>>();
        Ok(tool_versions)
    }
    fn get_requested_tool_versions(
        &self,
        config: &Config,
    ) -> Result<Vec<(Arc<dyn Plugin>, ToolVersion)>> {
        let runtimes = ToolArg::double_tool_condition(&self.installed_tool);
        let tool_versions = runtimes
            .into_par_iter()
            .map(|a| {
                let tool = config.get_or_create_plugin(&a.plugin);
                let query = a.tvr.as_ref().map(|tvr| tvr.version()).unwrap_or_default();
                let mut tvs = tool
                    .list_installed_versions()?
                    .into_iter()
                    .filter(|v| v.starts_with(&query))
                    .map(|v| {
                        let tvr = ToolVersionRequest::new(tool.name().into(), &v);
                        let tv = ToolVersion::new(tool.clone(), tvr, Default::default(), v);
                        (tool.clone(), tv)
                    })
                    .collect::<Vec<_>>();
                if let Some(tvr) = &a.tvr {
                    tvs.push((
                        tool.clone(),
                        tvr.resolve(tool.clone(), Default::default(), false)?,
                    ));
                }
                if tvs.is_empty() {
                    warn!("no versions found for {}", style(&tool).cyan().for_stderr());
                }
                Ok(tvs)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        Ok(tool_versions)
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx uninstall node@18.0.0</bold> # will uninstall specific version
  $ <bold>rtx uninstall node</bold>        # will uninstall current node version
  $ <bold>rtx uninstall --all node@18.0.0</bold> # will uninstall all node versions
"#
);
