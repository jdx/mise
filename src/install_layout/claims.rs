//! Which installations a config's templated tool versions rendered to.
//!
//! `mise prune` runs from any directory, so it cannot render
//! `node = "{{ vars.node }}"` the way the project does: the render depends on
//! the vars, env, `MISE_ENV`, `--no-env`, settings and dotenv files in effect
//! when the project was used. Rather than rebuild that, the identity layout
//! remembers the outcome. Whenever an installation is installed or reused for a
//! templated version of a config, the catalog records it under that config, and
//! prune protects what is recorded.
//!
//! Recording a tool replaces that tool's entries made under an older
//! configuration, so an edit does not protect old versions forever. Other tools
//! keep their entries until they are recorded again, so between an edit and the
//! next use a record can only keep too much, never too little: a version that is
//! installed is recorded as it is installed.

use std::path::Path;

use eyre::Result;
use serde::{Deserialize, Serialize};

use super::catalog::Catalog;
use crate::config::Config;
use crate::toolset::{ToolSource, ToolVersion};
use crate::{dirs, file, hash};

/// What one config's templated versions rendered to under one configuration.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Claims {
    /// The config these belong to, for readers of the catalog.
    config: String,
    #[serde(default)]
    needs: Vec<Need>,
}

/// One installation, in the form `mise prune` keys needed versions by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Need {
    /// The tool this installation is for.
    backend: String,
    /// Fingerprint of the configuration it was rendered under.
    fingerprint: String,
    /// The tool name for a legacy path; empty for an identity-layout directory.
    short: String,
    /// The version directory name, or the identity-layout directory name.
    version: String,
}

/// The same file reached through a symlinked directory or a relative path names
/// one config.
fn canonical(config: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf())
}

fn path_for(catalog: &Catalog, config: &Path) -> std::path::PathBuf {
    catalog
        .meta_dir()
        .join("claims")
        .join(format!("{}.toml", hash::hash_to_str(&canonical(config))))
}

/// Fingerprint of everything that rendered the config's templates: the loaded
/// config files and the environments selecting among them.
fn fingerprint(config: &Config) -> Result<String> {
    let mut text = String::new();
    for (path, cf) in &config.config_files {
        text.push_str(&path.to_string_lossy());
        text.push('\n');
        text.push_str(&cf.dump()?);
        text.push('\n');
    }
    text.push_str(&crate::env::MISE_ENV.join(","));
    Ok(hash::hash_to_str(&text))
}

/// Record that `tv`, installed in `dir`, is what a templated version of its
/// config renders to. A version that is not a template, or that did not come
/// from a config, is not recorded: prune renders it without help.
pub(crate) fn record(tv: &ToolVersion, dir: &Path) -> Result<()> {
    let ToolSource::MiseToml(path) = tv.request.source() else {
        return Ok(());
    };
    let Some(config) = Config::maybe_get() else {
        return Ok(());
    };
    if !config
        .config_files
        .get(path)
        .is_some_and(|cf| cf.has_templated_tool_versions())
    {
        return Ok(());
    }
    let fingerprint = fingerprint(&config)?;
    let backend = tv.ba().short.to_string();
    let need = match super::resolver::dir_name_of(dir) {
        Some(dir) => Need {
            backend,
            fingerprint,
            short: String::new(),
            version: dir,
        },
        None => Need {
            short: backend.clone(),
            backend,
            fingerprint,
            version: tv.tv_pathname(),
        },
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let file = path_for(&catalog, path);
    let _lock = catalog.lock()?;
    let mut claims = read(&file).unwrap_or_default();
    if claims.needs.contains(&need) {
        return Ok(());
    }
    // This tool's entries from an older configuration are stale; other tools'
    // are left until they are recorded themselves.
    claims
        .needs
        .retain(|n| n.backend != need.backend || n.fingerprint == need.fingerprint);
    claims.config = canonical(path).to_string_lossy().to_string();
    claims.needs.push(need);
    file::create_dir_all(file.parent().unwrap())?;
    file::write_atomic(&file, toml::to_string_pretty(&claims)?)
}

fn read(path: &Path) -> Option<Claims> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The installations recorded for `config`, in the keys `mise prune` uses, or
/// `None` when nothing was recorded.
pub(crate) fn needed_by(config: &Path) -> Option<Vec<(String, String)>> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let claims = read(&path_for(&catalog, config))?;
    Some(
        claims
            .needs
            .into_iter()
            .map(|need| (need.short, need.version))
            .collect(),
    )
}
