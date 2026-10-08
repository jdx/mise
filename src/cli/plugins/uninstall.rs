use std::path::PathBuf;
use std::sync::Arc;

use eyre::Result;

use crate::args::BackendArg;
use crate::backend::backend_type::BackendType;
use crate::backend::{Backend, unalias_backend};
use crate::plugins::PluginType;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::style;
use crate::{backend, dirs, env, file, plugins};

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
                let (backends, leftovers) = match self.purge {
                    true => (
                        backends_to_purge(plugin_name, plugin.get_plugin_type())?,
                        leftover_tool_dirs(plugin_name, plugin.get_plugin_type())?,
                    ),
                    false => (vec![], vec![]),
                };
                plugin.uninstall(pr.as_ref()).await?;
                for backend in backends {
                    backend.purge(pr.as_ref())?;
                }
                for dir in leftovers {
                    file::remove_all_with_progress(dir, pr.as_ref())?;
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
///
/// Only the user's own installs dir is purged. The installed-tool scan also
/// reports tools found in shared and system install dirs, which other users
/// rely on, so a tool found only there is pointed back at the user's installs
/// dir: its own cache and downloads are purged, the shared install is not.
fn backends_to_purge(plugin_name: &str, plugin_type: PluginType) -> Result<Vec<Arc<dyn Backend>>> {
    if plugin_type != PluginType::VfoxBackend {
        return Ok(backend::get(&plugin_name.into()).into_iter().collect());
    }
    let backend_type = BackendType::VfoxBackend(plugin_name.to_string());
    Ok(install_state::try_list_tools()?
        .values()
        .map(|tool| {
            let mut tool = tool.clone();
            if tool.installs_path.as_deref().is_some_and(|path| {
                env::install_path_category(path) != env::InstallPathCategory::Local
            }) {
                tool.installs_path = None;
            }
            BackendArg::from(tool)
        })
        .filter(|ba| ba.backend_type() == backend_type)
        .filter_map(backend::arg_to_backend)
        .collect())
}

/// Install, cache and download dirs of a backend plugin's tools that are no
/// longer installed. Uninstalling a tool's last version keeps its tool-level
/// cache, its version symlinks, and its downloads with `always_keep_download`,
/// but the tool drops out of the installed-tool scan, so only the
/// `<plugin>-<tool>` dir names are left to find them by.
///
/// A dir is left alone when it belongs to an installed tool of another
/// backend, or to a plugin whose name extends this one (`<plugin>-x-<tool>`).
fn leftover_tool_dirs(plugin_name: &str, plugin_type: PluginType) -> Result<Vec<PathBuf>> {
    if plugin_type != PluginType::VfoxBackend {
        return Ok(vec![]);
    }
    let prefix = format!("{}-", backend::tool_directory_name(plugin_name));
    let longer_plugins = file::dir_subdirs(&dirs::PLUGINS)?
        .into_iter()
        .map(|name| format!("{}-", backend::tool_directory_name(&name)))
        .filter(|other| other.len() > prefix.len() && other.starts_with(&prefix))
        .collect::<Vec<_>>();
    let backend_type = BackendType::VfoxBackend(plugin_name.to_string());
    let other_tools = install_state::try_list_tools()?
        .values()
        .filter(|tool| BackendArg::from((*tool).clone()).backend_type() != backend_type)
        .map(|tool| backend::tool_directory_name(&tool.short))
        .collect::<Vec<_>>();
    let mut leftovers = vec![];
    for base in [*dirs::INSTALLS, *dirs::CACHE, *dirs::DOWNLOADS] {
        for name in file::dir_subdirs(base)? {
            if name.starts_with(&prefix)
                && !longer_plugins.iter().any(|other| name.starts_with(other))
                && !other_tools.contains(&name)
            {
                leftovers.push(base.join(name));
            }
        }
    }
    Ok(leftovers)
}
