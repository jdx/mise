//! What a config's templated tool versions rendered to, as a snapshot.
//!
//! `mise prune` runs from any directory, so it cannot render
//! `node = "{{ vars.node }}"` the way the project does: the render depends on
//! the vars, env, `MISE_ENV`, `--no-env`, settings and dotenv files in effect
//! where the project is used. Rather than rebuild that, the identity layout
//! remembers the outcome.
//!
//! The rule prune relies on: a version may be removed on the strength of a
//! snapshot only if the snapshot is complete and current.
//!
//! * **Complete.** A snapshot is the config's whole rendered tool list for one
//!   context (the `MISE_ENV` and the set of loaded config files), taken from the
//!   live config and not from whichever tools a command happened to resolve, minus
//!   tools a higher-precedence config replaces. A new snapshot of a context
//!   replaces the old one outright, which is how a requirement that has ended
//!   stops protecting anything.
//! * **Current.** A snapshot lists the config files it loaded and a hash of each.
//!   If one has changed it is stale and ignored, and if one is gone the context
//!   cannot occur again. A config with no current snapshot keeps every
//!   installation of its templated tools until it is observed again.
//!
//! What a snapshot cannot see is anything that is not a config file: a changed
//! shell variable or flag is noticed at the next observation, not before.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::catalog::Catalog;
use crate::args::BackendArg;
use crate::config::Config;
use crate::toolset::{ToolRequest, ToolSource, ToolVersionOptions};
use crate::{dirs, file, hash};

/// One config's snapshots, one per context.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
struct Snapshots {
    /// The config these belong to, for readers of the catalog.
    config: String,
    #[serde(default)]
    contexts: Vec<Snapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    /// The `MISE_ENV` it was rendered under.
    env: String,
    /// Which config files were loaded, by path alone.
    context: String,
    /// The loaded config files and a hash of each file's bytes.
    files: Vec<(String, String)>,
    /// The rendered requests of the config's templated tools.
    tools: Vec<Recorded>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Recorded {
    backend: String,
    version: String,
    #[serde(default)]
    options: ToolVersionOptions,
}

/// The same file reached through a symlinked directory or a relative path names
/// one config.
fn canonical(config: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf())
}

fn path_for(catalog: &Catalog, config: &Path) -> std::path::PathBuf {
    catalog
        .meta_dir()
        .join("snapshots")
        .join(format!("{}.toml", hash::hash_to_str(&canonical(config))))
}

/// Hash of a file's bytes, or an empty string when it cannot be read.
fn bytes_hash(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| hash::hash_to_str(&bytes))
        .unwrap_or_default()
}

fn read(path: &Path) -> Option<Snapshots> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Contexts already snapshotted by this process; one observation is enough.
static OBSERVED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Snapshot, for the context `config` was loaded in, every loaded config that has
/// templated tool versions. Commands call this once they have resolved a toolset,
/// when the live config renders as the project does.
pub(crate) fn observe(config: &Config) {
    let env = crate::env::MISE_ENV.join(",");
    let mut paths = config
        .config_files
        .keys()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    let context = hash::hash_to_str(&paths);
    if !OBSERVED.lock().unwrap().insert(format!("{env}|{context}")) {
        return;
    }
    for (path, cf) in &config.config_files {
        if !cf.has_templated_tool_versions() {
            continue;
        }
        if let Err(err) = observe_config(config, path, &env, &context) {
            warn!(
                "could not record what {} renders to: {err:#}",
                crate::file::display_path(path)
            );
        }
    }
}

