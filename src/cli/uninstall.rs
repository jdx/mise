use std::sync::Arc;

use console::style;
use eyre::{bail, eyre, Result};
use itertools::Itertools;
use rayon::prelude::*;

use crate::backend::Backend;
use crate::cli::args::ToolArg;
use crate::config::Config;
use crate::toolset::{ToolRequest, ToolSource, ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::{config, dirs, file};

/// Removes installed tool versions
///
/// This only removes the installed version, it does not modify mise.toml.
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["remove", "rm"], after_long_help = AFTER_LONG_HELP)]
pub struct Uninstall {
    /// Tool(s) to remove
    #[clap(value_name = "INSTALLED_TOOL@VERSION", required_unless_present = "all")]
    installed_tool: Vec<ToolArg>,

    /// Delete all installed versions
    #[clap(long, short)]
    all: bool,

    /// Do not actually delete anything
    #[clap(long, short = 'n')]
    dry_run: bool,
}

impl Uninstall {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let tool_versions = if self.installed_tool.is_empty() && self.all {
            self.get_all_tool_versions(&config)?
        } else {
            self.get_requested_tool_versions()?
        };
        let tool_versions = tool_versions
            .into_iter()
            .unique_by(|(_, tv)| (tv.request.ba().short.clone(), tv.version.clone()))
            .collect::<Vec<_>>();
        if !self.all && tool_versions.len() > self.installed_tool.len() {
            bail!("multiple tools specified, use --all to uninstall all versions");
        }

        let mpr = MultiProgressReport::get();
        for (plugin, tv) in tool_versions {
            if !plugin.is_version_installed(&tv, true) {
                warn!("{} is not installed", tv.style());
                continue;
            }

            let pr = mpr.add(&tv.style());
            if let Err(err) = plugin.uninstall_version(&tv, pr.as_ref(), self.dry_run) {
                error!("{err}");
                return Err(eyre!(err).wrap_err(format!("failed to uninstall {tv}")));
            }
            if self.dry_run {
                pr.finish_with_message("uninstalled (dry-run)".into());
            } else {
                pr.finish_with_message("uninstalled".into());
            }
        }

        file::touch_dir(&dirs::DATA)?;
        config::rebuild_shims_and_runtime_symlinks(&[])?;

        Ok(())
    }

    fn get_all_tool_versions(
        &self,
        config: &Config,
    ) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let ts = ToolsetBuilder::new().build(config)?;
        let tool_versions = ts
            .list_installed_versions()?
            .into_iter()
            .collect::<Vec<_>>();
        Ok(tool_versions)
    }
    fn get_requested_tool_versions(&self) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
        let runtimes = ToolArg::double_tool_condition(&self.installed_tool)?;
        let tool_versions = runtimes
            .into_par_iter()
            .map(|ta| {
                let backend = ta.ba.backend()?;
                let query = ta.tvr.as_ref().map(|tvr| tvr.version()).unwrap_or_default();
                let installed_versions = backend.list_installed_versions()?;
                let exact_match = installed_versions.iter().find(|v| v == &&query);
                let matches = match exact_match {
                    Some(m) => vec![m],
                    None => installed_versions
                        .iter()
                        .filter(|v| v.starts_with(&query))
                        .collect_vec(),
                };
                let mut tvs = matches
                    .into_iter()
                    .map(|v| {
                        let tvr = ToolRequest::new(backend.ba().clone(), v, ToolSource::Unknown)?;
                        let tv = ToolVersion::new(tvr, v.into());
                        Ok((backend.clone(), tv))
                    })
                    .collect::<Result<Vec<_>>>()?;
                if let Some(tvr) = &ta.tvr {
                    tvs.push((backend.clone(), tvr.resolve(&Default::default())?));
                }
                if tvs.is_empty() {
                    warn!(
                        "no versions found for {}",
                        style(&backend).blue().for_stderr()
                    );
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

    # will uninstall specific version
    $ <bold>mise uninstall node@18.0.0</bold>

    # will uninstall the current node version (if only one version is installed)
    $ <bold>mise uninstall node</bold>

    # will uninstall all installed versions of node
    $ <bold>mise uninstall --all node@18.0.0</bold> # will uninstall all node versions
"#
);
