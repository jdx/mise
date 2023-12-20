use console::style;
use eyre::Result;

use crate::config::Config;

use crate::plugins::unalias_plugin;
use crate::ui::multi_progress_report::MultiProgressReport;

/// Removes a plugin
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, visible_aliases = ["remove", "rm"], after_long_help = AFTER_LONG_HELP)]
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
    pub fn run(self, config: &Config) -> Result<()> {
        let mpr = MultiProgressReport::new();

        let plugins = match self.all {
            true => config
                .list_plugins()
                .into_iter()
                .map(|p| p.name().to_string())
                .collect(),
            false => self.plugin.clone(),
        };

        for plugin_name in plugins {
            let plugin_name = unalias_plugin(&plugin_name);
            self.uninstall_one(config, plugin_name, &mpr)?;
        }
        Ok(())
    }

    fn uninstall_one(
        &self,
        config: &Config,
        plugin_name: &str,
        mpr: &MultiProgressReport,
    ) -> Result<()> {
        match config.get_or_create_plugin(plugin_name) {
            plugin if plugin.is_installed() => {
                let prefix = format!("plugin:{}", style(&plugin.name()).blue().for_stderr());
                let pr = mpr.add(&prefix);
                plugin.uninstall(pr.as_ref())?;
                if self.purge {
                    plugin.purge(pr.as_ref())?;
                }
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
  $ <bold>rtx uninstall node</bold>
"#
);
