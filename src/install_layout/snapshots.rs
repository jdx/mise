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
//!   requests a command resolved, and only when that command resolved every
//!   templated tool of the config that no other source replaces. A command that
//!   resolved fewer (`mise exec node@22`, a scoped toolset) records nothing. A new
//!   snapshot of a context replaces the old one outright, which is how a
//!   requirement that has ended stops protecting anything.
//! * **Current.** A snapshot lists the config files it loaded and a hash of each.
//!   If one is gone the context cannot occur again and is retired. If one has
//!   changed the snapshot is stale, and prune keeps every installation of the
//!   config's templated tools until it is observed again. So does a config with no
//!   snapshot. Commands that inspect rather than use a project, such as prune,
//!   do not take snapshots.
//!
//! What a snapshot cannot see is anything that is not a config file: a changed
//! shell variable or flag is noticed at the next observation, not before.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use eyre::Result;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::catalog::Catalog;
use crate::args::BackendArg;
use crate::config::Config;
use crate::toolset::{ToolRequest, ToolSource, ToolVersionList, ToolVersionOptions};
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
    /// Tools whose requests are not stored because they carry backend options or an
    /// `install_env`; prune keeps every installation of these instead.
    #[serde(default)]
    opaque: Vec<String>,
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

/// Hash of file contents, the same way for the bytes of a file on disk and for the
/// text a config was parsed from.
pub(crate) fn bytes_hash_of(bytes: &[u8]) -> String {
    hash::hash_to_str(&bytes)
}

/// Hash of a file's bytes, or an empty string when it cannot be read.
fn bytes_hash(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| bytes_hash_of(&bytes))
        .unwrap_or_default()
}

fn read(path: &Path) -> Option<Snapshots> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Observations this process has already recorded.
static OBSERVED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// While held, [`observe`] records nothing. Prune holds it while it builds the
/// toolset of the directory it runs in: that is an inspection from whatever
/// environment prune was started in, not a use of the project, and recording it
/// would replace the snapshot prune is about to rely on.
pub struct Suspended;

static SUSPENDED: AtomicUsize = AtomicUsize::new(0);

pub fn suspend() -> Suspended {
    SUSPENDED.fetch_add(1, Ordering::SeqCst);
    Suspended
}

