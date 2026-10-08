use std::sync::Arc;

use eyre::Result;

use crate::backend::backend_type::BackendType;
use crate::backend::{Backend, unalias_backend};
use crate::plugins::PluginType;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend, plugins};

/// Remove an installed plugin
///
/// Tool installations are retained by default. Pass `--purge` to also remove
/// installs, downloads, and cache associated with the selected plugins.
#[derive(Debug, usage_rs::Args)]
#[usage(verbatim_doc_comment, visible_aliases = ["remove", "rm"], example(r###"mise plugins uninstall my-tool"###))]
pub(super) struct PluginsUninstall {
    /// Plugin(s) to remove
    #[usage(verbatim_doc_comment)]
    plugin: Vec<String>,

    /// Remove all plugins
    #[usage(long, short, verbatim_doc_comment, conflicts = "plugin")]
    all: bool,

    /// Also remove the plugin's installs, downloads, and cache
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
                // Resolve the backends first: once the plugin is gone, its tools
                // no longer resolve to a backend.
                let backends = match self.purge {
                    true => backends_to_purge(plugin_name, plugin.get_plugin_type())?,
                    false => vec![],
                };
                plugin.uninstall(pr.as_ref()).await?;
                for backend in backends {
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

/// The backends whose installs, downloads, and cache belong to a plugin.
///
/// A backend plugin provides any number of `plugin:tool` tools, so every
/// installed tool it backs is purged. Other plugins provide the tool of the
/// same name.
fn backends_to_purge(plugin_name: &str, plugin_type: PluginType) -> Result<Vec<Arc<dyn Backend>>> {
    if plugin_type != PluginType::VfoxBackend {
        return Ok(backend::get(&plugin_name.into()).into_iter().collect());
    }
    let backend_type = BackendType::VfoxBackend(plugin_name.to_string());
    Ok(install_state::try_list_tools()?
        .values()
        .filter_map(|tool| backend::arg_to_backend(tool.clone().into()))
        .filter(|backend| backend.get_type() == backend_type)
        .collect())
}
