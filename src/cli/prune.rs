use color_eyre::eyre::Result;
use console::style;
use indexmap::IndexMap;
use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::plugins::PluginName;
use crate::runtimes::RuntimeVersion;
use crate::toolset::ToolsetBuilder;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Delete unused versions of tools
/// rtx tracks which config files have been used in ~/.local/share/rtx/tracked_config_files
/// Versions which are no longer the latest specified in any of those configs are deleted.
/// Versions installed only with environment variables (`RTX_<PLUGIN>_VERSION`) will be deleted,
/// as will versions only referenced on the command line (`rtx exec <PLUGIN>@<VERSION>`).
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP.as_str())]
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
            .map(|rtv| (rtv.to_string(), rtv))
            .collect::<IndexMap<String, RuntimeVersion>>();

        if let Some(plugins) = &self.plugins {
            to_delete.retain(|_, rtv| plugins.contains(&rtv.plugin.name));
        }

        for cf in config.get_tracked_config_files()?.values() {
            let mut ts = cf.to_toolset().clone();
            ts.resolve(&config);
            for rtv in ts.list_current_versions() {
                to_delete.remove(&rtv.to_string());
            }
        }

        self.delete(&config, to_delete.into_values().collect())
    }
}

impl Prune {
    fn delete(&self, config: &Config, to_delete: Vec<RuntimeVersion>) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);
        for rtv in to_delete {
            let mut pr = mpr.add();
            rtv.decorate_progress_bar(&mut pr);
            if self.dry_run {
                pr.set_prefix(format!("{} {} ", pr.prefix(), style("[dryrun]").bold()));
            }
            rtv.uninstall(&config.settings, &pr, self.dry_run)?;
            pr.finish();
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx prune --dry-run
      rm -rf ~/.local/share/rtx/versions/nodejs/18.0.0
      rm -rf ~/.local/share/rtx/versions/nodejs/18.0.1
    "#, style("Examples:").bold().underlined()}
});

#[cfg(test)]
mod tests {
    use crate::{assert_cli, cmd};

    #[test]
    fn test_prune() {
        assert_cli!("prune", "--dry-run");
        assert_cli!("prune", "tiny");
        assert_cli!("prune");
        cmd!("git", "checkout", "../data").run().unwrap();
    }
}
