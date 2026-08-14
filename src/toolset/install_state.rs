use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::git::Git;
use crate::lock_file::LockFile;
use crate::plugins::PluginType;
use crate::toolset::{EPHEMERAL_OPT_KEYS, parse_tool_options};
use crate::{dirs, env, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use versions::Versioning;

/// Normalize a version string for sorting by stripping leading 'v' or 'V' prefix.
/// This ensures "v1.0.0" and "1.0.0" are sorted together correctly.
fn normalize_version_for_sort(v: &str) -> &str {
    v.strip_prefix('v')
        .or_else(|| v.strip_prefix('V'))
        .unwrap_or(v)
}

type InstallStatePlugins = BTreeMap<String, PluginType>;
type InstallStateTools = BTreeMap<String, InstallStateTool>;
type MutexResult<T> = Result<Arc<T>>;

#[derive(Debug, Clone)]
pub struct InstallStateTool {
    pub short: String,
    pub full: Option<String>,
    pub versions: Vec<String>,
    pub explicit_backend: bool,
    pub opts: BTreeMap<String, toml::Value>,
    pub version_backends: BTreeMap<String, String>,
    pub installs_path: Option<PathBuf>,
}

/// Entry in the consolidated manifest file (.mise-installs.toml).
/// Version directories remain the inventory source of truth; `version_backends`
/// only records which backend produced versions that still exist on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestTool {
    /// Original short name (e.g. "github:jdx/mise-test-fixtures").
    /// May differ from the manifest key (which is the kebab-cased dir name).
    short: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    full: Option<String>,
    #[serde(default = "default_true")]
    explicit_backend: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    opts: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    version_backends: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

/// In-memory representation of the manifest keyed by short name.
type Manifest = BTreeMap<String, ManifestTool>;

enum ManifestMigration {
    BracketedOptions {
        expected_full: String,
        full: String,
        opts: BTreeMap<String, toml::Value>,
    },
    Legacy(ManifestTool),
}

type ManifestMigrations = BTreeMap<String, ManifestMigration>;

static INSTALL_STATE_PLUGINS: Mutex<Option<Arc<InstallStatePlugins>>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<Arc<InstallStateTools>>> = Mutex::new(None);
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

fn manifest_path() -> PathBuf {
    dirs::INSTALLS.join(".mise-installs.toml")
}

fn tool_manifest_path(installs_dir: &Path, short: &str) -> PathBuf {
    installs_dir
        .join(short.to_kebab_case())
        .join(".mise.backend.toml")
}

/// Returns the cross-process lock associated with one install manifest.
fn manifest_file_lock(path: &Path) -> LockFile {
    LockFile::new(path)
}

/// Read the consolidated manifest file. Returns empty map if it doesn't exist.
fn read_manifest() -> Manifest {
    read_manifest_from(&manifest_path())
}

fn read_manifest_from(path: &Path) -> Manifest {
    match file::read_to_string(path) {
        std::result::Result::Ok(body) => match toml::from_str(&body) {
            std::result::Result::Ok(m) => m,
            Err(err) => {
                warn!(
                    "failed to parse manifest at {}: {err:#}",
                    display_path(path)
                );
                Default::default()
            }
        },
        Err(_) => Default::default(),
    }
}

fn write_manifest_to(path: &Path, manifest: &Manifest) -> Result<()> {
    let body = toml::to_string_pretty(manifest)?;
    file::write_atomic(path, body.trim())?;
    Ok(())
}

fn read_tool_manifest_from(path: &Path) -> Option<ManifestTool> {
    if !path.exists() {
        return None;
    }
    match file::read_to_string(path) {
        std::result::Result::Ok(body) if body.trim().is_empty() => None,
        std::result::Result::Ok(body) => match toml::from_str(&body) {
            std::result::Result::Ok(m) => Some(m),
            Err(err) => {
                warn!(
                    "failed to parse tool manifest at {}: {err:#}",
                    display_path(path)
                );
                None
            }
        },
        Err(_) => None,
    }
}

/// Merge duplicated install metadata after a partial consolidated/sidecar write.
///
/// The consolidated manifest is written first, so its fields win when both
/// copies exist. The sidecar still contributes version identities missing from
/// the consolidated entry for compatibility with older or copied installs.
fn merge_manifest_tool(
    consolidated: Option<&ManifestTool>,
    sidecar: Option<&ManifestTool>,
) -> Option<ManifestTool> {
    match (consolidated, sidecar) {
        (Some(consolidated), Some(sidecar)) => {
            let mut merged = sidecar.clone();
            merged.short.clone_from(&consolidated.short);
            merged.full.clone_from(&consolidated.full);
            merged.explicit_backend = consolidated.explicit_backend;
            merged.opts.clone_from(&consolidated.opts);
            merged
                .version_backends
                .extend(consolidated.version_backends.clone());
            Some(merged)
        }
        (Some(consolidated), None) => Some(consolidated.clone()),
        (None, Some(sidecar)) => Some(sidecar.clone()),
        (None, None) => None,
    }
}

fn write_tool_manifest_to(path: &Path, tool: &ManifestTool) -> Result<()> {
    let body = toml::to_string_pretty(tool)?;
    file::write_atomic(path, body.trim())?;
    Ok(())
}

/// Applies legacy metadata migrations to the latest manifest under the same
/// process and file locks used by normal install metadata writes.
fn write_manifest_migrations(path: &Path, migrations: ManifestMigrations) -> Result<()> {
    let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
    let _file_lock = manifest_file_lock(path)
        .with_callback(|lock| {
            debug!(
                "waiting for install manifest lock on {}",
                display_path(lock)
            );
        })
        .lock()?;
    let mut manifest = read_manifest_from(path);
    let mut changed = false;
    for (short, migration) in migrations {
        match migration {
            ManifestMigration::BracketedOptions {
                expected_full,
                full,
                opts,
            } => {
                let Some(tool) = manifest.get_mut(&short) else {
                    continue;
                };
                if tool.opts.is_empty() && tool.full.as_deref() == Some(expected_full.as_str()) {
                    tool.full = Some(full);
                    tool.opts = opts;
                    changed = true;
                }
            }
            ManifestMigration::Legacy(tool) => {
                if let std::collections::btree_map::Entry::Vacant(entry) = manifest.entry(short) {
                    entry.insert(tool);
                    changed = true;
                }
            }
        }
    }
    if changed {
        write_manifest_to(path, &manifest)?;
    }
    Ok(())
}

/// Read a legacy `.mise.backend` file for migration purposes.
///
/// Returns `Some((short, full, explicit_backend))` if legacy metadata is found.
fn read_legacy_backend_meta(short: &str) -> Option<(String, Option<String>, bool)> {
    // Try .mise.backend.json first (oldest format)
    let json_path = dirs::INSTALLS.join(short).join(".mise.backend.json");
    if json_path.exists()
        && let std::result::Result::Ok(f) = file::open(&json_path)
        && let std::result::Result::Ok(json) = serde_json::from_reader::<_, serde_json::Value>(f)
    {
        let full = json.get("id").and_then(|id| id.as_str()).map(String::from);
        let s = json
            .get("short")
            .and_then(|s| s.as_str())
            .unwrap_or(short)
            .to_string();
        return Some((s, full, true));
    }

    // Try .mise.backend (text format)
    let path = dirs::INSTALLS
        .join(short.to_kebab_case())
        .join(".mise.backend");
    if !path.exists() {
        return None;
    }
    let body = match file::read_to_string(&path) {
        std::result::Result::Ok(body) => body,
        Err(err) => {
            warn!(
                "failed to read backend meta at {}: {err:?}",
                display_path(&path)
            );
            return None;
        }
    };
    let lines: Vec<&str> = body.lines().filter(|f| !f.is_empty()).collect();
    let s = lines.first().unwrap_or(&short).to_string();
    let full = lines.get(1).map(|f| f.to_string());
    let explicit_backend = lines.get(2).is_some_and(|v| *v == "1");
    Some((s, full, explicit_backend))
}

pub(crate) async fn init() -> Result<()> {
    let (plugins, tools) = tokio::join!(
        tokio::task::spawn(async { measure!("init_plugins", { init_plugins().await }) }),
        tokio::task::spawn(async { measure!("init_tools", { init_tools().await }) }),
    );
    plugins??;
    tools??;
    Ok(())
}

async fn init_plugins() -> MutexResult<InstallStatePlugins> {
    if let Some(plugins) = INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed")
        .clone()
    {
        return Ok(plugins);
    }
    let dirs = file::dir_subdirs(&dirs::PLUGINS)?;
    let plugins: InstallStatePlugins = dirs
        .into_iter()
        .filter_map(|d| {
            time!("init_plugins {d}");
            let path = dirs::PLUGINS.join(&d);
            if is_banned_plugin(&path) {
                info!("removing banned plugin {d}");
                let _ = file::remove_all(&path);
                None
            } else {
                PluginType::from_plugin_path(&path).map(|plugin_type| (d, plugin_type))
            }
        })
        .collect();
    let plugins = Arc::new(plugins);
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = Some(plugins.clone());
    Ok(plugins)
}

async fn init_tools() -> MutexResult<InstallStateTools> {
    if let Some(tools) = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed")
        .clone()
    {
        return Ok(tools);
    }

    // 1. Read manifest (1 syscall)
    let manifest = read_manifest();

    // 2. List install dirs (1 syscall)
    let subdirs = file::dir_subdirs(&dirs::INSTALLS)?;

    // 3. For each dir, read versions from filesystem and merge with manifest metadata.
    //    Record only the legacy entries that need migration. They are merged
    //    into the latest manifest under its cross-process lock after scanning.
    let mut manifest_migrations = ManifestMigrations::new();
    let mut tools = BTreeMap::new();
    for dir_name in subdirs {
        let dir = dirs::INSTALLS.join(&dir_name);
        let tool_manifest = read_tool_manifest_from(&dir.join(".mise.backend.toml"));
        let manifest_tool = merge_manifest_tool(manifest.get(&dir_name), tool_manifest.as_ref());
        let legacy_meta = if manifest_tool.is_none() {
            read_legacy_backend_meta(&dir_name)
        } else {
            None
        };
        // Read versions from filesystem (1 syscall per tool — unavoidable)
        let versions: Vec<String> = file::dir_subdirs(&dir)
            .unwrap_or_else(|err| {
                warn!("reading versions in {} failed: {err:?}", display_path(&dir));
                Default::default()
            })
            .into_iter()
            .filter(|v| !v.starts_with('.'))
            .filter(|v| !runtime_symlinks::is_runtime_symlink(&dir.join(v)))
            .filter(|v| !dir.join(v).join("incomplete").exists())
            .sorted_by_cached_key(|v| {
                let normalized = normalize_version_for_sort(v);
                (Versioning::new(normalized), v.to_string())
            })
            .collect();

        if versions.is_empty() {
            continue;
        }

        // Get metadata: prefer manifest, fall back to legacy .mise.backend
        let (short, full, explicit_backend, opts) = if let Some(mt) = manifest_tool.as_ref() {
            let mut full = mt.full.clone();
            let mut opts = mt.opts.clone();
            // Backward compat: if opts is empty but full contains [...], extract opts
            if opts.is_empty()
                && let Some(ref f) = full
                && let Some((stripped_str, opts_str)) = crate::cli::args::split_bracketed_opts(f)
            {
                let expected_full = f.clone();
                let stripped = stripped_str.to_string();
                let parsed = parse_tool_options(opts_str);
                for (k, v) in &parsed.opts {
                    if EPHEMERAL_OPT_KEYS.contains(&k.as_str()) {
                        continue;
                    }
                    opts.insert(k.clone(), v.clone());
                }
                full = Some(stripped.clone());
                // Schedule a conditional field migration. If another process
                // updates this entry first, its newer metadata wins.
                manifest_migrations.insert(
                    dir_name.clone(),
                    ManifestMigration::BracketedOptions {
                        expected_full,
                        full: stripped,
                        opts: opts.clone(),
                    },
                );
            }
            (mt.short.clone(), full, mt.explicit_backend, opts)
        } else if let Some((s, full, explicit)) = legacy_meta {
            // Migration: absorb into manifest (clone on first migration)
            manifest_migrations.insert(
                dir_name.clone(),
                ManifestMigration::Legacy(ManifestTool {
                    short: s.clone(),
                    full: full.clone(),
                    explicit_backend: explicit,
                    opts: BTreeMap::new(),
                    version_backends: BTreeMap::new(),
                }),
            );
            (s, full, explicit, BTreeMap::new())
        } else {
            (dir_name.clone(), None, true, BTreeMap::new())
        };

        let version_backends = versions
            .iter()
            .filter_map(|version| {
                let backend = manifest_tool
                    .as_ref()
                    .and_then(|tool| tool.version_backends.get(version).cloned())
                    .or_else(|| full.clone())?;
                Some((version.clone(), backend))
            })
            .collect();
        let tool = InstallStateTool {
            short: short.clone(),
            full,
            versions,
            explicit_backend,
            opts,
            version_backends,
            installs_path: Some(dir),
        };
        time!("init_tools {short}");
        tools.insert(short, tool);
    }

    // Write updated manifest if we migrated any legacy entries
    if !manifest_migrations.is_empty()
        && let Err(err) = write_manifest_migrations(&manifest_path(), manifest_migrations)
    {
        warn!("failed to write install manifest: {err:#}");
    }

    // Scan shared install directories (read-only fallback directories)
    for shared_dir in env::shared_install_dirs_early() {
        if !shared_dir.is_dir() {
            continue;
        }
        let shared_manifest_path = shared_dir.join(".mise-installs.toml");
        let shared_manifest = read_manifest_from(&shared_manifest_path);
        let shared_subdirs = match file::dir_subdirs(&shared_dir) {
            std::result::Result::Ok(d) => d,
            Err(err) => {
                warn!(
                    "reading shared install dir {} failed: {err:?}",
                    display_path(&shared_dir)
                );
                continue;
            }
        };
        for dir_name in shared_subdirs {
            let dir = shared_dir.join(&dir_name);
            let tool_manifest = read_tool_manifest_from(&dir.join(".mise.backend.toml"));
            let manifest_tool =
                merge_manifest_tool(shared_manifest.get(&dir_name), tool_manifest.as_ref());
            let versions: Vec<String> = file::dir_subdirs(&dir)
                .unwrap_or_else(|err| {
                    warn!("reading versions in {} failed: {err:?}", display_path(&dir));
                    Default::default()
                })
                .into_iter()
                .filter(|v| !v.starts_with('.'))
                .filter(|v| !runtime_symlinks::is_runtime_symlink(&dir.join(v)))
                .filter(|v| !dir.join(v).join("incomplete").exists())
                .sorted_by_cached_key(|v| {
                    let normalized = normalize_version_for_sort(v);
                    (Versioning::new(normalized), v.to_string())
                })
                .collect();

            if versions.is_empty() {
                continue;
            }

            let (short, full, explicit_backend, opts) = if let Some(mt) = manifest_tool.as_ref() {
                (
                    mt.short.clone(),
                    mt.full.clone(),
                    mt.explicit_backend,
                    mt.opts.clone(),
                )
            } else {
                (dir_name.clone(), None, true, BTreeMap::new())
            };

            // Merge with existing tool entry or create new one
            let tool = tools
                .entry(short.clone())
                .or_insert_with(|| InstallStateTool {
                    short: short.clone(),
                    full: full.clone(),
                    versions: Vec::new(),
                    explicit_backend,
                    opts: opts.clone(),
                    version_backends: BTreeMap::new(),
                    installs_path: Some(dir),
                });
            // Add versions from shared dir that aren't already present
            for v in versions {
                if !tool.versions.contains(&v) {
                    if let Some(backend) = manifest_tool
                        .as_ref()
                        .and_then(|mt| mt.version_backends.get(&v).cloned())
                        .or_else(|| full.clone())
                    {
                        tool.version_backends.insert(v.clone(), backend);
                    }
                    tool.versions.push(v);
                }
            }
            // Re-sort after merging
            tool.versions.sort_by_cached_key(|v| {
                let normalized = normalize_version_for_sort(v);
                (Versioning::new(normalized), v.to_string())
            });
            // Fill in metadata if not yet set
            if tool.full.is_none() {
                tool.full = full;
            }
        }
    }

    for (short, pt) in init_plugins().await?.iter() {
        let full = match pt {
            PluginType::Asdf => format!("asdf:{short}"),
            PluginType::Vfox => format!("vfox:{short}"),
            PluginType::VfoxBackend => short.clone(),
            PluginType::Package => continue,
        };
        let tool = tools
            .entry(short.clone())
            .or_insert_with(|| InstallStateTool {
                short: short.clone(),
                full: Some(full.clone()),
                versions: Default::default(),
                explicit_backend: true,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::new(),
                installs_path: None,
            });
        tool.full = Some(full);
    }
    let tools = Arc::new(tools);
    *INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed") = Some(tools.clone());
    Ok(tools)
}

pub fn list_plugins() -> Arc<BTreeMap<String, PluginType>> {
    try_list_plugins().expect("INSTALL_STATE_PLUGINS is None")
}

pub fn try_list_plugins() -> Option<Arc<BTreeMap<String, PluginType>>> {
    INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed")
        .as_ref()
        .cloned()
}

fn is_banned_plugin(path: &Path) -> bool {
    if path.ends_with("gradle") {
        let repo = Git::new(path);
        if let Some(url) = repo.get_remote_url() {
            return url == "https://github.com/rfrancis/asdf-gradle.git";
        }
    }
    false
}

pub fn get_tool_full(short: &str) -> Option<String> {
    list_tools().get(short).and_then(|t| t.full.clone())
}

/// Returns the backend that installed a concrete version, falling back to the
/// tool-level backend recorded by legacy manifests.
pub fn get_version_backend(short: &str, version: &str) -> Option<String> {
    try_list_tools()?.get(short).and_then(|tool| {
        tool.version_backends
            .get(version)
            .cloned()
            .or_else(|| tool.full.clone())
    })
}

pub fn get_plugin_type(short: &str) -> Option<PluginType> {
    list_plugins().get(short).cloned()
}

pub fn list_tools() -> Arc<BTreeMap<String, InstallStateTool>> {
    try_list_tools().expect("INSTALL_STATE_TOOLS is None")
}

/// Non-panicking counterpart to [`list_tools`], mirroring [`try_list_plugins`].
/// Callers on error paths must not panic just because install state was never
/// initialized.
pub fn try_list_tools() -> Option<Arc<BTreeMap<String, InstallStateTool>>> {
    INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed")
        .as_ref()
        .cloned()
}

pub fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let backend_type = list_tools()
        .get(short)
        .and_then(|ist| ist.full.as_ref())
        .and_then(|full| {
            full.split_once(':')
                .map(|(backend, _)| BackendType::guess(backend))
        });
    if let Some(BackendType::Unknown) = backend_type
        && let Some((plugin_name, _)) = short.split_once(':')
        && let Some(PluginType::VfoxBackend) = get_plugin_type(plugin_name)
    {
        return Ok(Some(BackendType::VfoxBackend(plugin_name.to_string())));
    }
    Ok(backend_type)
}

