use std::collections::HashSet;
use std::ffi::OsString;
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
                // The next plugin's shared-directory check must not see the
                // tools this purge just removed.
                if self.purge {
                    install_state::reset_tools();
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
/// A plugin owns the tool of the same name. A backend plugin instead
/// provides any number of `plugin:tool` tools, so it owns every installed
/// tool it backs and every tool config declares with it. The second covers a
/// tool whose last version was uninstalled: its tool-level cache and kept
/// downloads remain, but it is no longer an installed tool.
///
/// Purging removes whole directories, and names can collide: `acme:extra`
/// and a plugin `acme-extra` both use `acme-extra`. So a tool is purged only
/// when no other tool, installed or configured, uses one of its directories;
/// otherwise it is skipped with a warning. Directories are never matched by
/// name alone.
///
/// Only the user's own installs dir is purged. The installed-tool scan also
/// reports tools found in shared and system install dirs, which other users
/// rely on, so a tool found only there is pointed back at the user's installs
/// dir: its own cache and downloads are purged, the shared install is not.
async fn backends_to_purge(
    plugin_name: &str,
    plugin_type: PluginType,
) -> Result<Vec<Arc<dyn Backend>>> {
    let config = Config::get().await?;
    let configured = config
        .get_tool_request_set()
        .await?
        .tools
        .keys()
        .map(|ba| (**ba).clone())
        .collect::<Vec<_>>();
    let installed = install_state::try_list_tools()?;
    let installed = installed
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
        .collect::<Vec<_>>();
    let backend_type = BackendType::VfoxBackend(plugin_name.to_string());
    let owns = |ba: &BackendArg| match plugin_type {
        PluginType::VfoxBackend => ba.backend_type() == backend_type,
        _ => ba.short == plugin_name,
    };
    let claimed = installed
        .iter()
        .chain(&configured)
        .filter(|ba| !owns(ba))
        .flat_map(dir_names)
        .collect::<HashSet<_>>();
    // Installed first, so a tool both installed and configured keeps its
    // installed paths.
    let targets = match plugin_type {
        PluginType::VfoxBackend => installed
            .iter()
            .chain(&configured)
            .filter(|ba| owns(ba))
            .cloned()
            .collect::<Vec<_>>(),
        _ => vec![BackendArg::from(plugin_name)],
    };
    Ok(targets
        .into_iter()
        .unique_by(|ba| ba.short.clone())
        .filter(|ba| {
            let shared = dir_names(ba).any(|name| claimed.contains(&name));
            if shared {
                warn!(
                    "not purging {}: another tool uses its directory",
                    style::eblue(&ba.short)
                );
            }
            !shared
        })
        // Built from the checked argument, not looked up: a cached backend can
        // hold a tool's shared install path.
        .filter_map(backend::arg_to_backend)
        .collect())
}

/// The names of the installs, cache, and downloads directories a tool uses.
fn dir_names(ba: &BackendArg) -> impl Iterator<Item = OsString> + '_ {
    [ba.installs_path(), ba.cache_path(), ba.downloads_path()]
        .into_iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_os_string())
}
