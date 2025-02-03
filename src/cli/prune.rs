use std::collections::BTreeMap;
use std::sync::Arc;

use crate::backend::Backend;
use crate::cli::args::{BackendArg, ToolArg};
use crate::config::tracking::Tracker;
use crate::config::{Config, SETTINGS};
use crate::toolset::{ToolVersion, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;
use console::style;
use eyre::Result;

use super::trust::Trust;

/// Delete unused versions of tools
///
/// mise tracks which config files have been used in ~/.local/state/mise/tracked-configs
/// Versions which are no longer the latest specified in any of those configs are deleted.
/// Versions installed only with environment variables `MISE_<PLUGIN>_VERSION` will be deleted,
/// as will versions only referenced on the command line `mise exec <PLUGIN>@<VERSION>`.
///
/// You can list prunable tools with `mise ls --prunable`
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Prune {
    /// Prune only these tools
    #[clap()]
    pub installed_tool: Option<Vec<ToolArg>>,

    /// Do not actually delete anything
    #[clap(long, short = 'n')]
    pub dry_run: bool,

    /// Prune only tracked and trusted configuration links that point to non-existent configurations
    #[clap(long)]
    pub configs: bool,

    /// Prune only unused versions of tools
    #[clap(long)]
    pub tools: bool,
}

impl Prune {
    pub fn run(self) -> Result<()> {
        if self.configs || !self.tools {
            self.prune_configs()?;
        }
        if self.tools || !self.configs {
            let backends = self
                .installed_tool
                .as_ref()
                .map(|it| it.iter().map(|ta| &ta.ba).collect());
            prune(backends.unwrap_or_default(), self.dry_run)?;
        }
        Ok(())
    }

    fn prune_configs(&self) -> Result<()> {
        if self.dry_run {
            info!("pruned configuration links {}", style("[dryrun]").bold());
        } else {
            Tracker::clean()?;
            Trust::clean()?;
            info!("pruned configuration links");
        }
        Ok(())
    }
}

pub fn prunable_tools(tools: Vec<&BackendArg>) -> Result<Vec<(Arc<dyn Backend>, ToolVersion)>> {
    let config = Config::try_get()?;
    let ts = ToolsetBuilder::new().build(&config)?;
    let mut to_delete = ts
        .list_installed_versions()?
        .into_iter()
        .map(|(p, tv)| ((tv.ba().short.to_string(), tv.tv_pathname()), (p, tv)))
        .collect::<BTreeMap<(String, String), (Arc<dyn Backend>, ToolVersion)>>();

    if !tools.is_empty() {
        to_delete.retain(|_, (_, tv)| tools.contains(&tv.ba()));
    }

    for cf in config.get_tracked_config_files()?.values() {
        let mut ts = Toolset::from(cf.to_tool_request_set()?);
        ts.resolve()?;
        for (_, tv) in ts.list_current_versions() {
            to_delete.remove(&(tv.ba().short.to_string(), tv.tv_pathname()));
        }
    }

    Ok(to_delete.into_values().collect())
}

pub fn prune(tools: Vec<&BackendArg>, dry_run: bool) -> Result<()> {
    let to_delete = prunable_tools(tools)?;
    delete(dry_run, to_delete)
}

fn delete(dry_run: bool, to_delete: Vec<(Arc<dyn Backend>, ToolVersion)>) -> Result<()> {
    let mpr = MultiProgressReport::get();
    for (p, tv) in to_delete {
        let mut prefix = tv.style();
        if dry_run {
            prefix = format!("{} {} ", prefix, style("[dryrun]").bold());
        }
        let pr = mpr.add(&prefix);
        if dry_run || SETTINGS.yes || prompt::confirm_with_all(format!("remove {} ?", &tv))? {
            p.uninstall_version(&tv, &pr, dry_run)?;
            pr.finish();
        }
    }
    Ok(())
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise prune --dry-run</bold>
    rm -rf ~/.local/share/mise/versions/node/20.0.0
    rm -rf ~/.local/share/mise/versions/node/20.0.1
"#
);