pub fn list_versions(short: &str) -> Vec<String> {
    list_tools()
        .get(short)
        .map(|tool| tool.versions.clone())
        .unwrap_or_default()
}

pub fn add_tool_version(ba: &BackendArg, install_path: &Path, version: &str) {
    let tool_dir = install_path.parent().map(Path::to_path_buf);
    let full = ba.full_without_opts();
    let explicit_backend = ba.has_explicit_backend();
    let opts = persistent_opts(ba);

    let mut tools = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed");
    let Some(existing_tools) = tools.as_ref() else {
        return;
    };

    let mut next_tools = existing_tools.deref().clone();
    let tool = next_tools
        .entry(ba.short.clone())
        .or_insert_with(|| InstallStateTool {
            short: ba.short.clone(),
            full: Some(full.clone()),
            versions: Vec::new(),
            explicit_backend,
            opts: opts.clone(),
            version_backends: BTreeMap::new(),
            installs_path: tool_dir.clone(),
        });

    tool.full = Some(full.clone());
    tool.explicit_backend = explicit_backend;
    tool.opts = opts;
    if tool.installs_path.is_none() {
        tool.installs_path = tool_dir;
    }
    if !tool.versions.iter().any(|v| v == version) {
        // Do not sort here: this version has just been resolved by the backend
        // for this install run, and offline dependency env resolution should
        // see that concrete result without adding another ordering rule.
        tool.versions.push(version.to_string());
    }
    tool.version_backends.insert(version.to_string(), full);

    *tools = Some(Arc::new(next_tools));
}

