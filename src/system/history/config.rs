//! `[history]`: the operational history configuration read from the system
//! and global configuration layers only. Project configuration found from
//! the current directory never contributes, so no project can change what
//! personal history captures.

use std::path::PathBuf;

use eyre::Result;
use serde::Deserialize;

use crate::config::config_file::mise_toml::MiseToml;
use crate::file::display_path;

/// `[history]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) struct HistoryTomlConfig {
    /// Globs (with `~`) never captured; `!glob` re-includes.
    #[serde(default)]
    pub exclude: Vec<String>,
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

/// The effective `exclude` list: a union across layers where `!glob`
/// removes an inherited glob.
pub(crate) fn exclude_globs() -> Result<Vec<String>> {
    let mut globs: Vec<String> = vec![];
    for (_, layer) in layers()? {
        for glob in &layer.exclude {
            if let Some(re_included) = glob.strip_prefix('!') {
                globs.retain(|existing| existing != re_included);
            } else if !globs.contains(glob) {
                globs.push(glob.clone());
            }
        }
    }
    Ok(globs)
}
