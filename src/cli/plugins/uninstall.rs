use color_eyre::eyre::Result;
use console::style;

use crate::cli::command::Command;
use crate::config::Config;
use crate::output::Output;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
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
        match config.tools.get(plugin_name) {
            Some(plugin) if plugin.is_installed() => {
                let mut pr = mpr.add();
                plugin.decorate_progress_bar(&mut pr, None);
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

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>rtx uninstall nodejs</bold>
"#
);