pub async fn add_plugin(short: &str, plugin_type: PluginType) -> Result<()> {
    let mut plugins = init_plugins().await?.deref().clone();
    plugins.insert(short.to_string(), plugin_type);
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = Some(Arc::new(plugins));
    Ok(())
}

/// Writes backend metadata to the consolidated manifest file.
/// Uses the primary installs dir manifest by default.
pub fn write_backend_meta(ba: &BackendArg, version: &str) -> Result<()> {
    write_backend_meta_to(ba, &manifest_path(), version)
}

/// Writes backend metadata to a manifest at a specific install path.
pub fn write_backend_meta_to(ba: &BackendArg, path: &Path, version: &str) -> Result<()> {
    let full = ba.full_without_opts();
    let explicit = ba.has_explicit_backend();
    let opts_map = persistent_opts(ba);

    let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
    let _file_lock = manifest_file_lock(path)
        .with_callback(|lock| {
            debug!(
                "waiting for install manifest lock on {}",
                display_path(lock)
            );
        })
        .lock()?;
    let mut manifest = read_manifest_from(path);
    let tool_manifest = path
        .parent()
        .map(|installs_dir| tool_manifest_path(installs_dir, &ba.short));
    let sidecar = tool_manifest.as_deref().and_then(read_tool_manifest_from);
    let previous = merge_manifest_tool(manifest.get(&ba.short.to_kebab_case()), sidecar.as_ref());
    let mut version_backends = previous
        .as_ref()
        .map(|tool| tool.version_backends.clone())
        .unwrap_or_default();
    if let Some(installs_dir) = path.parent() {
        let tool_dir = installs_dir.join(ba.short.to_kebab_case());
        let existing_versions: BTreeSet<_> = file::dir_subdirs(&tool_dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|existing_version| !existing_version.starts_with('.'))
            .filter(|existing_version| {
                !runtime_symlinks::is_runtime_symlink(&tool_dir.join(existing_version))
            })
            .filter(|existing_version| !tool_dir.join(existing_version).join("incomplete").exists())
            .collect();
        version_backends.retain(|existing_version, _| existing_versions.contains(existing_version));
        if let Some(previous_full) = previous.as_ref().and_then(|tool| tool.full.clone()) {
            for existing_version in existing_versions {
                version_backends
                    .entry(existing_version)
                    .or_insert_with(|| previous_full.clone());
            }
        }
    }
    version_backends.insert(version.to_string(), full.clone());
    let manifest_tool = ManifestTool {
        short: ba.short.clone(),
        full: Some(full),
        explicit_backend: explicit,
        opts: opts_map,
        version_backends,
    };
    manifest.insert(ba.short.to_kebab_case(), manifest_tool.clone());
    write_manifest_to(path, &manifest)?;
    if let Some(tool_manifest) = tool_manifest
        && tool_manifest.parent().is_some_and(|p| p.exists())
    {
        write_tool_manifest_to(&tool_manifest, &manifest_tool)?;
    }
    Ok(())
}

