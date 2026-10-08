use eyre::Result;

use crate::backend::unalias_backend;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend, plugins};

/// Remove an installed plugin
///
/// Installed tool versions are kept unless you pass `--purge`, which also
/// removes a tool plugin's installs, downloads, and cache. `--purge` does not
/// remove the tools of a backend plugin (`<plugin>:<tool>`); uninstall those
/// first with `mise uninstall --all <plugin>:<tool>`.
#[derive(Debug, usage_rs::Args)]
#[usage(
    verbatim_doc_comment,
    visible_aliases = ["remove", "rm"],
    example(
        r###"mise plugins uninstall my-tool"###,
        help = "Remove the plugin and keep the versions it installed"
    ),
    example(
        r###"mise plugins uninstall --purge my-tool"###,
        help = "Also delete the versions it installed"
    )
)]
pub(super) struct PluginsUninstall {
    /// Plugins to remove
    #[usage(verbatim_doc_comment)]
    plugin: Vec<String>,

    /// Remove all plugins
    #[usage(long, short, verbatim_doc_comment, conflicts = "plugin")]
    all: bool,

    /// Also remove a tool plugin's installs, downloads, and cache
    #[usage(long, short, verbatim_doc_comment)]
    purge: bool,
}

impl PluginsUninstall {
    pub(super) async fn run(self) -> Result<()> {
        let mpr = MultiProgressReport::get();

        let plugins = match self.all {
            true => install_state::list_plugins().keys().cloned().collect(),
            false => self.plugin.clone(),
        };

        for plugin_name in plugins {
            let plugin_name = unalias_backend(&plugin_name);
            let plugin_name = plugin_name.as_ref();
            self.uninstall_one(plugin_name, &mpr).await?;
        }
        Ok(())
    }

    async fn uninstall_one(&self, plugin_name: &str, mpr: &MultiProgressReport) -> Result<()> {
        if let Ok(plugin) = plugins::get(plugin_name) {
            if plugin.is_installed() {
                let prefix = format!("plugin:{}", style::eblue(&plugin.name()));
                let pr = mpr.add(&prefix);
                plugin.uninstall(pr.as_ref()).await?;
                if self.purge {
                    let backend = backend::get(&plugin_name.into()).unwrap();
                    backend.purge(pr.as_ref())?;
                }
                pr.finish_with_message("uninstalled".into());
            } else {
                warn!("{} is not installed", style::eblue(plugin_name));
            }
        } else {
            warn!("{} is not installed", style::eblue(plugin_name));
        }
        Ok(())
    }
}
