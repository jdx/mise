use color_eyre::eyre::Result;
use console::style;

use crate::config::Config;
use crate::output::Output;
use crate::plugins::unalias_plugin;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, alias = "remove", alias = "rm", after_long_help = AFTER_LONG_HELP)]
pub struct PluginsUninstall {
    /// Plugin(s) to remove
    #[clap(verbatim_doc_comment)]
    plugin: Vec<String>,

    /// Also remove the plugin's installs, downloads, and cache
    #[clap(long, short, verbatim_doc_comment)]
    purge: bool,

    /// Remove all plugins
    #[clap(long, short, verbatim_doc_comment, conflicts_with = "plugin")]
    all: bool,
}

impl PluginsUninstall {
    pub fn run(self, config: Config, _out: &mut Output) -> Result<()> {
        let mpr = MultiProgressReport::new(config.show_progress_bars());

        let plugins = match self.all {
            true => config.tools.keys().cloned().collect(),
            false => self.plugin.clone(),
        };

        for plugin_name in plugins {
            let plugin_name = unalias_plugin(&plugin_name);
            self.uninstall_one(&config, plugin_name, &mpr)?;
        }
        Ok(())
    }

    fn uninstall_one(
        &self,
        config: &Config,
        plugin_name: &str,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        match config.tools.get(plugin_name) {
            Some(plugin) if plugin.is_installed() => {
                let mut pr = mpr.add();
                plugin.decorate_progress_bar(&mut pr, None);
                plugin.uninstall(&pr)?;
                if self.purge {
                    plugin.purge(&pr)?;
                }
                pr.finish_with_message("uninstalled");
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
  $ <bold>rtx uninstall node</bold>
"#
);