fn persistent_opts(ba: &BackendArg) -> BTreeMap<String, toml::Value> {
    // Store opts as native TOML values, filtering out ephemeral keys.
    let mut opts_map: BTreeMap<String, toml::Value> = BTreeMap::new();
    if let Some(o) = ba.opts.as_ref() {
        for (k, v) in &o.opts {
            if !EPHEMERAL_OPT_KEYS.contains(&k.as_str()) {
                opts_map.insert(k.clone(), v.clone());
            }
        }
    }
    opts_map
}

pub fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
    dirs::CACHE
        .join(short.to_kebab_case())
        .join(v)
        .join("incomplete")
}

fn tool_version_lock(short: &str, v: &str) -> LockFile {
    LockFile::new(&incomplete_file_path(short, v))
}

/// Acquires the transaction lock for one logical tool version.
///
/// The incomplete marker is shared by local, shared, and system install paths,
/// so install, uninstall, and link use this logical identity while mutating the
/// marker and install path. The marker path is only the lock identity; the
/// lock itself remains a separate stable file under the lockfiles cache.
pub(crate) fn lock_tool_version(short: &str, v: &str) -> Result<fslock::LockFile> {
    tool_version_lock(short, v)
        .with_callback(|lock| {
            debug!("waiting for tool-version lock on {}", display_path(lock));
        })
        .lock()
}

