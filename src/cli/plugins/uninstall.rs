use eyre::Result;

use crate::backend::unalias_backend;
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
            let plugin_name = unalias_backend(&plugin_name);
            self.uninstall_one(plugin_name, &mpr)?;
        }
        Ok(())
    }

    fn uninstall_one(&self, plugin_name: &str, mpr: &MultiProgressReport) -> Result<()> {
        let backend = plugins::get(plugin_name);
        if let Some(plugin) = backend.plugin() {
            let prefix = format!("plugin:{}", style::eblue(&backend.name()));
            let pr = mpr.add(&prefix);
            backend.plugin().unwrap().uninstall(pr.as_ref())?;
            if self.purge {
                backend.purge(pr.as_ref())?;
            }
            pr.finish_with_message("uninstalled".into());
        } else {
            warn!("{} is not installed", style::eblue(plugin_name));
        }
        Ok(())
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    $ <bold>mise uninstall node</bold>
"#
);
