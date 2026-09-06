//! `[history]`: the operational history configuration read from the system
//! and global configuration layers only. Project configuration found from
//! the current directory never contributes, so no project can change what
//! personal history captures.

use std::path::PathBuf;

use eyre::Result;
use indexmap::IndexMap;
use serde::Deserialize;

use crate::config::config_file::mise_toml::MiseToml;
use crate::file::display_path;

/// `[history]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct HistoryTomlConfig {
    /// Globs (with `~`) never captured; `!glob` re-includes.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Commands to run after a rollback touches a matching path.
    #[serde(default)]
    pub reload: IndexMap<String, String>,
    /// The setup repository this machine publishes to and fetches from.
    #[serde(default)]
    pub origin: Option<OriginTomlConfig>,
}

/// `[history.origin]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct OriginTomlConfig {
    pub url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Encrypt machine recovery refs (a later milestone; parsed and refused
    /// until then so an early declaration is never silently plaintext).
    #[serde(default)]
    pub encrypt_backups: bool,
}

fn default_branch() -> String {
    "main".to_string()
}

/// The effective `[history.origin]`: the last layer that declares one.
pub(crate) fn origin() -> Result<Option<(PathBuf, OriginTomlConfig)>> {
    let mut found = None;
    for (path, layer) in layers()? {
        if let Some(origin) = layer.origin {
            found = Some((path, origin));
        }
    }
    Ok(found)
}

/// The `[history]` tables of the system and global layers, in discovery
/// order, each with the file that declared it.
pub(crate) fn layers() -> Result<Vec<(PathBuf, HistoryTomlConfig)>> {
    let mut layers = vec![];
    let files = crate::config::system_config_files()
        .into_iter()
        .chain(crate::config::global_config_files())
        .filter(|path| path.is_file())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"));
    for path in files {
        let toml = match MiseToml::from_file(&path) {
            Ok(toml) => toml,
            Err(err) => {
                warn!("history: skipping {}: {err}", display_path(&path));
                continue;
            }
        };
        if let Some(history) = toml.history_config() {
            layers.push((path, history));
        }
    }
    Ok(layers)
}

/// The effective reload map: glob -> command, a later layer overriding an
/// earlier one for the same glob. Read from the trusted layers only, and
/// resolved before an operation begins so nothing it writes can change it.
pub(crate) fn reload_commands() -> Result<IndexMap<String, String>> {
    let mut commands = IndexMap::new();
    for (path, layer) in layers()? {
        if !crate::config::config_file::is_trusted(&path) {
            if !layer.reload.is_empty() {
                warn!(
                    "history: ignoring [history.reload] in untrusted {}",
                    display_path(&path)
                );
            }
            continue;
        }
        for (glob, command) in &layer.reload {
            commands.insert(glob.clone(), command.clone());
        }
    }
    Ok(commands)
}

/// The effective `exclude` list: every layer's globs in order, later
/// layers after earlier ones, repeats included. Patterns apply in order
/// and the last match wins, so `!glob` re-includes what an earlier glob
/// excluded and a repeated glob excludes again what a `!glob` in between
/// re-included.
pub(crate) fn exclude_globs() -> Result<Vec<String>> {
    let mut globs: Vec<String> = vec![];
    for (_, layer) in layers()? {
        globs.extend(layer.exclude.iter().cloned());
    }
    Ok(globs)
}
