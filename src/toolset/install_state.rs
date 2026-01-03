use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::git::Git;
use crate::lock_file::LockFile;
use crate::plugins::PluginType;
use crate::{dirs, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
use indexmap::IndexMap;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::task::JoinSet;
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
}

static INSTALL_STATE_PLUGINS: Mutex<Option<Arc<InstallStatePlugins>>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<Arc<InstallStateTools>>> = Mutex::new(None);

/// Path to the single index file that stores all backend metadata
fn index_path() -> PathBuf {
    dirs::INSTALLS.join(".mise.meta.toml")
}

/// Index file structure for TOML serialization
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct BackendIndex {
    #[serde(default)]
    tools: IndexMap<String, String>,
}

/// Read the backend index file. Returns map of short -> full.
/// If index doesn't exist, migrates from legacy .mise.backend files.
/// Also cleans up entries for tools whose directories no longer exist.
fn read_index() -> BTreeMap<String, String> {
    let mut index = file::read_to_string(index_path())
        .ok()
        .and_then(|content| toml::from_str::<BackendIndex>(&content).ok())
        .map(|parsed| parsed.tools.into_iter().collect())
        .unwrap_or_else(migrate_to_index);

    // Clean up entries for tools whose directories no longer exist
    let existing_dirs: std::collections::HashSet<_> = file::dir_subdirs(&dirs::INSTALLS)
        .unwrap_or_default()
        .into_iter()
        .collect();

    let original_len = index.len();
    index.retain(|short, _| existing_dirs.contains(&short.to_kebab_case()));

    if index.len() != original_len {
        let _ = write_index(&index);
    }

    index
}

/// Write the backend index atomically using temp file + rename
fn write_index(index: &BTreeMap<String, String>) -> Result<()> {
    let path = index_path();
    let tmp = path.with_extension("tmp");
    let backend_index = BackendIndex {
        tools: index.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
    };
    let content = toml::to_string_pretty(&backend_index)?;
    file::write(&tmp, &content)?;
    std::fs::rename(&tmp, &path)?;
    eyre::Ok(())
}

/// Migrate from legacy per-tool .mise.backend files to single index
fn migrate_to_index() -> BTreeMap<String, String> {
    let mut index = BTreeMap::new();
    if let std::result::Result::Ok(dirs) = file::dir_subdirs(&dirs::INSTALLS) {
        for dir in dirs {
            if let Some(full) = read_legacy_backend_meta(&dir) {
                // Use dir (kebab-cased) as key to match directory naming
                index.insert(dir, full);
            }
        }
    }
    // Write the new index file (ignore errors during migration)
    if !index.is_empty()
        && let Err(err) = write_index(&index)
    {
        warn!("Failed to write index during migration: {err}");
    }
    index
}

/// Read a legacy .mise.backend file (for migration)
/// Returns the full backend identifier (second line of the file)
fn read_legacy_backend_meta(dir: &str) -> Option<String> {
    let path = dirs::INSTALLS.join(dir).join(".mise.backend");

    if path.exists() {
        let body = file::read_to_string(&path)
            .map_err(|err| {
                warn!("{err:?}");
            })
            .unwrap_or_default();
        let lines: Vec<&str> = body.lines().filter(|f| !f.is_empty()).collect();
        let full = lines.get(1).map(|s| s.to_string());

        // Delete the legacy file after reading
        if let Err(err) = file::remove_file(&path) {
            debug!("Failed to remove legacy .mise.backend file: {err}");
        }

        full
    } else {
        None
    }
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
    // Read the index once instead of per-tool .mise.backend files
    let index = read_index();
    let mut jset = JoinSet::new();
    for dir in file::dir_subdirs(&dirs::INSTALLS)? {
        let index = index.clone();
        jset.spawn(async move {
            // Look up in index instead of reading individual .mise.backend file
            let (short, full) = if let Some(full) = index.get(&dir) {
                (dir.clone(), Some(full.clone()))
            } else {
                (dir.clone(), None)
            };
            let dir = dirs::INSTALLS.join(&dir);
            let versions = file::dir_subdirs(&dir)
                .unwrap_or_else(|err| {
                    warn!("reading versions in {} failed: {err:?}", display_path(&dir));
                    Default::default()
                })
                .into_iter()
                .filter(|v| !v.starts_with('.'))
                .filter(|v| !runtime_symlinks::is_runtime_symlink(&dir.join(v)))
                .filter(|v| !dir.join(v).join("incomplete").exists())
                .sorted_by_cached_key(|v| {
                    // Normalize version for sorting to handle mixed v-prefix versions
                    // e.g., "v2.0.51" and "2.0.35" should sort by numeric value
                    let normalized = normalize_version_for_sort(v);
                    (Versioning::new(normalized), v.to_string())
                })
                .collect();
            let tool = InstallStateTool {
                short: short.clone(),
                full,
                versions,
            };
            time!("init_tools {short}");
            (short, tool)
        });
    }
    let mut tools = jset
        .join_all()
        .await
        .into_iter()
        .filter(|(_, tool)| !tool.versions.is_empty())
        .collect::<BTreeMap<_, _>>();
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
    let key = short.to_kebab_case();
    list_tools().get(&key).and_then(|t| t.full.clone())
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
    let key = short.to_kebab_case();
    let backend_type = list_tools()
        .get(&key)
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
    let key = short.to_kebab_case();
    list_tools()
        .get(&key)
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

/// Update the backend index with a new tool entry
pub fn write_backend_meta(ba: &BackendArg) -> Result<()> {
    let full = match ba.full() {
        full if full.starts_with("core:") => ba.full(),
        _ => ba.full_with_opts(),
    };
    // Use file lock to prevent race conditions during parallel installs
    let _lock = LockFile::new(&index_path())
        .with_callback(|p| debug!("waiting for lock on {}", display_path(p)))
        .lock()?;
    // Read current index, update it, and write back
    // Use kebab-cased short as key to match directory naming
    let key = ba.short.to_kebab_case();
    let mut index = read_index();
    index.insert(key, full);
    write_index(&index)?;
    Ok(())
}

pub fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
    dirs::CACHE
        .join(short.to_kebab_case())
        .join(v)
        .join("incomplete")
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
}
