use crate::backend::backend_type::BackendType;
use crate::file::display_path;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::{backend, dirs, file, runtime_symlinks};
use eyre::{Ok, Result};
use heck::ToKebabCase;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use versions::Versioning;

type InstallStatePlugins = BTreeMap<String, PluginType>;
type InstallStateTools = BTreeMap<String, InstallStateTool>;

#[derive(Debug, Clone)]
pub struct InstallStateTool {
    pub short: String,
    pub _dir: PathBuf,
    pub full: String,
    pub backend_type: BackendType,
    pub versions: Vec<String>,
}

static INSTALL_STATE_PLUGINS: Mutex<Option<InstallStatePlugins>> = Mutex::new(None);
static INSTALL_STATE_TOOLS: Mutex<Option<InstallStateTools>> = Mutex::new(None);

pub fn init() -> Result<()> {
    drop(init_plugins()?);
    drop(init_tools()?);
    Ok(())
}

fn init_plugins() -> Result<MutexGuard<'static, Option<BTreeMap<String, PluginType>>>> {
    let mut mu = INSTALL_STATE_PLUGINS.lock().unwrap();
    if mu.is_some() {
        return Ok(mu);
    }
    let dirs = file::dir_subdirs(&dirs::PLUGINS)?;
    let plugins = dirs
        .into_iter()
        .filter_map(|d| {
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
    time!("init_install_state plugins");
    *mu = Some(plugins);
    Ok(mu)
}

fn init_tools() -> Result<MutexGuard<'static, Option<BTreeMap<String, InstallStateTool>>>> {
    let mut mu = INSTALL_STATE_TOOLS.lock().unwrap();
    if mu.is_some() {
        return Ok(mu);
    }
    let mut tools: InstallStateTools = init_plugins()?
        .as_ref()
        .unwrap()
        .iter()
        .map(|(short, pt)| {
            let tool = InstallStateTool {
                short: short.clone(),
                _dir: dirs::PLUGINS.join(short),
                backend_type: match pt {
                    PluginType::Asdf => BackendType::Asdf,
                    PluginType::Vfox => BackendType::Vfox,
                },
                full: match pt {
                    PluginType::Asdf => format!("asdf:{short}"),
                    PluginType::Vfox => format!("vfox:{short}"),
                },
                versions: Default::default(),
            };
            (short.clone(), tool)
        })
        .collect();
    let dirs = file::dir_subdirs(&dirs::INSTALLS)?;
    tools.extend(
        dirs.into_iter()
            .map(|dir| {
                let backend_meta = read_backend_meta(&dir).unwrap_or_default();
                let short = backend_meta.first().unwrap_or(&dir).to_string();
                let full = backend_meta.get(1).cloned();
                let dir = dirs::INSTALLS.join(&dir);
                let full = if let Some(full) = short_to_full(&short, full)? {
                    full
                } else {
                    return Ok(None);
                };
                let backend_type = BackendType::guess(&full);
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
                    _dir: dir.clone(),
                    full,
                    backend_type,
                    versions,
                };
                Ok(Some((short, tool)))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten(),
    );
    time!("init_install_state tools");
    *mu = Some(tools);
    Ok(mu)
}

pub fn short_to_full(short: &str, meta_full: Option<String>) -> Result<Option<String>> {
    let plugins = init_plugins()?;
    let plugins = plugins.as_ref().unwrap();
    if let Some(plugin) = plugins.get(short) {
        match plugin {
            PluginType::Asdf => Ok(Some(format!("asdf:{short}"))),
            PluginType::Vfox => Ok(Some(format!("vfox:{short}"))),
        }
    } else if let Some(full) = meta_full {
        Ok(Some(full))
    } else if let Some(full) = REGISTRY
        .get(short)
        .map(|r| r.backends())
        .unwrap_or_default()
        .first()
    {
        Ok(Some(full.to_string()))
    } else {
        Ok(None)
    }
}

pub fn list_plugins() -> Result<BTreeMap<String, PluginType>> {
    let plugins = init_plugins()?;
    Ok(plugins.as_ref().unwrap().clone())
}

pub fn get_plugin_type(short: &str) -> Result<Option<PluginType>> {
    let plugins = init_plugins()?;
    Ok(plugins.as_ref().unwrap().get(short).cloned())
}

pub fn list_tools() -> Result<BTreeMap<String, InstallStateTool>> {
    let tools = init_tools()?;
    Ok(tools.as_ref().unwrap().clone())
}

pub fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let tools = init_tools()?;
    Ok(tools
        .as_ref()
        .unwrap()
        .get(short)
        .map(|tool| tool.backend_type))
}

pub fn list_versions(short: &str) -> Result<Vec<String>> {
    let tools = init_tools()?;
    Ok(tools
        .as_ref()
        .unwrap()
        .get(short)
        .map(|tool| tool.versions.clone())
        .unwrap_or_default())
}

pub fn add_plugin(short: &str, plugin_type: PluginType) -> Result<()> {
    let mut plugins = init_plugins()?;
    plugins
        .as_mut()
        .unwrap()
        .insert(short.to_string(), plugin_type);
    Ok(())
}

fn backend_meta_path(short: &str) -> PathBuf {
    dirs::INSTALLS.join(short).join(".mise.backend")
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

pub fn incomplete_file_path(short: &str, v: &str) -> PathBuf {
    dirs::CACHE
        .join(short.to_kebab_case())
        .join(v)
        .join("incomplete")
}

pub fn reset() {
    *INSTALL_STATE_PLUGINS.lock().unwrap() = None;
    *INSTALL_STATE_TOOLS.lock().unwrap() = None;
    backend::reset();
}
