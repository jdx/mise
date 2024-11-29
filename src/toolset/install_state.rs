use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::file::display_path;
use crate::plugins::PluginType;
use crate::{dirs, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use versions::Versioning;

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

pub(crate) fn init() -> Result<()> {
    let (plugins, tools) = rayon::join(
        || {
            measure!("init_plugins", { drop(init_plugins()?) });
            Ok(())
        },
        || {
            measure!("init_tools", {
                drop(init_tools()?);
            });
            Ok(())
        },
    );
    plugins?;
    tools?;
    Ok(())
}

fn init_plugins() -> MutexResult<InstallStatePlugins> {
    if let Some(plugins) = INSTALL_STATE_PLUGINS.lock().unwrap().clone() {
        return Ok(plugins);
    }
    let dirs = file::dir_subdirs(&dirs::PLUGINS)?;
    let plugins: InstallStatePlugins = dirs
        .into_iter()
        .filter_map(|d| {
            time!("init_plugins {d}");
            let path = dirs::PLUGINS.join(&d);
            if path.join("metadata.lua").exists() {
                Some((d, PluginType::Vfox))
            } else if path.join("bin").join("list-all").exists() {
                Some((d, PluginType::Asdf))
            } else {
                None
            }
        })
        .collect();
    let plugins = Arc::new(plugins);
    *INSTALL_STATE_PLUGINS.lock().unwrap() = Some(plugins.clone());
    Ok(plugins)
}

fn init_tools() -> MutexResult<InstallStateTools> {
    if let Some(tools) = INSTALL_STATE_TOOLS.lock().unwrap().clone() {
        return Ok(tools);
    }
    let mut tools = file::dir_subdirs(&dirs::INSTALLS)?
        .into_par_iter()
        .map(|dir| {
            let backend_meta = read_backend_meta(&dir).unwrap_or_default();
            let short = backend_meta.first().unwrap_or(&dir).to_string();
            let full = backend_meta.get(1).cloned();
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
                .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
                .collect();
            let tool = InstallStateTool {
                short: short.clone(),
                full,
                versions,
            };
            time!("init_tools {short}");
            Ok(Some((short, tool)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .filter(|(_, tool)| !tool.versions.is_empty())
        .collect::<BTreeMap<_, _>>();
    for (short, pt) in init_plugins()?.iter() {
        let full = match pt {
            PluginType::Asdf => format!("asdf:{short}"),
            PluginType::Vfox => format!("vfox:{short}"),
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
    *INSTALL_STATE_TOOLS.lock().unwrap() = Some(tools.clone());
    Ok(tools)
}

pub fn list_plugins() -> Result<Arc<BTreeMap<String, PluginType>>> {
    let plugins = init_plugins()?;
    Ok(plugins)
}

pub fn get_tool_full(short: &str) -> Result<Option<String>> {
    let tools = init_tools()?;
    Ok(tools.get(short).and_then(|t| t.full.clone()))
}

pub fn get_plugin_type(short: &str) -> Result<Option<PluginType>> {
    let plugins = init_plugins()?;
    Ok(plugins.get(short).cloned())
}

pub fn list_tools() -> Result<Arc<BTreeMap<String, InstallStateTool>>> {
    let tools = init_tools()?;
    Ok(tools)
}

pub fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let tools = init_tools()?;
    let backend_type = tools
        .get(short)
        .and_then(|ist| ist.full.as_ref())
        .map(|full| BackendType::guess(full));
    Ok(backend_type)
}

pub fn list_versions(short: &str) -> Result<Vec<String>> {
    let tools = init_tools()?;
    Ok(tools
        .get(short)
        .map(|tool| tool.versions.clone())
        .unwrap_or_default())
}

pub fn add_plugin(short: &str, plugin_type: PluginType) -> Result<()> {
    let mut plugins = init_plugins()?.deref().clone();
    plugins.insert(short.to_string(), plugin_type);
    *INSTALL_STATE_PLUGINS.lock().unwrap() = Some(Arc::new(plugins));
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
            let doc = format!("{}\n{}", short, full);
            file::write(backend_meta_path(dir), doc.trim())?;
        }
        Ok(())
    };
    if old.exists() {
        if let Err(err) = migrate() {
            debug!("{err:#}");
        }
        if let Err(err) = file::remove_file(&old) {
            debug!("{err:#}");
        }
    }
}

fn read_backend_meta(short: &str) -> Option<Vec<String>> {
    migrate_backend_meta_json(short);
    let path = backend_meta_path(short);
    if path.exists() {
        let body = file::read_to_string(&path)
            .map_err(|err| {
                warn!("{err:?}");
            })
            .unwrap_or_default();
        Some(
            body.lines()
                .filter(|f| !f.is_empty())
                .map(|f| f.to_string())
                .collect(),
        )
    } else {
        None
    }
}

pub fn write_backend_meta(ba: &BackendArg) -> Result<()> {
    let doc = format!("{}\n{}", ba.short, ba.full());
    file::write(backend_meta_path(&ba.short), doc.trim())?;
    Ok(())
}

pub fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
    dirs::CACHE
        .join(short.to_kebab_case())
        .join(v)
        .join("incomplete")
}

pub fn reset() {
    *INSTALL_STATE_PLUGINS.lock().unwrap() = None;
    *INSTALL_STATE_TOOLS.lock().unwrap() = None;
}