impl Drop for Suspended {
    fn drop(&mut self) {
        SUSPENDED.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The config files a command started resolving tools from, hashed as they were
/// before it resolved anything.
pub(crate) struct Begun {
    files: Vec<(String, String)>,
}

fn hash_files(config: &Config) -> Vec<(String, String)> {
    config
        .config_files
        .keys()
        .map(|p| (p.to_string_lossy().to_string(), bytes_hash(p)))
        .collect()
}

/// Call before resolving tools; `None` when nothing will be recorded.
pub(crate) fn begin(config: &Config) -> Option<Begun> {
    if SUSPENDED.load(Ordering::SeqCst) > 0
        || !config
            .config_files
            .values()
            .any(|cf| cf.has_templated_tool_versions())
    {
        return None;
    }
    let files = hash_files(config);
    // A config that kept the hash of what it parsed tells us its file changed
    // between being loaded and now, which the hashes taken here would hide: the
    // requests that follow come from the text it was loaded with.
    let changed = config.config_files.iter().any(|(path, cf)| {
        cf.loaded_hash().is_some_and(|loaded| {
            files
                .iter()
                .any(|(file, hash)| Path::new(file) == path.as_path() && *hash != loaded)
        })
    });
    if changed {
        debug!("a config file changed after it was loaded; not recording a snapshot");
        return None;
    }
    Some(Begun { files })
}

/// Snapshot, for the context `config` was loaded in, every loaded config that has
/// templated tool versions, from the requests a command just resolved. Nothing is
/// recorded if a config file changed while the command was resolving: the
/// requests then belong to bytes the snapshot would not name.
pub(crate) fn observe(
    config: &Config,
    versions: &IndexMap<Arc<BackendArg>, ToolVersionList>,
    begun: Begun,
) {
    if SUSPENDED.load(Ordering::SeqCst) > 0 {
        return;
    }
    if hash_files(config) != begun.files {
        debug!("config changed while resolving tools; not recording a snapshot");
        return;
    }
    let env = crate::env::MISE_ENV.join(",");
    let mut paths = config
        .config_files
        .keys()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    paths.sort();
    let context = hash::hash_to_str(&paths);
    for (path, cf) in &config.config_files {
        if !cf.has_templated_tool_versions() {
            continue;
        }
        if let Err(err) = observe_config(config, versions, &begun.files, path, &env, &context) {
            warn!(
                "could not record what {} renders to: {err:#}",
                crate::file::display_path(path)
            );
        }
    }
}

/// Whether a request carries options that are not stored. Any backend option, and
/// an `install_env`, can hold a credential under a name or in a shape no list of
/// names would catch (a nested table, a header, a key), so none are written to the
/// catalog; prune keeps every installation of such a tool instead.
fn has_unstored_options(options: &ToolVersionOptions) -> bool {
    !options.opts.values.is_empty() || !options.core.install_env.is_empty()
}

fn observe_config(
    config: &Config,
    versions: &IndexMap<Arc<BackendArg>, ToolVersionList>,
    files: &[(String, String)],
    path: &Path,
    env: &str,
    context: &str,
) -> Result<()> {
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
    let from_config = |request: &&ToolRequest| matches!(request.source(), ToolSource::MiseToml(source) if source == path);
    let mut tools = vec![];
    let mut opaque: Vec<String> = vec![];
    for backend in &templated {
        if replaced.contains(backend) {
            continue;
        }
        let mut seen = false;
        for tvl in versions
            .values()
            .filter(|tvl| &tvl.backend.short == backend)
        {
            for request in tvl.requests.iter().filter(from_config) {
                seen = true;
                let options = request.options();
                if has_unstored_options(&options) {
                    if !opaque.contains(backend) {
                        opaque.push(backend.clone());
                    }
                    continue;
                }
                let requested = request.version();
                // The version the command settled on after aliases and the like
                // is recorded as well: prune rebuilds the request without the
                // project's aliases and could not reach it from the request.
                let resolved = tvl
                    .versions
                    .iter()
                    .find(|tv| tv.request.version() == requested)
                    .map(|tv| tv.version.clone())
                    .filter(|resolved| *resolved != requested);
                tools.push(Recorded {
                    backend: backend.clone(),
                    version: requested,
                    options: options.clone(),
                });
                if let Some(resolved) = resolved {
                    tools.push(Recorded {
                        backend: backend.clone(),
                        version: resolved,
                        options,
                    });
                }
            }
        }
        // A command that did not resolve this tool from this config (an argument,
        // an environment variable, a scoped toolset) cannot vouch for it, and a
        // partial snapshot must not replace a complete one.
        if !seen {
            return Ok(());
        }
    }
    // The same observation again in this process records nothing new.
    let key = format!(
        "{}|{env}|{context}|{}",
        path.display(),
        hash::hash_to_str(&format!("{tools:?}{opaque:?}"))
    );
    if !OBSERVED.lock().unwrap().insert(key) {
        return Ok(());
    }
    let snapshot = Snapshot {
        env: env.to_string(),
        context: context.to_string(),
        files: files.to_vec(),
        tools,
        opaque,
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
    let dir = file.parent().unwrap();
    file::create_dir_all(dir)?;
    // Rendered options can hold values a user would not share, so keep the
    // snapshots to the user.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    file::write_atomic(file, toml::to_string_pretty(&snapshots)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Remove snapshots of configs that no longer exist, and the contexts of the rest
/// that loaded a config file that is gone. `mise prune --configs` runs this next to
/// its cleanup of tracked configs.
pub fn clean() -> Result<()> {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let dir = catalog.meta_dir().join("snapshots");
    if !dir.is_dir() {
        return Ok(());
    }
    let _lock = catalog.lock()?;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let Some(mut snapshots) = read(&path) else {
            continue;
        };
        if !Path::new(&snapshots.config).exists() {
            file::remove_file(&path)?;
            continue;
        }
        let before = snapshots.contexts.len();
        snapshots.contexts.retain(|snapshot| {
            snapshot
                .files
                .iter()
                .all(|(file, _)| Path::new(file).exists())
        });
        if snapshots.contexts.len() != before {
            file::write_atomic(&path, toml::to_string_pretty(&snapshots)?)?;
        }
    }
    Ok(())
}

/// What prune takes from `config`'s snapshots.
#[derive(Debug, Default)]
pub(crate) struct Current {
    /// The requests of every snapshot that is current.
    pub(crate) requests: Vec<ToolRequest>,
    /// Whether some snapshot is stale, or none is current, so the config's
    /// templated tools cannot be told apart and every installation is kept.
    pub(crate) keep_all: bool,
    /// Tools a current snapshot could not store, whose installations are all kept.
    pub(crate) keep_tools: Vec<String>,
}

/// `config`'s snapshots as prune may use them. A snapshot that loaded a file that
/// is gone is retired; one that loaded a file that has changed makes prune keep
/// every installation of the config's templated tools.
pub(crate) fn current(config: &Path, source: &ToolSource) -> Current {
    let catalog = Catalog::new(dirs::INSTALLS.to_path_buf());
    let mut current = Current::default();
    let Some(snapshots) = read(&path_for(&catalog, config)) else {
        current.keep_all = true;
        return current;
    };
    let mut fresh = false;
    for snapshot in snapshots.contexts {
        if snapshot
            .files
            .iter()
            .any(|(path, _)| !Path::new(path).exists())
        {
            continue;
        }
        if !snapshot
            .files
            .iter()
            .all(|(path, hash)| !hash.is_empty() && bytes_hash(Path::new(path)) == *hash)
        {
            current.keep_all = true;
            continue;
        }
        fresh = true;
        for tool in snapshot.opaque {
            if !current.keep_tools.contains(&tool) {
                current.keep_tools.push(tool);
            }
        }
        for tool in snapshot.tools {
            let ba = Arc::new(BackendArg::from(tool.backend.as_str()));
            match ToolRequest::new_with_options(ba, &tool.version, tool.options, source.clone()) {
                Ok(request) => current.requests.push(request),
                // A request that cannot be rebuilt cannot vouch for anything.
                Err(err) => {
                    debug!("snapshot of {}: {err:#}", config.display());
                    current.keep_all = true;
                }
            }
        }
    }
    current.keep_all |= !fresh;
    current
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
                    opaque: vec![],
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