pub fn clear_incomplete_marker(short: &str, v: &str) -> Result<()> {
    let incomplete_path = incomplete_file_path(short, v);
    match file::remove_file(&incomplete_path) {
        std::result::Result::Ok(()) => {
            if let Some(parent) = incomplete_path.parent()
                && let Err(err) = file::sync_dir(parent)
            {
                debug!("error syncing incomplete marker parent: {:?}", err);
            }
            Ok(())
        }
        Err(err)
            if err
                .downcast_ref::<std::io::Error>()
                .is_some_and(|err| err.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(())
        }
        Err(err) => Err(err),
    }
}

pub fn clear_incomplete_marker_best_effort(short: &str, v: &str) {
    if let Err(err) = clear_incomplete_marker(short, v) {
        debug!("error clearing incomplete marker: {:?}", err);
    }
}

/// Path to the checksum file for a specific tool version.
/// Used to track changes in rolling releases (like "nightly").
fn checksum_file_path(install_path: &Path) -> PathBuf {
    install_path.join(".mise.checksum")
}

/// Store the checksum for a tool version (used for rolling release tracking)
pub fn write_checksum(install_path: &Path, checksum: &str) -> Result<()> {
    let path = checksum_file_path(install_path);
    file::write(&path, checksum)?;
    Ok(())
}

/// Read the stored checksum for a tool version
pub fn read_checksum(install_path: &Path) -> Option<String> {
    let path = checksum_file_path(install_path);
    if path.exists() {
        file::read_to_string(&path).ok()
    } else {
        None
    }
}

pub fn reset() {
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = None;
    *INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed") = None;
    super::tool_version::reset_install_path_cache();
}

#[cfg(test)]
mod tests {
    use super::{
        Manifest, ManifestMigration, ManifestMigrations, ManifestTool, lock_tool_version,
        manifest_file_lock, merge_manifest_tool, normalize_version_for_sort, read_manifest_from,
        read_tool_manifest_from, tool_version_lock, write_backend_meta_to,
        write_manifest_migrations, write_manifest_to,
    };
    use crate::cli::args::BackendArg;
    use itertools::Itertools;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;
    use versions::Versioning;

