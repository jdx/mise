//! The config files of the nearest project directory, for commands that act on
//! "this project" rather than on everything mise has loaded.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, eyre};
use itertools::Itertools;

use crate::args::BackendArg;
use crate::config::config_file::ConfigFile;
use crate::config::config_file::config_root::{config_root, nested_checkout};
use crate::config::{self, Config};
use crate::dirs;
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolRequestSetBuilder, ToolSource};

/// The config files of the nearest directory, from the working directory up,
/// that owns any. Only that directory's configs are the project's: a parent's
/// config is another project's, or the monorepo's.
///
/// A file belongs to the directory it configures, the one mise calls its config
/// root: `.mise/config.toml`, `.config/mise.toml` and `.config/mise/config.toml`
/// all belong to the directory holding the `.mise` or `.config` folder, and a
/// `conf.d` folder fragment to the project its `conf.d` belongs to. Running from
/// inside one of those folders therefore still finds the project that owns it.
/// Global, system and override configs never count, and neither does a config
/// directly in the home directory.
pub fn nearest_project_configs(config: &Config) -> Option<(PathBuf, Vec<Arc<dyn ConfigFile>>)> {
    let cwd = dirs::CWD.as_ref()?;
    let owned = config
        .config_files
        .iter()
        .filter(|(path, _)| !config::is_global_config(path))
        .map(|(path, cf)| (owning_dir(path), cf))
        .collect_vec();
    cwd.ancestors()
        .filter(|dir| *dir != *dirs::HOME)
        .find_map(|dir| {
            let files = owned
                .iter()
                .filter(|(owner, _)| owner == dir)
                .map(|(_, cf)| Arc::clone(cf))
                .collect_vec();
            (!files.is_empty()).then(|| (dir.to_path_buf(), files))
        })
}

/// The directory a config file configures. A folder fragment is its own config
/// root so that relative paths stay inside the folder, but it configures the
/// project its `conf.d` belongs to. A git checkout of its own inside `.mise` or
/// `.config` is a project of its own, not part of the directory around it.
fn owning_dir(path: &Path) -> PathBuf {
    if let Some(checkout) = nested_checkout(path) {
        return checkout;
    }
    match path.parent() {
        Some(dir) if config::is_conf_d_folder_file(path) => config_root(dir),
        _ => config_root(path),
    }
}

/// Every tool request the nearest project's config files declare, and that
/// project's directory.
///
/// Requests come from those files alone, each with its own source. The merged
/// tool request set drops what a later file or a shell override such as
/// `MISE_NODE_VERSION` replaces, which would hide the version a file names.
pub fn nearest_project_tool_requests(config: &Config) -> Result<(PathBuf, Vec<ToolRequest>)> {
    let (root, files) = nearest_project_configs(config).ok_or_else(|| {
        eyre!(
            "no config file found in {} or a parent directory",
            dirs::CWD.as_deref().map(display_path).unwrap_or_default()
        )
    })?;
    let mut requests = Vec::new();
    for cf in &files {
        let set = cf.to_tool_request_set()?;
        requests.extend(set.iter().flat_map(|(_, trs, _)| trs.iter().cloned()));
    }
    // Daemon tools are declared in the same files but loaded separately. They
    // are read here rather than through `add_tool_requests`, whose conflict
    // check is against the whole config and could refuse a project that normal
    // tool selection accepts.
    let paths = files.iter().map(|cf| cf.get_path()).collect::<HashSet<_>>();
    for daemon in config.daemons()?.daemons.values() {
        let Some((tool, version)) = daemon.tool.as_ref().filter(|_| !daemon.imported) else {
            continue;
        };
        if paths.contains(daemon.source.as_path()) {
            let ba = Arc::new(BackendArg::from(tool.as_str()));
            let source = ToolSource::MiseTomlDaemon(daemon.source.clone());
            requests.push(ToolRequest::new(ba, version, source)?);
        }
    }
    // Tools that settings or the platform leave out are not this project's.
    let filter = ToolRequestSetBuilder::new();
    requests.retain(|tr| !filter.is_disabled(tr.ba()));
    Ok((root, requests))
}

/// The requests mise would actually use: for each tool, those from the
/// highest-priority file that names it. `requests` comes in priority order, as
/// [`nearest_project_tool_requests`] returns it, so a version that a later file
/// overrides is dropped, as when mise merges the files itself.
pub fn effective_requests(requests: Vec<ToolRequest>) -> Vec<ToolRequest> {
    let mut winners: HashMap<Arc<BackendArg>, ToolSource> = HashMap::new();
    for tr in &requests {
        winners
            .entry(tr.ba().clone())
            .or_insert_with(|| tr.source().clone());
    }
    requests
        .into_iter()
        .filter(|tr| winners.get(tr.ba()) == Some(tr.source()))
        .collect()
}

/// The requests as a set, each filed under its own source.
pub fn tool_request_set(requests: impl IntoIterator<Item = ToolRequest>) -> ToolRequestSet {
    let mut set = ToolRequestSet::new();
    for tr in requests {
        let source = tr.source().clone();
        set.add_version(tr, &source);
    }
    set
}
