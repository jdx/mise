//! The config files of the nearest project directory, for commands that act on
//! "this project" rather than on everything mise has loaded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eyre::{Result, eyre};
use itertools::Itertools;

use crate::config::config_file::ConfigFile;
use crate::config::{self, Config};
use crate::daemons::glue::DaemonSetExt;
use crate::dirs;
use crate::file::display_path;
use crate::toolset::{ToolRequest, ToolRequestSet, ToolSource};

/// The config files of the nearest directory, from the working directory up,
/// that has any. Only that directory's configs are the project's: a parent's
/// config is another project's, or the monorepo's.
///
/// A directory's configs are the ones discovery finds in it, which includes
/// `.mise/config.toml` and `conf.d` folder fragments whose own config root is a
/// folder below the project, plus any file such as `.tool-versions` or an
/// idiomatic version file that sits in it. Global, system and override configs
/// never count, and neither does a config directly in the home directory.
pub fn nearest_project_configs(config: &Config) -> Option<(PathBuf, Vec<Arc<dyn ConfigFile>>)> {
    let cwd = dirs::CWD.as_ref()?;
    cwd.ancestors()
        .filter(|dir| *dir != *dirs::HOME)
        .find_map(|dir| {
            let discovered = config::config_files_in_dir(dir);
            let files = config
                .config_files
                .iter()
                .filter(|(path, _)| {
                    !config::is_global_config(path)
                        && (discovered.contains(path.as_path()) || path.parent() == Some(dir))
                })
                .map(|(_, cf)| cf.clone())
                .collect_vec();
            (!files.is_empty()).then(|| (dir.to_path_buf(), files))
        })
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
    // Daemon tools are declared in the same files but loaded separately.
    let mut daemon_tools = ToolRequestSet::new();
    config.daemons()?.add_tool_requests(&mut daemon_tools)?;
    let paths = files.iter().map(|cf| cf.get_path()).collect::<HashSet<_>>();
    requests.extend(
        daemon_tools
            .iter()
            .flat_map(|(_, trs, _)| trs.iter())
            .filter(|tr| config_file_of(tr.source()).is_some_and(|path| paths.contains(path)))
            .cloned(),
    );
    Ok((root, requests))
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

/// The file a tool request came from, when it came from a project's config
/// rather than a flag, an environment variable or a tool stub.
fn config_file_of(source: &ToolSource) -> Option<&Path> {
    match source {
        ToolSource::MiseToml(path)
        | ToolSource::MiseTomlDaemon(path)
        | ToolSource::IdiomaticVersionFile(path)
        | ToolSource::ToolVersions(path) => Some(path),
        ToolSource::ToolStub(_)
        | ToolSource::Argument
        | ToolSource::Environment(..)
        | ToolSource::Unknown => None,
    }
}