    #[test]
    fn empty_tool_manifest_is_ignored() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".mise.backend.toml");
        std::fs::write(&path, "").unwrap();

        assert!(read_tool_manifest_from(&path).is_none());
    }

    #[test]
    fn tool_version_locks_serialize_logical_versions() {
        let short = format!("lock_test_{}", std::process::id());
        let first = lock_tool_version(&short, "1.0.0").unwrap();
        let (waiting_tx, waiting_rx) = mpsc::channel();
        let (acquired_tx, acquired_rx) = mpsc::channel();
        let thread_short = short.replace('_', "-");
        let waiter = std::thread::spawn(move || {
            let lock = tool_version_lock(&thread_short, "1.0.0")
                .with_callback(move |_| waiting_tx.send(()).unwrap())
                .lock()
                .unwrap();
            acquired_tx.send(()).unwrap();
            drop(lock);
        });

        // The callback proves the second lock reached the contended lock rather
        // than merely losing a scheduling race with this assertion.
        waiting_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            acquired_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));

        // A different logical version is independent even while the first is held.
        let other = lock_tool_version(&short, "2.0.0").unwrap();
        drop(other);
        drop(first);

        acquired_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
    }

    #[test]
    fn manifest_file_locks_serialize_across_processes() {
        const CHILD_PATH: &str = "MISE_TEST_MANIFEST_LOCK_PATH";
        const WAITING_PATH: &str = "MISE_TEST_MANIFEST_LOCK_WAITING_PATH";
        const ACQUIRED_PATH: &str = "MISE_TEST_MANIFEST_LOCK_ACQUIRED_PATH";

        if let (Some(lock_path), Some(waiting_path), Some(acquired_path)) = (
            std::env::var_os(CHILD_PATH),
            std::env::var_os(WAITING_PATH),
            std::env::var_os(ACQUIRED_PATH),
        ) {
            let waiting_path = PathBuf::from(waiting_path);
            let _lock = manifest_file_lock(Path::new(&lock_path))
                .with_callback(move |_| std::fs::write(&waiting_path, "waiting").unwrap())
                .lock()
                .unwrap();
            std::fs::write(acquired_path, "acquired").unwrap();
            return;
        }

        let tempdir = tempfile::tempdir().unwrap();
        let manifest_path = tempdir.path().join(".mise-installs.toml");
        let waiting_path = tempdir.path().join("waiting");
        let acquired_path = tempdir.path().join("acquired");
        let lock = manifest_file_lock(&manifest_path).lock().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("manifest_file_locks_serialize_across_processes")
            .env("MISE_TEST_SKIP_INIT", "1")
            .env(CHILD_PATH, &manifest_path)
            .env(WAITING_PATH, &waiting_path)
            .env(ACQUIRED_PATH, &acquired_path)
            .spawn()
            .unwrap();

        for _ in 0..500 {
            if waiting_path.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(waiting_path.exists(), "child did not contend for the lock");
        assert!(!acquired_path.exists());

        drop(lock);
        assert!(child.wait().unwrap().success());
        assert!(acquired_path.exists());
    }

    #[test]
    fn test_normalize_version_for_sort() {
        assert_eq!(normalize_version_for_sort("v1.0.0"), "1.0.0");
        assert_eq!(normalize_version_for_sort("V1.0.0"), "1.0.0");
        assert_eq!(normalize_version_for_sort("1.0.0"), "1.0.0");
        assert_eq!(normalize_version_for_sort("latest"), "latest");
    }

    #[test]
    fn test_version_sorting_with_v_prefix() {
        // Test that mixed v-prefix and non-v-prefix versions sort correctly
        let versions = ["v2.0.51", "2.0.35", "2.0.52"];

        // Without normalization - demonstrates the problem
        let sorted_without_norm: Vec<_> = versions
            .iter()
            .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
            .collect();
        println!("Without normalization: {:?}", sorted_without_norm);

        // With normalization - the fix
        let sorted_with_norm: Vec<_> = versions
            .iter()
            .sorted_by_cached_key(|v| {
                let normalized = normalize_version_for_sort(v);
                (Versioning::new(normalized), v.to_string())
            })
            .collect();
        println!("With normalization: {:?}", sorted_with_norm);

        // With the fix, v2.0.51 should sort between 2.0.35 and 2.0.52
        // The highest version should be 2.0.52
        assert_eq!(**sorted_with_norm.last().unwrap(), "2.0.52");

        // v2.0.51 should be second to last
        assert_eq!(**sorted_with_norm.get(1).unwrap(), "v2.0.51");

        // 2.0.35 should be first
        assert_eq!(**sorted_with_norm.first().unwrap(), "2.0.35");
    }

    #[test]
    fn test_manifest_roundtrip() {
        use super::{Manifest, ManifestTool};

        let mut manifest = Manifest::new();
        manifest.insert(
            "node".to_string(),
            ManifestTool {
                short: "node".to_string(),
                full: Some("core:node".to_string()),
                explicit_backend: true,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::from([("22.0.0".to_string(), "core:node".to_string())]),
            },
        );
        manifest.insert(
            "bun".to_string(),
            ManifestTool {
                short: "bun".to_string(),
                full: Some("aqua:oven-sh/bun".to_string()),
                explicit_backend: false,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::new(),
            },
        );
        manifest.insert(
            "tiny".to_string(),
            ManifestTool {
                short: "tiny".to_string(),
                full: None,
                explicit_backend: true,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::new(),
            },
        );

        let serialized = toml::to_string_pretty(&manifest).unwrap();
        let deserialized: Manifest = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.len(), 3);
        assert_eq!(deserialized["node"].full.as_deref(), Some("core:node"));
        assert!(deserialized["node"].explicit_backend);
        assert_eq!(
            deserialized["node"].version_backends.get("22.0.0"),
            Some(&"core:node".to_string())
        );
        assert_eq!(
            deserialized["bun"].full.as_deref(),
            Some("aqua:oven-sh/bun")
        );
        assert!(!deserialized["bun"].explicit_backend);
        assert!(deserialized["tiny"].full.is_none());
        assert!(deserialized["tiny"].explicit_backend);
    }

    #[test]
    fn test_manifest_with_opts_roundtrip() {
        use super::{Manifest, ManifestTool};

        let mut opts = BTreeMap::new();
        opts.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/tool.tar.gz".to_string()),
        );
        opts.insert(
            "bin_path".to_string(),
            toml::Value::String("bin".to_string()),
        );

        // Nested table for platforms
        let mut platforms = toml::map::Map::new();
        let mut linux = toml::map::Map::new();
        linux.insert(
            "url".to_string(),
            toml::Value::String("https://example.com/linux.tar.gz".to_string()),
        );
        platforms.insert("linux-x64".to_string(), toml::Value::Table(linux));
        opts.insert("platforms".to_string(), toml::Value::Table(platforms));

        let mut manifest = Manifest::new();
        manifest.insert(
            "hello".to_string(),
            ManifestTool {
                short: "hello".to_string(),
                full: Some("http:hello".to_string()),
                explicit_backend: true,
                opts,
                version_backends: BTreeMap::new(),
            },
        );

        let serialized = toml::to_string_pretty(&manifest).unwrap();
        let deserialized: Manifest = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized["hello"].full.as_deref(), Some("http:hello"));
        assert_eq!(
            deserialized["hello"].opts.get("url"),
            Some(&toml::Value::String(
                "https://example.com/tool.tar.gz".to_string()
            ))
        );
        assert_eq!(
            deserialized["hello"].opts.get("bin_path"),
            Some(&toml::Value::String("bin".to_string()))
        );
        // Verify nested platforms table survived round-trip
        let platforms = deserialized["hello"].opts.get("platforms").unwrap();
        assert!(platforms.is_table());
        let linux = platforms.get("linux-x64").unwrap();
        assert_eq!(
            linux.get("url").unwrap().as_str(),
            Some("https://example.com/linux.tar.gz")
        );
    }

    #[test]
    fn test_manifest_backward_compat_bracketed_full() {
        use super::Manifest;

        // Old format: full contains bracketed opts
        let toml_str = r#"
[hello]
short = "hello"
full = "http:hello[url = \"https://example.com/tool.tar.gz\", bin_path = \"bin\"]"
explicit_backend = true
"#;
        let manifest: Manifest = toml::from_str(toml_str).unwrap();
        let mt = &manifest["hello"];
        // Old format should deserialize with opts empty and brackets in full
        assert!(mt.opts.is_empty());
        assert!(mt.version_backends.is_empty());
        assert!(mt.full.as_ref().unwrap().contains('['));
    }

    #[test]
    fn manifest_migrations_preserve_concurrent_updates() {
        let tempdir = tempfile::tempdir().unwrap();
        let manifest_path = tempdir.path().join(".mise-installs.toml");
        let mut current = Manifest::new();
        current.insert(
            "bracketed".into(),
            ManifestTool {
                short: "bracketed".into(),
                full: Some("http:bracketed[url = \"https://example.com\"]".into()),
                explicit_backend: true,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::from([(
                    "1.0.0".into(),
                    "http:concurrent-backend".into(),
                )]),
            },
        );
        current.insert(
            "legacy".into(),
            ManifestTool {
                short: "legacy".into(),
                full: Some("http:current-backend".into()),
                explicit_backend: true,
                opts: BTreeMap::new(),
                version_backends: BTreeMap::from([("2.0.0".into(), "http:current-backend".into())]),
            },
        );
        write_manifest_to(&manifest_path, &current).unwrap();

        let migrations = ManifestMigrations::from([
            (
                "bracketed".into(),
                ManifestMigration::BracketedOptions {
                    expected_full: "http:bracketed[url = \"https://example.com\"]".into(),
                    full: "http:bracketed".into(),
                    opts: BTreeMap::from([(
                        "url".into(),
                        toml::Value::String("https://example.com".into()),
                    )]),
                },
            ),
            (
                "legacy".into(),
                ManifestMigration::Legacy(ManifestTool {
                    short: "legacy".into(),
                    full: Some("asdf:stale-backend".into()),
                    explicit_backend: true,
                    opts: BTreeMap::new(),
                    version_backends: BTreeMap::new(),
                }),
            ),
        ]);
        write_manifest_migrations(&manifest_path, migrations).unwrap();

        let manifest = read_manifest_from(&manifest_path);
        assert_eq!(
            manifest["bracketed"].full.as_deref(),
            Some("http:bracketed")
        );
        assert_eq!(
            manifest["bracketed"]
                .version_backends
                .get("1.0.0")
                .map(String::as_str),
            Some("http:concurrent-backend")
        );
        assert_eq!(
            manifest["legacy"].full.as_deref(),
            Some("http:current-backend")
        );
        assert_eq!(
            manifest["legacy"]
                .version_backends
                .get("2.0.0")
                .map(String::as_str),
            Some("http:current-backend")
        );
    }

    #[test]
    fn backend_change_preserves_other_version_identities() {
        let tempdir = tempfile::tempdir().unwrap();
        let installs_dir = tempdir.path().join("installs");
        let tool_dir = installs_dir.join("bd");
        std::fs::create_dir_all(tool_dir.join("1.0.0")).unwrap();
        std::fs::create_dir_all(tool_dir.join("2.0.0")).unwrap();
        let manifest_path = installs_dir.join(".mise-installs.toml");

        let old_backend = BackendArg::new("bd".into(), Some("asdf:backend-a".into()));
        write_backend_meta_to(&old_backend, &manifest_path, "1.0.0").unwrap();

        let new_backend = BackendArg::new("bd".into(), Some("asdf:backend-b".into()));
        write_backend_meta_to(&new_backend, &manifest_path, "1.0.0").unwrap();

        let manifest = read_manifest_from(&manifest_path);
        let tool = &manifest["bd"];
        assert_eq!(tool.full.as_deref(), Some("asdf:backend-b"));
        assert_eq!(
            tool.version_backends.get("1.0.0").map(String::as_str),
            Some("asdf:backend-b")
        );
        assert_eq!(
            tool.version_backends.get("2.0.0").map(String::as_str),
            Some("asdf:backend-a")
        );

        let sidecar = read_tool_manifest_from(&tool_dir.join(".mise.backend.toml")).unwrap();
        assert_eq!(sidecar.version_backends, tool.version_backends);
    }

    #[test]
    fn backend_meta_recovers_stale_sidecar_after_partial_write() {
        let tempdir = tempfile::tempdir().unwrap();
        let installs_dir = tempdir.path().join("installs");
        let tool_dir = installs_dir.join("bd");
        std::fs::create_dir_all(tool_dir.join("1.0.0")).unwrap();
        std::fs::create_dir_all(tool_dir.join("2.0.0")).unwrap();
        let manifest_path = installs_dir.join(".mise-installs.toml");
        let old_backend = BackendArg::new("bd".into(), Some("asdf:backend-a".into()));
        let new_backend = BackendArg::new("bd".into(), Some("asdf:backend-b".into()));

        write_backend_meta_to(&old_backend, &manifest_path, "1.0.0").unwrap();
        write_backend_meta_to(&old_backend, &manifest_path, "2.0.0").unwrap();

        // Simulate termination after the consolidated manifest was committed
        // for 2.0.0 but before its stale sidecar could be replaced.
        let mut interrupted_manifest = read_manifest_from(&manifest_path);
        let interrupted_tool = interrupted_manifest.get_mut("bd").unwrap();
        interrupted_tool.full = Some("asdf:backend-b".into());
        interrupted_tool
            .version_backends
            .insert("2.0.0".into(), "asdf:backend-b".into());
        write_manifest_to(&manifest_path, &interrupted_manifest).unwrap();

        let stale_sidecar = read_tool_manifest_from(&tool_dir.join(".mise.backend.toml")).unwrap();
        let recovered =
            merge_manifest_tool(interrupted_manifest.get("bd"), Some(&stale_sidecar)).unwrap();
        assert_eq!(
            recovered.version_backends.get("2.0.0").map(String::as_str),
            Some("asdf:backend-b")
        );

        write_backend_meta_to(&new_backend, &manifest_path, "2.0.0").unwrap();

        let manifest = read_manifest_from(&manifest_path);
        assert_eq!(
            manifest["bd"]
                .version_backends
                .get("1.0.0")
                .map(String::as_str),
            Some("asdf:backend-a")
        );
        assert_eq!(
            manifest["bd"]
                .version_backends
                .get("2.0.0")
                .map(String::as_str),
            Some("asdf:backend-b")
        );
        let healed_sidecar = read_tool_manifest_from(&tool_dir.join(".mise.backend.toml")).unwrap();
        assert_eq!(
            healed_sidecar.version_backends,
            manifest["bd"].version_backends
        );
    }

    #[test]
    fn backend_meta_prunes_deleted_version_identities() {
        let tempdir = tempfile::tempdir().unwrap();
        let installs_dir = tempdir.path().join("installs");
        let tool_dir = installs_dir.join("bd");
        std::fs::create_dir_all(tool_dir.join("1.0.0")).unwrap();
        std::fs::create_dir_all(tool_dir.join("2.0.0")).unwrap();
        let manifest_path = installs_dir.join(".mise-installs.toml");
        let backend = BackendArg::new("bd".into(), Some("asdf:backend-a".into()));

        write_backend_meta_to(&backend, &manifest_path, "1.0.0").unwrap();
        write_backend_meta_to(&backend, &manifest_path, "2.0.0").unwrap();
        assert!(
            read_manifest_from(&manifest_path)["bd"]
                .version_backends
                .contains_key("2.0.0")
        );

        std::fs::remove_dir_all(tool_dir.join("2.0.0")).unwrap();
        write_backend_meta_to(&backend, &manifest_path, "1.0.0").unwrap();

        let manifest = read_manifest_from(&manifest_path);
        assert!(!manifest["bd"].version_backends.contains_key("2.0.0"));
        let sidecar = read_tool_manifest_from(&tool_dir.join(".mise.backend.toml")).unwrap();
        assert!(!sidecar.version_backends.contains_key("2.0.0"));
    }
}
