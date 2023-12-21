use std::collections::BTreeMap;
use std::sync::Arc;

use console::style;
use eyre::Result;

use crate::config::{Config, Settings};

use crate::plugins::{Plugin, PluginName};
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::prompt;

/// Delete unused versions of tools
///
/// rtx tracks which config files have been used in ~/.local/share/rtx/tracked_config_files
/// Versions which are no longer the latest specified in any of those configs are deleted.
/// Versions installed only with environment variables (`RTX_<PLUGIN>_VERSION`) will be deleted,
/// as will versions only referenced on the command line (`rtx exec <PLUGIN>@<VERSION>`).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct Prune {
    /// Prune only versions from these plugins
    #[clap()]
    pub plugin: Option<Vec<PluginName>>,

    /// Do not actually delete anything
    #[clap(long, short = 'n')]
    pub dry_run: bool,
}

impl Prune {
    pub fn run(self, config: &Config) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config)?;
        let mut to_delete = ts
            .list_installed_versions(config)?
            .into_iter()
            .map(|(p, tv)| (tv.to_string(), (p, tv)))
            .collect::<BTreeMap<String, (Arc<dyn Plugin>, ToolVersion)>>();

        if let Some(plugins) = &self.plugin {
            to_delete.retain(|_, (_, tv)| plugins.contains(&tv.plugin_name));
        }

        for cf in config.get_tracked_config_files()?.values() {
            let mut ts = cf.to_toolset().clone();
            ts.resolve(config);
            for (_, tv) in ts.list_current_versions() {
                to_delete.remove(&tv.to_string());
            }
        }

        self.delete(to_delete.into_values().collect())
    }

    fn delete(&self, to_delete: Vec<(Arc<dyn Plugin>, ToolVersion)>) -> Result<()> {
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
  $ <bold>rtx prune --dry-run</bold>
  rm -rf ~/.local/share/rtx/versions/node/20.0.0
  rm -rf ~/.local/share/rtx/versions/node/20.0.1
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
