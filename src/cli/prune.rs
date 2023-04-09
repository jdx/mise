use color_eyre::eyre::Result;
use console::style;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::tool::Tool;
use crate::toolset::{ToolVersion, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;

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
    pub plugins: Option<Vec<PluginName>>,

    /// Do not actually delete anything
    #[clap(long)]
    pub dry_run: bool,
}

impl Command for Prune {
    fn run(self, mut config: Config, _out: &mut Output) -> Result<()> {
        let ts = ToolsetBuilder::new().build(&mut config)?;
        let mut to_delete = ts
            .list_installed_versions(&config)?
            .into_iter()
            .map(|(p, tv)| (tv.to_string(), (p, tv)))
            .collect::<BTreeMap<String, (Arc<Tool>, ToolVersion)>>();

        if let Some(plugins) = &self.plugins {
            to_delete.retain(|_, (_, tv)| plugins.contains(&tv.plugin_name));
        }

        for cf in config.get_tracked_config_files()?.values() {
            let mut ts = cf.to_toolset().clone();
            ts.resolve(&mut config);
            for (_, tv) in ts.list_current_versions(&config) {
                to_delete.remove(&tv.to_string());
            }
        }

        self.delete(&mut config, to_delete.into_values().collect())
    }
}

impl Prune {
    fn delete(&self, config: &mut Config, to_delete: Vec<(Arc<Tool>, ToolVersion)>) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);
        for (p, tv) in to_delete {
            let mut pr = mpr.add();
            p.decorate_progress_bar(&mut pr, Some(&tv));
            if self.dry_run {
                pr.set_prefix(format!("{} {} ", pr.prefix(), style("[dryrun]").bold()));
            }
            p.uninstall_version(config, &tv, &pr, self.dry_run)?;
            pr.finish();
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx prune --dry-run</bold>
  rm -rf ~/.local/share/rtx/versions/nodejs/18.0.0
  rm -rf ~/.local/share/rtx/versions/nodejs/18.0.1
"#
);

#[cfg(test)]
mod tests {
    use crate::assert_cli;

    #[test]
    fn test_prune() {
        assert_cli!("prune", "--dry-run");
        assert_cli!("prune", "tiny");
        assert_cli!("prune");
        assert_cli!("install");
    }
}
