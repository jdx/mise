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
//! Prune refuses a config unless, for the config file as it is now, every
//! environment and directory it was used in recorded every templated tool, and a
//! record that fails to write is deleted, so a stale one is never trusted.

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
    /// Which config files were loaded when it was rendered. A project used from
    /// another directory loads another set and renders its own outcome.
    #[serde(default)]
    context: String,
    /// Fingerprint of those files' contents.
    fingerprint: String,
    /// Hash of the config's own file, so prune can tell whether the record was
    /// made for the file as it is now.
    #[serde(default)]
    file: String,
    /// Set when this records that a higher-precedence config supplies the tool in
    /// this context instead, rather than an installation.
    #[serde(default)]
    overridden: bool,
    /// For an override: the configs that supply the tool, with a hash of each
    /// file's bytes, so the marker is only trusted while they are unchanged.
    #[serde(default)]
    by: Vec<(String, String)>,
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

/// Hash of one config file's own contents.
pub(crate) fn file_hash(cf: &dyn crate::config::config_file::ConfigFile) -> Result<String> {
    Ok(hash::hash_to_str(&cf.dump()?))
}

/// Which config files were loaded, by path alone.
fn context(config: &Config) -> String {
    let mut paths = config
        .config_files
        .keys()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    hash::hash_to_str(&paths)
}

/// Record that `tv`, installed in `dir`, is what a templated version of its
/// config renders to. A tool whose version is not a template, or that did not
/// come from a config, is not recorded: prune renders it without help.
pub(crate) fn record(tv: &ToolVersion, dir: &Path) -> Result<()> {
    match Config::maybe_get() {
        Some(config) => record_with(&config, tv, Some(dir)),
        None => Ok(()),
    }
}

/// [`record`] for a caller that has the config in hand. With no `dir` the version
/// renders to nothing installed (`system`), which still counts as recorded.
pub(crate) fn record_with(config: &Config, tv: &ToolVersion, dir: Option<&Path>) -> Result<()> {
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
    dir: Option<&Path>,
    path: &Path,
    catalog: &Catalog,
    file: &Path,
) -> Result<()> {
    let fingerprint = fingerprint(config)?;
    let env = crate::env::MISE_ENV.join(",");
    let backend = tv.ba().short.to_string();
    let context = context(config);
    let file_hash = config
        .config_files
        .get(path)
        .map(|cf| file_hash(cf.as_ref()))
        .transpose()?
        .unwrap_or_default();
    let need = match dir {
        None => Need {
            backend,
            env,
            context,
            fingerprint,
            file: file_hash,
            overridden: false,
            by: vec![],
            short: String::new(),
            version: String::new(),
        },
        Some(dir) => match super::resolver::dir_name_of(dir) {
            Some(dir) => Need {
                backend,
                env,
                context,
                fingerprint,
                file: file_hash,
                overridden: false,
                by: vec![],
                short: String::new(),
                version: dir,
            },
            None => Need {
                short: backend.clone(),
                backend,
                env,
                context,
                fingerprint,
                file: file_hash,
                overridden: false,
                by: vec![],
                version: tv.tv_pathname(),
            },
        },
    };
    let _lock = catalog.lock()?;
    let mut claims = read(file).unwrap_or_default();
    let before = claims.needs.clone();
    claims.config = canonical(path).to_string_lossy().to_string();
    let same_context = |n: &Need| n.env == need.env && n.context == need.context;
    // A config loaded after a higher-precedence one that sets the same tool never
    // supplies it in this context. Record that, so the context still counts as
    // complete: re-running the install cannot record what the project does not
    // use. Markers for tools that are no longer overridden are dropped.
    let overridden = overridden_by(config, path);
    claims.needs.retain(|n| {
        !(n.overridden
            && same_context(n)
            && !overridden
                .iter()
                .any(|(backend, by)| *backend == n.backend && *by == n.by))
    });
    for (backend, by) in overridden {
        let marker = Need {
            backend,
            overridden: true,
            by,
            short: String::new(),
            version: String::new(),
            ..need.clone()
        };
        claims.needs.retain(|n| {
            n.backend != marker.backend || !same_context(n) || n.fingerprint == marker.fingerprint
        });
        if !claims.needs.contains(&marker) {
            claims.needs.push(marker);
        }
    }
    if !claims.needs.contains(&need) {
        // This tool's entries from older contents of the same config files are
        // stale; other tools', other environments' and other directories' are
        // left until they are recorded themselves.
        claims.needs.retain(|n| {
            n.backend != need.backend || !same_context(n) || n.fingerprint == need.fingerprint
        });
        claims.needs.push(need.clone());
    }
    if claims.needs == before {
        return Ok(());
    }
    file::create_dir_all(file.parent().unwrap())?;
    file::write_atomic(file, toml::to_string_pretty(&claims)?)
}

