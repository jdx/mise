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
//! A record is replaced when the loaded configuration changes, so an edit does
//! not protect old versions forever. Between an edit and the next use a stale
//! record can only keep too much, never too little: a version that is installed
//! is recorded as it is installed.

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
    /// Fingerprint of the configuration they were rendered under.
    fingerprint: String,
    #[serde(default)]
    needs: Vec<Need>,
}

/// One installation, in the form `mise prune` keys needed versions by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Need {
    /// The tool name for a legacy path; empty for an identity-layout directory.
    short: String,
    /// The version directory name, or the identity-layout directory name.
    version: String,
}

fn path_for(catalog: &Catalog, config: &Path) -> std::path::PathBuf {
    catalog
        .meta_dir()
        .join("claims")
        .join(format!("{}.toml", hash::hash_to_str(&config)))
}

/// Fingerprint of everything that rendered the config's templates: the loaded
/// config files and the environments selecting among them.
fn fingerprint(config: &Config) -> String {
    let mut text = String::new();
    for (path, cf) in &config.config_files {
        text.push_str(&path.to_string_lossy());
        text.push('\n');
        text.push_str(&cf.dump().unwrap_or_default());
        text.push('\n');
    }
    text.push_str(&crate::env::MISE_ENV.join(","));
    hash::hash_to_str(&text)
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
    let need = match super::resolver::dir_name_of(dir) {
        Some(dir) => Need {
            short: String::new(),
            version: dir,
        },
        None => Need {
            short: tv.ba().short.to_string(),
            version: tv.tv_pathname(),
        },
    };
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let file = path_for(&catalog, path);
    let fingerprint = fingerprint(&config);
    let _lock = catalog.lock()?;
    let mut claims = read(&file).unwrap_or_default();
    if claims.fingerprint != fingerprint {
        claims = Claims::default();
    } else if claims.needs.contains(&need) {
        return Ok(());
    }
    claims.config = path.to_string_lossy().to_string();
    claims.fingerprint = fingerprint;
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
