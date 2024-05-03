use std::collections::BTreeMap;
use std::sync::Arc;

use console::style;
use eyre::Result;

use crate::cli::args::ForgeArg;
use crate::config::tracking::Tracker;
use crate::config::{Config, Settings};
use crate::forge::Forge;
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;

use super::trust::Trust;

/// Delete unused versions of tools
///
/// mise tracks which config files have been used in ~/.local/share/mise/tracked_config_files
/// Versions which are no longer the latest specified in any of those configs are deleted.
/// Versions installed only with environment variables (`MISE_<PLUGIN>_VERSION`) will be deleted,
/// as will versions only referenced on the command line (`mise exec <PLUGIN>@<VERSION>`).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Prune {
    /// Prune only versions from this plugin(s)
    #[clap()]
    pub plugin: Option<Vec<ForgeArg>>,

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
            self.prune_tools()?;
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

    fn prune_tools(&self) -> Result<()> {
        let config = Config::try_get()?;
        let ts = ToolsetBuilder::new().build(&config)?;
        let mut to_delete = ts
            .list_installed_versions()?
            .into_iter()
            .map(|(p, tv)| (tv.to_string(), (p, tv)))
            .collect::<BTreeMap<String, (Arc<dyn Forge>, ToolVersion)>>();

        if let Some(forges) = &self.plugin {
            to_delete.retain(|_, (_, tv)| forges.contains(&tv.forge));
        }

        for cf in config.get_tracked_config_files()?.values() {
            let mut ts = cf.to_toolset()?.clone();
            ts.resolve();
            for (_, tv) in ts.list_current_versions() {
                to_delete.remove(&tv.to_string());
            }
        }

        self.delete(to_delete.into_values().collect())
    }

    fn delete(&self, to_delete: Vec<(Arc<dyn Forge>, ToolVersion)>) -> Result<()> {
        let settings = Settings::try_get()?;
        let mpr = MultiProgressReport::get();
        for (p, tv) in to_delete {
            let mut prefix = tv.style();
            if self.dry_run {
                prefix = format!("{} {} ", prefix, style("[dryrun]").bold());
            }
            let pr = mpr.add(&prefix);
            if self.dry_run || settings.yes || prompt::confirm(&format!("remove {} ?", &tv))? {
                p.uninstall_version(&tv, pr.as_ref(), self.dry_run)?;
                pr.finish();
            }
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise prune --dry-run</bold>
    rm -rf ~/.local/share/mise/versions/node/20.0.0
    rm -rf ~/.local/share/mise/versions/node/20.0.1
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_prune() {
        assert_cli!("prune", "--dry-run");
        assert_cli!("prune", "tiny");
        assert_cli!("prune");
        assert_cli!("install");
    }
}
