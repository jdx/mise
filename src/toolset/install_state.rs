use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::git::Git;
use crate::lock_file::LockFile;
use crate::plugins::PluginType;
use crate::toolset::{EPHEMERAL_OPT_KEYS, parse_tool_options};
use crate::{dirs, env, file, runtime_symlinks};
use eyre::{Ok, Result, WrapErr};
use heck::ToKebabCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
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
pub(crate) struct InstallStateTool {
    pub short: String,
    pub full: Option<String>,
    pub versions: Vec<String>,
    pub explicit_backend: bool,
    pub opts: BTreeMap<String, toml::Value>,
    pub installs_path: Option<PathBuf>,
}

/// Entry in the consolidated manifest file (.mise-installs.toml).
/// Versions are NOT stored here — they come from the filesystem.
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
}

fn default_true() -> bool {
    true
}

/// In-memory representation of the manifest keyed by short name.
type Manifest = BTreeMap<String, ManifestTool>;

static INSTALL_STATE_PLUGINS: Mutex<Option<Arc<InstallStatePlugins>>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<Arc<InstallStateTools>>> = Mutex::new(None);
/// Per-tool results loaded without a full installs scan. `None` records a
/// known-absent tool so repeated lookups of uninstalled tools stay cheap.
/// Superseded by INSTALL_STATE_TOOLS once a full scan has run.
static INSTALL_STATE_TOOL_MEMO: Mutex<Option<HashMap<String, Option<InstallStateTool>>>> =
    Mutex::new(None);
/// Memoized read of the consolidated root manifest, for per-tool lookups.
static ROOT_MANIFEST_MEMO: Mutex<Option<Arc<Manifest>>> = Mutex::new(None);
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

fn manifest_path() -> PathBuf {
    dirs::INSTALLS.join(".mise-installs.toml")
}

