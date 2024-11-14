use crate::backend::backend_type::BackendType;
use crate::file::display_path;
use crate::plugins::PluginType;
use crate::registry::REGISTRY;
use crate::{dirs, file, runtime_symlinks};
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
            .map(|short| {
                let dir = dirs::INSTALLS.join(&short);
                let full = if let Some(full) = short_to_full(&short)? {
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

pub fn short_to_full(short: &str) -> Result<Option<String>> {
    let plugins = init_plugins()?;
    let plugins = plugins.as_ref().unwrap();
    if let Some(plugin) = plugins.get(short) {
        match plugin {
            PluginType::Asdf => Ok(Some(format!("asdf:{short}"))),
            PluginType::Vfox => Ok(Some(format!("vfox:{short}"))),
        }
    } else if let Some(full) = read_backend_meta(short) {
        Ok(Some(full))
    } else if let Some(full) = REGISTRY
        .get(short)
        .map(|r| &r.backends)
        .unwrap_or(&EMPTY_VEC)
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

fn migrate_backend_meta_json(short: &str) {
    let old = dirs::INSTALLS.join(short).join(".mise.backend.json");
    let migrate = || {
        let json: serde_json::Value = serde_json::from_reader(file::open(&old)?)?;
        if let Some(full) = json.get("id").and_then(|id| id.as_str()) {
            file::write(backend_meta_path(short), full.trim())?;
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

fn read_backend_meta(short: &str) -> Option<String> {
    migrate_backend_meta_json(short);
    let path = backend_meta_path(short);
    if path.exists() {
        let body = file::read_to_string(&path)
            .map_err(|err| {
                warn!("{err:?}");
            })
            .unwrap_or_default();
        Some(body)
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

static EMPTY_VEC: Vec<&'static str> = vec![];

pub fn reset() {
    *INSTALL_STATE_PLUGINS.lock().unwrap() = None;
    *INSTALL_STATE_TOOLS.lock().unwrap() = None;
}
