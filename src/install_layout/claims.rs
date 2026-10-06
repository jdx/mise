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
//! configuration of the same `MISE_ENV`, so an edit does not protect old
//! versions forever. Other tools, and other environments, keep their entries
//! until they are recorded again, so a record only ever keeps too much: a
//! version that is installed or resolved for a config is recorded as it is.
//! Prune refuses a config with a templated tool that has no entry, and a record
//! that fails to write is deleted, so a stale one is never trusted.

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
    /// The `MISE_ENV` it was rendered under.
    #[serde(default)]
    env: String,
    /// Fingerprint of the config files it was rendered under.
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

/// Fingerprint of the loaded config files that rendered the config's templates.
fn fingerprint(config: &Config) -> Result<String> {
    let mut text = String::new();
    for (path, cf) in &config.config_files {
        text.push_str(&path.to_string_lossy());
        text.push('\n');
        text.push_str(&cf.dump()?);
        text.push('\n');
    }
    Ok(hash::hash_to_str(&text))
}

/// Record that `tv`, installed in `dir`, is what a templated version of its
/// config renders to. A tool whose version is not a template, or that did not
/// come from a config, is not recorded: prune renders it without help.
pub(crate) fn record(tv: &ToolVersion, dir: &Path) -> Result<()> {
    match Config::maybe_get() {
        Some(config) => record_with(&config, tv, dir),
        None => Ok(()),
    }
}

/// [`record`] for a caller that has the config in hand.
pub(crate) fn record_with(config: &Config, tv: &ToolVersion, dir: &Path) -> Result<()> {
    let ToolSource::MiseToml(path) = tv.request.source() else {
        return Ok(());
    };
    if !config
        .config_files
        .get(path)
        .is_some_and(|cf| cf.templated_tool_backends().contains(&tv.ba().short))
    {
        return Ok(());
    }
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let file = path_for(&catalog, path);
    let result = write(config, tv, dir, path, &catalog, &file);
    if result.is_err() {
        // A record that cannot be brought up to date must not be trusted.
        let _ = std::fs::remove_file(&file);
    }
    result
}

fn write(
    config: &Config,
    tv: &ToolVersion,
    dir: &Path,
    path: &Path,
    catalog: &Catalog,
    file: &Path,
) -> Result<()> {
    let fingerprint = fingerprint(config)?;
    let env = crate::env::MISE_ENV.join(",");
    let backend = tv.ba().short.to_string();
    let need = match super::resolver::dir_name_of(dir) {
        Some(dir) => Need {
            backend,
            env,
            fingerprint,
            short: String::new(),
            version: dir,
        },
        None => Need {
            short: backend.clone(),
            backend,
            env,
            fingerprint,
            version: tv.tv_pathname(),
        },
    };
    let _lock = catalog.lock()?;
    let mut claims = read(file).unwrap_or_default();
    if claims.needs.contains(&need) {
        return Ok(());
    }
    // This tool's entries from an older configuration are stale; other tools'
    // and other environments' are left until they are recorded themselves.
    claims.needs.retain(|n| {
        n.backend != need.backend || n.env != need.env || n.fingerprint == need.fingerprint
    });
    claims.config = canonical(path).to_string_lossy().to_string();
    claims.needs.push(need);
    file::create_dir_all(file.parent().unwrap())?;
    file::write_atomic(file, toml::to_string_pretty(&claims)?)
}

/// Record an installation that a command resolved for a templated version of its
/// config and found already installed, for commands that only use installs
/// (`mise exec`, `mise run`, the shell hook) and so never reach an install.
pub(crate) fn note_use(config: &Config, tv: &ToolVersion) {
    let ToolSource::MiseToml(path) = tv.request.source() else {
        return;
    };
    if !config
        .config_files
        .get(path)
        .is_some_and(|cf| cf.templated_tool_backends().contains(&tv.ba().short))
    {
        return;
    }
    let mut bare = tv.clone();
    bare.install_path = None;
    // The identity layout's directory when one is installed; otherwise the legacy
    // path, which is where a tool the layout does not govern (`http`, `rust`,
    // `dotnet`) or one installed before it lives. Prune keys those by tool and
    // version.
    let dir = match super::resolver::locate(&bare) {
        Some(located) if located.installed => located.dir,
        _ => {
            let legacy = tv.install_path();
            if !legacy.exists() {
                return;
            }
            legacy
        }
    };
    if let Err(err) = record_with(config, tv, &dir) {
        warn!("could not record what {} is used for: {err:#}", tv.style());
    }
}

fn read(path: &Path) -> Option<Claims> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The installations recorded for `config`, in the keys `mise prune` uses, or
/// `None` when nothing was recorded or a tool in `backends` has no entry.
pub(crate) fn needed_by(config: &Path, backends: &[String]) -> Option<Vec<(String, String)>> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let claims = read(&path_for(&catalog, config))?;
    if backends
        .iter()
        .any(|backend| !claims.needs.iter().any(|need| &need.backend == backend))
    {
        return None;
    }
    Some(
        claims
            .needs
            .into_iter()
            .filter(|need| backends.contains(&need.backend))
            .map(|need| (need.short, need.version))
            .collect(),
    )
}