/// Hash of a file's bytes, or an empty string when it cannot be read.
fn bytes_hash(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| hash::hash_to_str(&bytes))
        .unwrap_or_default()
}

/// The templated tools of `path` that a config loaded before it (higher
/// precedence) also sets, each with the configs that set it and their hashes.
fn overridden_by(config: &Config, path: &Path) -> Vec<(String, Vec<(String, String)>)> {
    let Some(cf) = config.config_files.get(path) else {
        return vec![];
    };
    let mut higher: Vec<(String, std::path::PathBuf)> = vec![];
    for (other, cf) in &config.config_files {
        if other == path {
            break;
        }
        higher.extend(cf.tool_backends().into_iter().map(|b| (b, other.clone())));
    }
    let mut overridden = vec![];
    for backend in cf.templated_tool_backends() {
        if overridden.iter().any(|(b, _)| *b == backend) {
            continue;
        }
        let by = higher
            .iter()
            .filter(|(b, _)| *b == backend)
            .map(|(_, other)| (other.to_string_lossy().to_string(), bytes_hash(other)))
            .collect::<Vec<_>>();
        if !by.is_empty() {
            overridden.push((backend, by));
        }
    }
    overridden
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
    if matches!(tv.request, crate::toolset::ToolRequest::System { .. }) {
        if let Err(err) = record_with(config, tv, None) {
            warn!("could not record what {} is used for: {err:#}", tv.style());
        }
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
    if let Err(err) = record_with(config, tv, Some(&dir)) {
        warn!("could not record what {} is used for: {err:#}", tv.style());
    }
}

fn read(path: &Path) -> Option<Claims> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// What `mise prune` keeps for a config's templated versions.
#[derive(Debug, Default)]
pub(crate) struct Needed {
    /// Installations to keep, in the keys `mise prune` uses.
    pub(crate) keys: Vec<(String, String)>,
    /// Tools with several templated versions in the config. The record cannot say
    /// which version an entry belongs to, so every installation of these is kept.
    pub(crate) keep_all: Vec<String>,
}

/// The installations recorded for `config`, or `None` when a templated version
/// in `backends` (one per version, so a tool may repeat) is not covered. `file`
/// is [`file_hash`] of the config file as it is now.
///
/// Every environment and directory the config was used in has to cover every
/// templated tool on its own: entries recorded under different contexts must not
/// stand in for each other, since each context renders its own versions.
pub(crate) fn needed_by(config: &Path, file: &str, backends: &[String]) -> Option<Needed> {
    if backends.is_empty() {
        return Some(Needed::default());
    }
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let claims = read(&path_for(&catalog, config))?;
    let mut needed = Needed::default();
    let mut single: Vec<&String> = vec![];
    for backend in backends {
        if backends.iter().filter(|b| *b == backend).count() > 1 {
            if !needed.keep_all.contains(backend) {
                needed.keep_all.push(backend.clone());
            }
        } else {
            single.push(backend);
        }
    }
    if !single.is_empty() {
        let mut contexts: Vec<(&str, &str)> = vec![];
        for need in claims.needs.iter().filter(|need| need.file == file) {
            let key = (need.env.as_str(), need.context.as_str());
            if !contexts.contains(&key) {
                contexts.push(key);
            }
        }
        if contexts.is_empty() {
            return None;
        }
        // A context that loaded a config file that is gone cannot happen again, so
        // its override markers can no longer be checked or refreshed; retire it.
        // Its recorded installations are still kept.
        contexts.retain(|(env, context)| {
            !claims.needs.iter().any(|need| {
                need.overridden
                    && need.env == *env
                    && need.context == *context
                    && need.by.iter().any(|(path, _)| !Path::new(path).exists())
            })
        });
        // With nothing left, no context vouches for the config as it is now.
        if contexts.is_empty() {
            return None;
        }
        for (env, context) in contexts {
            let covered = |backend: &String| {
                claims.needs.iter().any(|need| {
                    need.file == file
                        && need.env == env
                        && need.context == context
                        && &need.backend == backend
                        && (!need.overridden
                            || (!need.by.is_empty()
                                && need
                                    .by
                                    .iter()
                                    .all(|(path, hash)| bytes_hash(Path::new(path)) == *hash)))
                })
            };
            if !single.iter().all(|backend| covered(backend)) {
                return None;
            }
        }
    }
    needed.keys = claims
        .needs
        .into_iter()
        .filter(|need| backends.contains(&need.backend) && !need.version.is_empty())
        .map(|need| (need.short, need.version))
        .collect();
    Some(needed)
}
