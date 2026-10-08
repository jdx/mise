use std::sync::Arc;

use eyre::Result;

use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::backend::{Backend, unalias_backend};
use crate::config::Config;
use crate::plugins::PluginType;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend, env, plugins};
use itertools::Itertools;

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
                    true => backends_to_purge(plugin_name, plugin.get_plugin_type()).await?,
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
/// installed tool it backs is purged, and every tool config declares with it.
/// The second covers a tool whose last version was uninstalled: its tool-level
/// cache and kept downloads remain, but it is no longer an installed tool.
/// Directories are never matched by name, since `<plugin>-<tool>` can also
/// be another tool's directory. Other plugins provide the tool of the same
/// name.
///
/// Only the user's own installs dir is purged. The installed-tool scan also
/// reports tools found in shared and system install dirs, which other users
/// rely on, so a tool found only there is pointed back at the user's installs
/// dir: its own cache and downloads are purged, the shared install is not.
async fn backends_to_purge(
    plugin_name: &str,
    plugin_type: PluginType,
) -> Result<Vec<Arc<dyn Backend>>> {
    if plugin_type != PluginType::VfoxBackend {
        return Ok(backend::get(&plugin_name.into()).into_iter().collect());
    }
    let backend_type = BackendType::VfoxBackend(plugin_name.to_string());
    let config = Config::get().await?;
    let configured = config
        .get_tool_request_set()
        .await?
        .tools
        .keys()
        .map(|ba| (**ba).clone())
        .collect::<Vec<_>>();
    let installed = install_state::try_list_tools()?;
    let installed =
        installed.values().map(|tool| {
            let mut tool = tool.clone();
            if tool.installs_path.as_deref().is_some_and(|path| {
                env::install_path_category(path) != env::InstallPathCategory::Local
            }) {
                tool.installs_path = None;
            }
            BackendArg::from(tool)
        });
    Ok(installed
        .chain(configured)
        .filter(|ba| ba.backend_type() == backend_type)
        .unique_by(|ba| ba.short.clone())
        .filter_map(backend::arg_to_backend)
        .collect())
}