fn tool_manifest_path(installs_dir: &Path, short: &str) -> PathBuf {
    installs_dir
        .join(short.to_kebab_case())
        .join(".mise.backend.toml")
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

/// Write the consolidated manifest file.
fn write_manifest(manifest: &Manifest) -> Result<()> {
    write_manifest_to(&manifest_path(), manifest)
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

fn write_tool_manifest_to(path: &Path, tool: &ManifestTool) -> Result<()> {
    let body = toml::to_string_pretty(tool)?;
    file::write_atomic(path, body.trim())?;
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

/// Initialize plugin state. Tool state is deliberately NOT loaded here: a full
/// scan of the installs dir costs a readdir per tool plus stats per version,
/// which dominated startup on machines with many tools installed. Per-tool
/// lookups load lazily via [`get_tool`]; enumerating callers get the full scan
/// on first use of [`list_tools`].
pub(crate) async fn init() -> Result<()> {
    measure!("init_plugins", { init_plugins().await })?;
    Ok(())
}

async fn init_plugins() -> MutexResult<InstallStatePlugins> {
    load_plugins()
}

fn load_plugins() -> MutexResult<InstallStatePlugins> {
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

/// Versions present in a tool's install dir, sorted. One readdir plus stats
/// per version.
///
/// An unreadable dir is an error, not an empty list: dropping the tool would
/// make an incomplete scan look like a complete one, and shim rebuilding
/// deletes every shim it cannot account for. Callers that are explicitly
/// best-effort (read-only shared dirs) downgrade this to a warning.
fn scan_versions(dir: &Path) -> Result<Vec<String>> {
    // Keeping the links that lead nowhere. `mise link` leaves one behind as soon as its target is
    // moved or deleted, and dropping it here is what made the version invisible to `mise ls` and
    // unreachable to `mise uninstall` — occupying a name nothing would admit to. It is listed, not
    // treated as installed: `is_version_installed` still resolves the path and still says no.
    Ok(file::dir_subdirs_keeping_broken_links(dir)?
        .into_iter()
        .filter(|v| !v.starts_with('.'))
        .filter(|v| !runtime_symlinks::is_runtime_symlink(&dir.join(v)))
        .filter(|v| !dir.join(v).join("incomplete").exists())
        .sorted_by_cached_key(|v| {
            let normalized = normalize_version_for_sort(v);
            (Versioning::new(normalized), v.to_string())
        })
        .collect())
}

/// [`scan_versions`] for read-only shared install dirs, where a unreadable
/// entry is reported and skipped rather than failing the whole scan.
fn scan_versions_best_effort(dir: &Path) -> Vec<String> {
    scan_versions(dir).unwrap_or_else(|err| {
        warn!("reading versions in {} failed: {err:?}", display_path(dir));
        Default::default()
    })
}

/// Identity and versions for one primary install dir: the sidecar wins, then
/// the consolidated manifest, then legacy `.mise.backend` metadata. Returns
/// `None` when the dir holds no complete versions.
///
/// Read-only: when a legacy format was parsed, the manifest entry the caller
/// may want to persist is returned as the second element. Only the full scan
/// writes it; per-tool lookups leave migration to the next full scan.
fn scan_tool_dir(
    dir_name: &str,
    dir: &Path,
    manifest: &Manifest,
) -> Result<Option<(InstallStateTool, Option<ManifestTool>)>> {
    // Nothing is installed here, so skip the sidecar read, the legacy probes and
    // the version scan. Per-tool lookups ask about tools that are usually *not*
    // installed (every short mentioned in a config or the registry), so this is
    // the common path for them; the full scan only passes dirs that exist.
    if !dir.is_dir() {
        return Ok(None);
    }
    let tool_manifest = read_tool_manifest_from(&dir.join(".mise.backend.toml"));
    let manifest_tool = tool_manifest.as_ref().or_else(|| manifest.get(dir_name));
    let legacy_meta = if manifest_tool.is_none() {
        read_legacy_backend_meta(dir_name)
    } else {
        None
    };
    let versions = scan_versions(dir)?;
    if versions.is_empty() {
        return Ok(None);
    }

    let mut migrate = None;
    let (short, full, explicit_backend, opts) = if let Some(mt) = manifest_tool {
        let mut full = mt.full.clone();
        let mut opts = mt.opts.clone();
        // Backward compat: if opts is empty but full contains [...], extract opts
        if opts.is_empty()
            && let Some(ref f) = full
            && let Some((stripped_str, opts_str)) = crate::cli::args::split_bracketed_opts(f)
        {
            let stripped = stripped_str.to_string();
            let parsed = parse_tool_options(opts_str);
            for (k, v) in &parsed.opts {
                if EPHEMERAL_OPT_KEYS.contains(&k.as_str()) {
                    continue;
                }
                opts.insert(k.clone(), v.clone());
            }
            full = Some(stripped);
            migrate = Some(ManifestTool {
                short: mt.short.clone(),
                full: full.clone(),
                explicit_backend: mt.explicit_backend,
                opts: opts.clone(),
            });
        }
        (mt.short.clone(), full, mt.explicit_backend, opts)
    } else if let Some((s, full, explicit)) = legacy_meta {
        migrate = Some(ManifestTool {
            short: s.clone(),
            full: full.clone(),
            explicit_backend: explicit,
            opts: BTreeMap::new(),
        });
        (s, full, explicit, BTreeMap::new())
    } else {
        (dir_name.to_string(), None, true, BTreeMap::new())
    };

    let tool = InstallStateTool {
        short,
        full,
        versions,
        explicit_backend,
        opts,
        installs_path: Some(dir.to_path_buf()),
    };
    Ok(Some((tool, migrate)))
}

/// Merge the versions (and missing identity) of a shared-dir install into an
/// already-collected tool entry, creating it if absent.
fn merge_shared_tool(
    tools: &mut InstallStateTools,
    dir: &Path,
    dir_name: &str,
    shared_manifest: &Manifest,
) {
    let tool_manifest = read_tool_manifest_from(&dir.join(".mise.backend.toml"));
    let manifest_tool = tool_manifest
        .as_ref()
        .or_else(|| shared_manifest.get(dir_name));
    let versions = scan_versions_best_effort(dir);
    if versions.is_empty() {
        return;
    }

    let (short, full, explicit_backend, opts) = if let Some(mt) = manifest_tool {
        (
            mt.short.clone(),
            mt.full.clone(),
            mt.explicit_backend,
            mt.opts.clone(),
        )
    } else {
        (dir_name.to_string(), None, true, BTreeMap::new())
    };

    let tool = tools
        .entry(short.clone())
        .or_insert_with(|| InstallStateTool {
            short: short.clone(),
            full: full.clone(),
            versions: Vec::new(),
            explicit_backend,
            opts: opts.clone(),
            installs_path: Some(dir.to_path_buf()),
        });
    for v in versions {
        if !tool.versions.contains(&v) {
            tool.versions.push(v);
        }
    }
    tool.versions.sort_by_cached_key(|v| {
        let normalized = normalize_version_for_sort(v);
        (Versioning::new(normalized), v.to_string())
    });
    if tool.full.is_none() {
        tool.full = full;
    }
}

/// Scan every install dir. Memoized; enumerating callers ([`list_tools`]) pay
/// for this once per process, per-tool callers never do.
fn full_scan_tools() -> MutexResult<InstallStateTools> {
    // The full-map lock is held from before the first directory read until the
    // map is published. get_tool serves this map ahead of the per-tool memo,
    // so a raw publish could hide an install that finished mid-scan: the scan
    // reads the tool's dir too early, add_tool_version records the version in
    // the memo, then the publish shadows it. add_tool_version takes this lock
    // too, so it either completes before the scan starts reading (its version
    // is on disk by then) or blocks until the publish and updates the
    // published map. Holding the lock also makes the scan single-flight.
    let mut published = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed");
    if let Some(tools) = published.clone() {
        return Ok(tools);
    }
    measure!("install_state full_scan_tools", {
        // Shared with the per-tool path so the manifest is parsed once.
        let manifest = root_manifest();
        let subdirs = file::dir_subdirs(&dirs::INSTALLS)?;

        // Only clone the manifest for mutation if we actually need to migrate
        // legacy entries.
        let mut updated_manifest: Option<Manifest> = None;
        let mut tools = BTreeMap::new();
        for dir_name in subdirs {
            let dir = dirs::INSTALLS.join(&dir_name);
            let Some((tool, migrate)) = scan_tool_dir(&dir_name, &dir, manifest.as_ref())
                .wrap_err_with(|| format!("failed to scan {}", display_path(&dir)))?
            else {
                continue;
            };
            if let Some(mt) = migrate {
                updated_manifest
                    .get_or_insert_with(|| manifest.as_ref().clone())
                    .insert(dir_name.clone(), mt);
            }
            tools.insert(tool.short.clone(), tool);
        }

        // Write updated manifest if we migrated any legacy entries
        if let Some(ref m) = updated_manifest {
            let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
            if let Err(err) = write_manifest(m) {
                warn!("failed to write install manifest: {err:#}");
            }
        }

        // Scan shared install directories (read-only fallback directories)
        for shared_dir in env::shared_install_dirs_early() {
            if !shared_dir.is_dir() {
                continue;
            }
            let shared_manifest = shared_manifest(&shared_dir);
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
                merge_shared_tool(&mut tools, &dir, &dir_name, &shared_manifest);
            }
        }

        let plugins = load_plugins()?;
        merge_plugin_tools(&mut tools, plugins.as_ref());
        let tools = Arc::new(tools);
        *published = Some(tools.clone());
        Ok(tools)
    })
}

/// short name -> manifest dir, for the manifest entries whose dir is not just
/// the kebab-cased short. Lets a per-tool lookup skip walking every entry.
static MANIFEST_BY_SHORT: Mutex<Option<Arc<HashMap<String, String>>>> = Mutex::new(None);

/// Consolidated manifests of shared install dirs, keyed by dir. Memoized like
/// the root manifest so a per-tool lookup costs one file read per shared dir
/// per process rather than one per resolved tool.
static SHARED_MANIFEST_MEMO: Mutex<Option<HashMap<PathBuf, Arc<Manifest>>>> = Mutex::new(None);

fn shared_manifest(shared_dir: &Path) -> Arc<Manifest> {
    let mut memo = SHARED_MANIFEST_MEMO
        .lock()
        .expect("SHARED_MANIFEST_MEMO lock failed");
    memo.get_or_insert_with(Default::default)
        .entry(shared_dir.to_path_buf())
        .or_insert_with(|| Arc::new(read_manifest_from(&shared_dir.join(".mise-installs.toml"))))
        .clone()
}

fn manifest_dir_for_short(short: &str) -> Option<String> {
    let mut memo = MANIFEST_BY_SHORT
        .lock()
        .expect("MANIFEST_BY_SHORT lock failed");
    memo.get_or_insert_with(|| {
        Arc::new(
            root_manifest()
                .iter()
                .filter(|(d, mt)| **d != mt.short.to_kebab_case())
                .map(|(d, mt)| (mt.short.clone(), d.clone()))
                .collect(),
        )
    })
    .get(short)
    .cloned()
}

fn root_manifest() -> Arc<Manifest> {
    let mut memo = ROOT_MANIFEST_MEMO
        .lock()
        .expect("ROOT_MANIFEST_MEMO lock failed");
    memo.get_or_insert_with(|| Arc::new(read_manifest()))
        .clone()
}

/// Install state for one tool, loaded without scanning any other tool's
/// install dir.
///
/// mise creates install dirs as the kebab-cased short name, so that dir is
/// probed directly; the consolidated manifest (one memoized file read) covers
/// dirs recorded under a different name. A dir whose only identity is a
/// sidecar under a name that doesn't kebab-match its short is not found here —
/// that requires enumeration, which mise itself never produces, and full-scan
/// callers still see such dirs.
pub(crate) fn get_tool(short: &str) -> Option<InstallStateTool> {
    // A completed full scan is authoritative (it includes plugin identities and
    // shared dirs), so serve from it when available.
    if let Some(tools) = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed")
        .as_ref()
    {
        return tools.get(short).cloned();
    }
    {
        let memo = INSTALL_STATE_TOOL_MEMO
            .lock()
            .expect("INSTALL_STATE_TOOL_MEMO lock failed");
        if let Some(memo) = memo.as_ref()
            && let Some(hit) = memo.get(short)
        {
            return hit.clone();
        }
    }
    let tool = load_tool(short);
    // The disk read above runs without the memo lock, so add_tool_version may
    // have landed meanwhile. Neither side is authoritative: this one scanned
    // every version on disk, that one knows about an install that may have
    // completed after the scan. Picking either loses versions, so merge.
    let mut memo = INSTALL_STATE_TOOL_MEMO
        .lock()
        .expect("INSTALL_STATE_TOOL_MEMO lock failed");
    match memo
        .get_or_insert_with(Default::default)
        .entry(short.to_string())
    {
        std::collections::hash_map::Entry::Vacant(v) => v.insert(tool).clone(),
        std::collections::hash_map::Entry::Occupied(mut o) => {
            match (o.get_mut(), tool) {
                (Some(recorded), Some(scanned)) => merge_scanned_into(recorded, scanned),
                (slot, Some(scanned)) if slot.is_none() => *slot = Some(scanned),
                // A scan that found nothing does not disprove an install that
                // was just recorded.
                _ => {}
            }
            o.get().clone()
        }
    }
}

/// Fold a directory scan into an entry an installer already recorded. The
/// recorded identity wins — it was just written — and any version either side
/// saw is kept.
fn merge_scanned_into(recorded: &mut InstallStateTool, scanned: InstallStateTool) {
    for v in scanned.versions {
        if !recorded.versions.contains(&v) {
            recorded.versions.push(v);
        }
    }
    if recorded.full.is_none() {
        recorded.full = scanned.full;
    }
    if recorded.opts.is_empty() {
        recorded.opts = scanned.opts;
    }
    if recorded.installs_path.is_none() {
        recorded.installs_path = scanned.installs_path;
    }
}

fn load_tool(short: &str) -> Option<InstallStateTool> {
    let manifest = root_manifest();
    let dir_name = short.to_kebab_case();
    let scan_named = |dir_name: &str| {
        let dir = dirs::INSTALLS.join(dir_name);
        scan_tool_dir(dir_name, &dir, &manifest)
            .unwrap_or_else(|err| {
                // Enumerating callers get this as a hard error from the full
                // scan; a single lookup degrades to "not found" so one bad dir
                // cannot break resolution of unrelated tools.
                warn!("failed to scan {}: {err:#}", display_path(&dir));
                None
            })
            .map(|(tool, _migrate)| tool)
            .filter(|tool| tool.short == short)
    };
    let mut tool = scan_named(&dir_name);
    if tool.is_none() {
        // The manifest may record this short under a dir that doesn't
        // kebab-match it (e.g. a renamed or hand-migrated install).
        tool = manifest_dir_for_short(short).and_then(|d| scan_named(&d));
    }

    // Shared install directories can add versions or supply the whole tool.
    for shared_dir in env::shared_install_dirs_early() {
        let shared_manifest = shared_manifest(&shared_dir);
        // Like the root manifest above, a shared manifest may record this
        // short under a dir that doesn't kebab-match it; the full scan finds
        // such dirs by enumeration, so the per-tool path has to probe them.
        let alt_dir = shared_manifest
            .iter()
            .find(|(d, mt)| mt.short == short && **d != dir_name)
            .map(|(d, _)| d.clone());
        for dir_name in std::iter::once(dir_name.clone()).chain(alt_dir) {
            let dir = shared_dir.join(&dir_name);
            if !dir.is_dir() {
                continue;
            }
            let mut tools: InstallStateTools = tool
                .take()
                .map(|t| BTreeMap::from([(t.short.clone(), t)]))
                .unwrap_or_default();
            merge_shared_tool(&mut tools, &dir, &dir_name, &shared_manifest);
            tool = tools.remove(short);
        }
    }

    // An asdf/vfox plugin supplies an identity even with nothing installed.
    if let Some(plugins) = try_list_plugins()
        && plugins.contains_key(short)
    {
        let mut tools: InstallStateTools = tool
            .take()
            .map(|t| BTreeMap::from([(t.short.clone(), t)]))
            .unwrap_or_default();
        merge_plugin_tools(
            &mut tools,
            &BTreeMap::from([(short.to_string(), plugins[short])]),
        );
        tool = tools.remove(short);
    }
    tool
}

fn merge_plugin_tools(tools: &mut InstallStateTools, plugins: &InstallStatePlugins) {
    for (short, pt) in plugins {
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
                installs_path: None,
            });
        // Installed metadata describes the versions already on disk. Plugin
        // discovery should only supply an identity when no metadata exists.
        tool.full.get_or_insert(full);
    }
}

pub(crate) fn list_plugins() -> Arc<BTreeMap<String, PluginType>> {
    try_list_plugins().expect("INSTALL_STATE_PLUGINS is None")
}

pub(crate) fn try_list_plugins() -> Option<Arc<BTreeMap<String, PluginType>>> {
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

/// Run `f` against one tool's state without cloning it.
///
/// The state lives behind a mutex, so a caller that only needs one field would
/// otherwise clone the whole entry — versions vec, opts map and paths — to read
/// it. Backend resolution and `list_installed_versions` do that per backend, so
/// the copies add up on enumerating commands.
fn with_tool<T>(short: &str, f: impl FnOnce(&InstallStateTool) -> T) -> Option<T> {
    if let Some(tools) = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed")
        .as_ref()
    {
        return tools.get(short).map(f);
    }
    {
        let memo = INSTALL_STATE_TOOL_MEMO
            .lock()
            .expect("INSTALL_STATE_TOOL_MEMO lock failed");
        if let Some(hit) = memo.as_ref().and_then(|m| m.get(short)) {
            return hit.as_ref().map(f);
        }
    }
    get_tool(short).as_ref().map(f)
}

pub(crate) fn get_tool_full(short: &str) -> Option<String> {
    with_tool(short, |t| t.full.clone()).flatten()
}

pub(crate) fn get_plugin_type(short: &str) -> Option<PluginType> {
    list_plugins().get(short).cloned()
}

/// Every installed tool. This enumerates the whole installs dir (a readdir per
/// tool plus stats per version) — reach for [`get_tool`] instead when only
/// specific tools matter.
///
/// A failed scan is reported as an error rather than as an empty set: callers
/// that *remove* things treat "no tools installed" as authoritative (shim
/// rebuilding deletes every shim it cannot account for), so an unreadable
/// installs dir must not be indistinguishable from an empty one. Note a
/// missing installs dir is not an error — that reads as genuinely empty.
pub(crate) fn try_list_tools() -> Result<Arc<BTreeMap<String, InstallStateTool>>> {
    full_scan_tools()
}

/// [`try_list_tools`] for callers that only display or enumerate, where a
/// warning is a better outcome than aborting. Never use this to decide what to
/// delete.
pub(crate) fn list_tools() -> Arc<BTreeMap<String, InstallStateTool>> {
    try_list_tools().unwrap_or_else(|err| {
        warn!("failed to scan installed tools: {err:#}");
        Arc::new(Default::default())
    })
}

pub(crate) fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let backend_type = with_tool(short, |ist| ist.full.clone())
        .flatten()
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

pub(crate) fn list_versions(short: &str) -> Vec<String> {
    with_tool(short, |tool| tool.versions.clone()).unwrap_or_default()
}

pub(crate) fn add_tool_version(ba: &BackendArg, install_path: &Path, version: &str) {
    let tool_dir = install_path.parent().map(Path::to_path_buf);
    let full = ba.full_without_opts();
    let explicit_backend = ba.has_explicit_backend();
    let opts = persistent_opts(ba);

    let update = |tool: &mut InstallStateTool| {
        if tool.full.is_none() {
            tool.full = Some(full.clone());
        }
        tool.explicit_backend = explicit_backend;
        if tool.opts.is_empty() {
            tool.opts = opts.clone();
        }
        if tool.installs_path.is_none() {
            tool.installs_path = tool_dir.clone();
        }
        if !tool.versions.iter().any(|v| v == version) {
            // Do not sort here: this version has just been resolved by the backend
            // for this install run, and offline dependency env resolution should
            // see that concrete result without adding another ordering rule.
            tool.versions.push(version.to_string());
        }
    };
    let new_tool = || {
        let mut tool = InstallStateTool {
            short: ba.short.clone(),
            full: Some(full.clone()),
            versions: Vec::new(),
            explicit_backend,
            opts: opts.clone(),
            installs_path: tool_dir.clone(),
        };
        update(&mut tool);
        tool
    };

    // Same-run resolution reads through both memo layers, so the fresh install
    // has to land in whichever ones exist.
    {
        let mut tools = INSTALL_STATE_TOOLS
            .lock()
            .expect("INSTALL_STATE_TOOLS lock failed");
        if let Some(existing_tools) = tools.as_ref() {
            let mut next_tools = existing_tools.deref().clone();
            update(next_tools.entry(ba.short.clone()).or_insert_with(new_tool));
            *tools = Some(Arc::new(next_tools));
        }
    }
    let mut memo = INSTALL_STATE_TOOL_MEMO
        .lock()
        .expect("INSTALL_STATE_TOOL_MEMO lock failed");
    let entry = memo
        .get_or_insert_with(Default::default)
        .entry(ba.short.clone())
        .or_insert(None);
    match entry {
        Some(tool) => update(tool),
        None => *entry = Some(new_tool()),
    }
}

pub(crate) async fn add_plugin(short: &str, plugin_type: PluginType) -> Result<()> {
    let mut plugins = init_plugins().await?.deref().clone();
    plugins.insert(short.to_string(), plugin_type);
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = Some(Arc::new(plugins));
    // A plugin can supply this tool's identity, so state captured before the
    // plugin existed is stale. get_tool serves a completed full scan ahead of
    // the per-tool memo, so both layers have to learn about it.
    if let Some(memo) = INSTALL_STATE_TOOL_MEMO
        .lock()
        .expect("INSTALL_STATE_TOOL_MEMO lock failed")
        .as_mut()
    {
        memo.remove(short);
    }
    let mut tools = INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed");
    if let Some(existing) = tools.as_ref() {
        let mut next = existing.deref().clone();
        merge_plugin_tools(
            &mut next,
            &BTreeMap::from([(short.to_string(), plugin_type)]),
        );
        *tools = Some(Arc::new(next));
    }
    Ok(())
}

/// Writes backend metadata to the consolidated manifest file.
/// Uses the primary installs dir manifest by default.
pub(crate) fn write_backend_meta(ba: &BackendArg) -> Result<()> {
    write_backend_meta_to(ba, &manifest_path())
}

/// Writes backend metadata to a manifest at a specific install path.
pub(crate) fn write_backend_meta_to(ba: &BackendArg, path: &Path) -> Result<()> {
    let full = ba.full_without_opts();
    let explicit = ba.has_explicit_backend();
    let opts_map = persistent_opts(ba);

    let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
    let mut manifest = read_manifest_from(path);
    let manifest_tool = ManifestTool {
        short: ba.short.clone(),
        full: Some(full),
        explicit_backend: explicit,
        opts: opts_map,
    };
    manifest.insert(ba.short.to_kebab_case(), manifest_tool.clone());
    write_manifest_to(path, &manifest)?;
    if let Some(installs_dir) = path.parent() {
        let tool_manifest = tool_manifest_path(installs_dir, &ba.short);
        if tool_manifest.parent().is_some_and(|p| p.exists()) {
            write_tool_manifest_to(&tool_manifest, &manifest_tool)?;
        }
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

pub(crate) fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
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

pub(crate) fn clear_incomplete_marker(short: &str, v: &str) -> Result<()> {
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

pub(crate) fn clear_incomplete_marker_best_effort(short: &str, v: &str) {
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
pub(crate) fn write_checksum(install_path: &Path, checksum: &str) -> Result<()> {
    let path = checksum_file_path(install_path);
    file::write(&path, checksum)?;
    Ok(())
}

/// Read the stored checksum for a tool version
pub(crate) fn read_checksum(install_path: &Path) -> Option<String> {
    let path = checksum_file_path(install_path);
    if path.exists() {
        file::read_to_string(&path).ok()
    } else {
        None
    }
}

pub(crate) fn reset() {
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = None;
    *INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed") = None;
    *INSTALL_STATE_TOOL_MEMO
        .lock()
        .expect("INSTALL_STATE_TOOL_MEMO lock failed") = None;
    *ROOT_MANIFEST_MEMO
        .lock()
        .expect("ROOT_MANIFEST_MEMO lock failed") = None;
    *MANIFEST_BY_SHORT
        .lock()
        .expect("MANIFEST_BY_SHORT lock failed") = None;
    *SHARED_MANIFEST_MEMO
        .lock()
        .expect("SHARED_MANIFEST_MEMO lock failed") = None;
    super::tool_version::reset_install_path_cache();
}

#[cfg(test)]
mod tests {
    use super::{
        InstallStateTool, lock_tool_version, merge_plugin_tools, normalize_version_for_sort,
        read_tool_manifest_from, tool_version_lock,
    };
    use crate::plugins::PluginType;
    use itertools::Itertools;
    use std::collections::BTreeMap;
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
    fn plugin_discovery_preserves_installed_backend_metadata() {
        let mut tools = BTreeMap::from([(
            "babashka".to_string(),
            InstallStateTool {
                short: "babashka".to_string(),
                full: Some("github:babashka/babashka".to_string()),
                versions: vec!["1.13.219".to_string()],
                explicit_backend: false,
                opts: BTreeMap::new(),
                installs_path: None,
            },
        )]);
        let plugins = BTreeMap::from([("babashka".to_string(), PluginType::Asdf)]);

        merge_plugin_tools(&mut tools, &plugins);

        assert_eq!(
            tools["babashka"].full.as_deref(),
            Some("github:babashka/babashka")
        );
    }

    #[test]
    fn plugin_discovery_adds_missing_tool_metadata() {
        let installs_path = std::path::PathBuf::from("installs/babashka");
        let mut tools = BTreeMap::from([(
            "babashka".to_string(),
            InstallStateTool {
                short: "babashka".to_string(),
                full: None,
                versions: vec!["1.13.219".to_string()],
                explicit_backend: false,
                opts: BTreeMap::new(),
                installs_path: Some(installs_path.clone()),
            },
        )]);
        let plugins = BTreeMap::from([("babashka".to_string(), PluginType::Asdf)]);

        merge_plugin_tools(&mut tools, &plugins);

        assert_eq!(tools["babashka"].full.as_deref(), Some("asdf:babashka"));
        assert_eq!(tools["babashka"].versions, ["1.13.219"]);
        assert_eq!(
            tools["babashka"].installs_path.as_ref(),
            Some(&installs_path)
        );
        assert!(!tools["babashka"].explicit_backend);
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
            },
        );
        manifest.insert(
            "bun".to_string(),
            ManifestTool {
                short: "bun".to_string(),
                full: Some("aqua:oven-sh/bun".to_string()),
                explicit_backend: false,
                opts: BTreeMap::new(),
            },
        );
        manifest.insert(
            "tiny".to_string(),
            ManifestTool {
                short: "tiny".to_string(),
                full: None,
                explicit_backend: true,
                opts: BTreeMap::new(),
            },
        );

        let serialized = toml::to_string_pretty(&manifest).unwrap();
        let deserialized: Manifest = toml::from_str(&serialized).unwrap();

        assert_eq!(deserialized.len(), 3);
        assert_eq!(deserialized["node"].full.as_deref(), Some("core:node"));
        assert!(deserialized["node"].explicit_backend);
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
        assert!(mt.full.as_ref().unwrap().contains('['));
    }
}
