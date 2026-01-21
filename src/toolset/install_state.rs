use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::git::Git;
use crate::plugins::PluginType;
use crate::{dirs, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
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
    pub explicit_backend: bool,
}

static INSTALL_STATE_PLUGINS: Mutex<Option<Arc<InstallStatePlugins>>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<Arc<InstallStateTools>>> = Mutex::new(None);

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
    let mut jset = JoinSet::new();
    for dir in file::dir_subdirs(&dirs::INSTALLS)? {
        jset.spawn(async move {
            let backend_meta = read_backend_meta(&dir).unwrap_or_default();
            let short = backend_meta.first().unwrap_or(&dir).to_string();
            let full = backend_meta.get(1).cloned();
            // Default to true for backward compatibility: existing installations
            // without this flag should preserve their installed backend
            let explicit_backend = backend_meta.get(2).map_or(true, |v| v == "1");
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
                explicit_backend,
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

fn backend_meta_path(short: &str) -> PathBuf {
    dirs::INSTALLS
        .join(short.to_kebab_case())
        .join(".mise.backend")
}

fn migrate_backend_meta_json(dir: &str) {
    let old = dirs::INSTALLS.join(dir).join(".mise.backend.json");
    let migrate = || {
        let json: serde_json::Value = serde_json::from_reader(file::open(&old)?)?;
        if let Some(full) = json.get("id").and_then(|id| id.as_str()) {
            let short = json
                .get("short")
                .and_then(|short| short.as_str())
                .unwrap_or(dir);
            // Migrated tools default to explicit_backend=true to preserve their installed backend
            let doc = format!("{short}\n{full}\n1");
            file::write(backend_meta_path(dir), doc.trim())?;
        }
        Ok(())
    };
    if old.exists() {
        if let Err(err) = migrate() {
            warn!("failed to migrate backend meta for {dir}: {err:#}");
            return; // Don't delete the old file if migration failed
        }
        if let Err(err) = file::remove_file(&old) {
            warn!("failed to remove old backend meta for {dir}: {err:#}");
        }
    }
}

fn read_backend_meta(short: &str) -> Option<Vec<String>> {
    migrate_backend_meta_json(short);
    let path = backend_meta_path(short);
    if !path.exists() {
        return None;
    }
    let body = match file::read_to_string(&path) {
        std::result::Result::Ok(body) => body,
        std::result::Result::Err(err) => {
            warn!(
                "failed to read backend meta at {}: {err:?}",
                display_path(&path)
            );
            return None;
        }
    };
    Some(
        body.lines()
            .filter(|f| !f.is_empty())
            .map(|f| f.to_string())
            .collect(),
    )
}

/// Writes backend metadata to `.mise.backend` file.
/// Format: 3 lines
/// - Line 1: short name (e.g., "bun")
/// - Line 2: full backend identifier (e.g., "aqua:oven-sh/bun")
/// - Line 3: explicit flag ("1" if user specified full backend, "0" if resolved from registry)
pub fn write_backend_meta(ba: &BackendArg) -> Result<()> {
    let full = match ba.full() {
        full if full.starts_with("core:") => ba.full(),
        _ => ba.full_with_opts(),
    };
    let explicit = if ba.has_explicit_backend() { "1" } else { "0" };
    let doc = format!("{}\n{}\n{}", ba.short, full, explicit);
    file::write(backend_meta_path(&ba.short), doc.trim())?;
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
}
