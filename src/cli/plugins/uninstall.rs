use eyre::Result;

use crate::forge::unalias_forge;
use crate::plugins;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;

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
    pub fn run(self) -> Result<()> {
        let mpr = MultiProgressReport::get();

        let plugins = match self.all {
            true => plugins::list()
                .into_iter()
                .map(|p| p.id().to_string())
                .collect(),
            false => self.plugin.clone(),
        };

        for plugin_name in plugins {
            let plugin_name = unalias_forge(&plugin_name);
            self.uninstall_one(plugin_name, &mpr)?;
        }
        Ok(())
    }

    fn uninstall_one(&self, plugin_name: &str, mpr: &MultiProgressReport) -> Result<()> {
        match plugins::get(plugin_name) {
            plugin if plugin.is_installed() => {
                let prefix = format!("plugin:{}", style::eblue(&plugin.id()));
                let pr = mpr.add(&prefix);
                plugin.uninstall(pr.as_ref())?;
                if self.purge {
                    plugin.purge(pr.as_ref())?;
                }
                pr.finish_with_message("uninstalled".into());
            }
            _ => warn!("{} is not installed", style::eblue(plugin_name)),
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise uninstall node</bold>
"#
);
