use color_eyre::eyre::Result;
use console::style;

use indoc::formatdoc;
use once_cell::sync::Lazy;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;

use crate::ui::multi_progress_report::MultiProgressReport;

/// Removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP.as_str())]
pub struct PluginsUninstall {
    /// Plugin(s) to remove
    #[clap(required = true, verbatim_doc_comment)]
    pub plugin: Vec<String>,
}

impl Command for PluginsUninstall {
    fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mpr = MultiProgressReport::new(config.settings.verbose);

        for plugin_name in &self.plugin {
            self.uninstall_one(&config, plugin_name, &mpr)?;
        }
        Ok(())
    }
}

impl PluginsUninstall {
    fn uninstall_one(
        &self,
        config: &Config,
        plugin_name: &String,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        let plugin = config.plugins.get(plugin_name);
        match plugin {
            Some(plugin) if plugin.is_installed() => {
                let mut pr = mpr.add();
                plugin.decorate_progress_bar(&mut pr);
                plugin.uninstall(&pr)?;
                pr.finish_with_message("uninstalled".into());
            }
            _ => mpr.suspend(|| {
                warn!(
                    "{} is not installed",
                    style(plugin_name).cyan().for_stderr()
                );
            }),
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: Lazy<String> = Lazy::new(|| {
    formatdoc! {r#"
    {}
      $ rtx uninstall nodejs
    "#, style("Examples:").bold().underlined()}
});