fn observe_config(config: &Config, path: &Path, env: &str, context: &str) -> Result<()> {
    let Some(cf) = config.config_files.get(path) else {
        return Ok(());
    };
    let templated = cf.templated_tool_backends();
    // A tool a higher-precedence config sets is not supplied by this one here.
    let mut replaced = vec![];
    for (other, cf) in &config.config_files {
        if other == path {
            break;
        }
        replaced.extend(cf.tool_backends());
    }
    // A config that does not render keeps the snapshot it has; prune then falls
    // back on it, or on keeping every installation, as it would anyway.
    let set = cf.to_tool_request_set()?;
    let tools = set
        .iter()
        .filter(|(ba, _, _)| templated.contains(&ba.short) && !replaced.contains(&ba.short))
        .flat_map(|(ba, requests, _)| {
            requests.iter().map(|request| Recorded {
                backend: ba.short.to_string(),
                version: request.version(),
                options: request.options(),
            })
        })
        .collect::<Vec<_>>();
    let snapshot = Snapshot {
        env: env.to_string(),
        context: context.to_string(),
        files: config
            .config_files
            .keys()
            .map(|p| (p.to_string_lossy().to_string(), bytes_hash(p)))
            .collect(),
        tools,
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let file = path_for(&catalog, path);
    let result = write(&catalog, &file, path, snapshot);
    if result.is_err() {
        // A record that cannot be brought up to date must not be trusted.
        let _ = std::fs::remove_file(&file);
    }
    result
}

fn write(catalog: &Catalog, file: &Path, path: &Path, snapshot: Snapshot) -> Result<()> {
    let _lock = catalog.lock()?;
    let mut snapshots = read(file).unwrap_or_default();
    snapshots.config = canonical(path).to_string_lossy().to_string();
    let slot = snapshots
        .contexts
        .iter()
        .position(|s| s.env == snapshot.env && s.context == snapshot.context);
    match slot {
        Some(i) if snapshots.contexts[i] == snapshot => return Ok(()),
        Some(i) => snapshots.contexts[i] = snapshot,
        None => snapshots.contexts.push(snapshot),
    }
    file::create_dir_all(file.parent().unwrap())?;
    file::write_atomic(file, toml::to_string_pretty(&snapshots)?)
}

/// The requests of `config`'s current snapshots, or `None` when there is no
/// snapshot that is complete and current. A snapshot that loaded a file that has
/// changed, or is gone, is not current.
pub(crate) fn current_requests(config: &Path, source: &ToolSource) -> Option<Vec<ToolRequest>> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let snapshots = read(&path_for(&catalog, config))?;
    let mut requests = vec![];
    let mut any = false;
    for snapshot in snapshots.contexts {
        let current = snapshot
            .files
            .iter()
            .all(|(path, hash)| !hash.is_empty() && bytes_hash(Path::new(path)) == *hash);
        if !current {
            continue;
        }
        any = true;
        for tool in snapshot.tools {
            let ba = Arc::new(BackendArg::from(tool.backend.as_str()));
            match ToolRequest::new_with_options(ba, &tool.version, tool.options, source.clone()) {
                Ok(request) => requests.push(request),
                // A request that cannot be rebuilt cannot vouch for anything.
                Err(err) => {
                    debug!("snapshot of {}: {err:#}", config.display());
                    return None;
                }
            }
        }
    }
    any.then_some(requests)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request survives being written to a snapshot and rebuilt for prune.
    #[test]
    fn requests_round_trip_through_a_snapshot() {
        for version in [
            "1.2.3",
            "20",
            "latest",
            "system",
            "ref:main",
            "sub-1:latest",
            "path:/opt/tool",
        ] {
            let ba = Arc::new(BackendArg::from("dummy"));
            let request = ToolRequest::new(ba, version, ToolSource::Argument).unwrap_or_else(|e| {
                panic!("{version}: {e:#}");
            });
            let recorded = Recorded {
                backend: "dummy".into(),
                version: request.version(),
                options: request.options(),
            };
            let text = toml::to_string_pretty(&Snapshots {
                config: "c".into(),
                contexts: vec![Snapshot {
                    env: String::new(),
                    context: String::new(),
                    files: vec![],
                    tools: vec![recorded],
                }],
            })
            .unwrap();
            let back: Snapshots = toml::from_str(&text).unwrap();
            let tool = &back.contexts[0].tools[0];
            let rebuilt = ToolRequest::new_with_options(
                Arc::new(BackendArg::from(tool.backend.as_str())),
                &tool.version,
                tool.options.clone(),
                ToolSource::Argument,
            )
            .unwrap();
            assert_eq!(rebuilt.version(), request.version(), "{version}");
            assert_eq!(rebuilt.options(), request.options(), "{version}");
        }
    }
}
