use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::git::Git;
use crate::plugins::PluginType;
use crate::{dirs, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
}

fn default_true() -> bool {
    true
}

/// In-memory representation of the manifest keyed by short name.
type Manifest = BTreeMap<String, ManifestTool>;

static INSTALL_STATE_PLUGINS: Mutex<Option<Arc<InstallStatePlugins>>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<Arc<InstallStateTools>>> = Mutex::new(None);
static MANIFEST_LOCK: Mutex<()> = Mutex::new(());

fn manifest_path() -> PathBuf {
    dirs::INSTALLS.join(".mise-installs.toml")
}

/// Read the consolidated manifest file. Returns empty map if it doesn't exist.
fn read_manifest() -> Manifest {
    let path = manifest_path();
    match file::read_to_string(&path) {
        std::result::Result::Ok(body) => match toml::from_str(&body) {
            std::result::Result::Ok(m) => m,
            Err(err) => {
                warn!(
                    "failed to parse manifest at {}: {err:#}",
                    display_path(&path)
                );
                Default::default()
            }
        },
        Err(_) => Default::default(),
    }
}

/// Write the consolidated manifest file.
fn write_manifest(manifest: &Manifest) -> Result<()> {
    let path = manifest_path();
    let body = toml::to_string_pretty(manifest)?;
    file::write(&path, body.trim())?;
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
            } else if path.join("metadata.lua").exists() {
                if has_backend_methods(&path) {
                    Some((d, PluginType::VfoxBackend))
                } else {
                    Some((d, PluginType::Vfox))
                }
            } else if path.join("bin").join("list-all").exists() {
                Some((d, PluginType::Asdf))
            } else {
                None
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
    //    Only clone the manifest for mutation if we actually need to migrate legacy entries.
    let mut updated_manifest: Option<Manifest> = None;
    let mut tools = BTreeMap::new();
    for dir_name in subdirs {
        let dir = dirs::INSTALLS.join(&dir_name);

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
        let (short, full, explicit_backend) = if let Some(mt) = manifest.get(&dir_name) {
            (mt.short.clone(), mt.full.clone(), mt.explicit_backend)
        } else if let Some((s, full, explicit)) = read_legacy_backend_meta(&dir_name) {
            // Migration: absorb into manifest (clone on first migration)
            let m = updated_manifest.get_or_insert_with(|| manifest.clone());
            m.insert(
                dir_name.clone(),
                ManifestTool {
                    short: s.clone(),
                    full: full.clone(),
                    explicit_backend: explicit,
                },
            );
            (s, full, explicit)
        } else {
            (dir_name.clone(), None, true)
        };

        let tool = InstallStateTool {
            short: short.clone(),
            full,
            versions,
            explicit_backend,
        };
        time!("init_tools {short}");
        tools.insert(short, tool);
    }

    // Write updated manifest if we migrated any legacy entries
    if let Some(ref m) = updated_manifest {
        let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
        if let Err(err) = write_manifest(m) {
            warn!("failed to write install manifest: {err:#}");
        }
    }

    for (short, pt) in init_plugins().await?.iter() {
        let full = match pt {
            PluginType::Asdf => format!("asdf:{short}"),
            PluginType::Vfox => format!("vfox:{short}"),
            PluginType::VfoxBackend => short.clone(),
        };
        let tool = tools
            .entry(short.clone())
            .or_insert_with(|| InstallStateTool {
                short: short.clone(),
                full: Some(full.clone()),
                versions: Default::default(),
                explicit_backend: true,
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
    INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed")
        .as_ref()
        .expect("INSTALL_STATE_PLUGINS is None")
        .clone()
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

fn has_backend_methods(plugin_path: &Path) -> bool {
    // to be a backend plugin, it must have a backend_install.lua file so we don't need to check for other files
    plugin_path
        .join("hooks")
        .join("backend_install.lua")
        .exists()
}

pub fn get_tool_full(short: &str) -> Option<String> {
    list_tools().get(short).and_then(|t| t.full.clone())
}

pub fn get_plugin_type(short: &str) -> Option<PluginType> {
    list_plugins().get(short).cloned()
}

pub fn list_tools() -> Arc<BTreeMap<String, InstallStateTool>> {
    INSTALL_STATE_TOOLS
        .lock()
        .expect("INSTALL_STATE_TOOLS lock failed")
        .as_ref()
        .expect("INSTALL_STATE_TOOLS is None")
        .clone()
}

pub fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let backend_type = list_tools()
        .get(short)
        .and_then(|ist| ist.full.as_ref())
        .map(|full| BackendType::guess(full));
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

pub async fn add_plugin(short: &str, plugin_type: PluginType) -> Result<()> {
    let mut plugins = init_plugins().await?.deref().clone();
    plugins.insert(short.to_string(), plugin_type);
    *INSTALL_STATE_PLUGINS
        .lock()
        .expect("INSTALL_STATE_PLUGINS lock failed") = Some(Arc::new(plugins));
    Ok(())
}

/// Writes backend metadata to the consolidated manifest file.
pub fn write_backend_meta(ba: &BackendArg) -> Result<()> {
    let full = match ba.full() {
        full if full.starts_with("core:") => ba.full(),
        _ => ba.full_with_opts(),
    };
    let explicit = ba.has_explicit_backend();

    let _lock = MANIFEST_LOCK.lock().expect("MANIFEST_LOCK lock failed");
    let mut manifest = read_manifest();
    manifest.insert(
        ba.short.to_kebab_case(),
        ManifestTool {
            short: ba.short.clone(),
            full: Some(full),
            explicit_backend: explicit,
        },
    );
    write_manifest(&manifest)?;
    Ok(())
}

pub fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
    dirs::CACHE
        .join(short.to_kebab_case())
        .join(v)
        .join("incomplete")
}

/// Path to the checksum file for a specific tool version
/// Used to track changes in rolling releases (like "nightly")
fn checksum_file_path(short: &str, v: &str) -> PathBuf {
    dirs::INSTALLS
        .join(short.to_kebab_case())
        .join(v)
        .join(".mise.checksum")
}

/// Store the checksum for a tool version (used for rolling release tracking)
pub fn write_checksum(short: &str, v: &str, checksum: &str) -> Result<()> {
    let path = checksum_file_path(short, v);
    file::write(&path, checksum)?;
    Ok(())
}

/// Read the stored checksum for a tool version
pub fn read_checksum(short: &str, v: &str) -> Option<String> {
    let path = checksum_file_path(short, v);
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
}

#[cfg(test)]
mod tests {
    use super::normalize_version_for_sort;
    use itertools::Itertools;
    use versions::Versioning;

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
            },
        );
        manifest.insert(
            "bun".to_string(),
            ManifestTool {
                short: "bun".to_string(),
                full: Some("aqua:oven-sh/bun".to_string()),
                explicit_backend: false,
            },
        );
        manifest.insert(
            "tiny".to_string(),
            ManifestTool {
                short: "tiny".to_string(),
                full: None,
                explicit_backend: true,
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
}
