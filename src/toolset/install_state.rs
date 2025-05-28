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
use std::sync::Arc;
use std::sync::Mutex;
use tokio::task::JoinSet;
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
    if let Some(plugins) = INSTALL_STATE_PLUGINS.lock().unwrap().clone() {
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

async fn init_tools() -> MutexResult<InstallStateTools> {
    if let Some(tools) = INSTALL_STATE_TOOLS.lock().unwrap().clone() {
        return Ok(tools);
    }
    let mut jset = JoinSet::new();
    for dir in file::dir_subdirs(&dirs::INSTALLS)? {
        jset.spawn(async move {
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

pub fn list_plugins() -> Arc<BTreeMap<String, PluginType>> {
    INSTALL_STATE_PLUGINS
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
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

pub fn get_tool_full(short: &str) -> Option<String> {
    list_tools().get(short).and_then(|t| t.full.clone())
}

pub fn get_plugin_type(short: &str) -> Option<PluginType> {
    list_plugins().get(short).cloned()
}

pub fn list_tools() -> Arc<BTreeMap<String, InstallStateTool>> {
    INSTALL_STATE_TOOLS
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone()
}

pub fn backend_type(short: &str) -> Result<Option<BackendType>> {
    let backend_type = list_tools()
        .get(short)
        .and_then(|ist| ist.full.as_ref())
        .map(|full| BackendType::guess(full));
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
            let doc = format!("{short}\n{full}");
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
    // only use full_with_opts for specific plugin prefixes
    let full = match ba.full() {
        full if full.starts_with("cargo:")
            || full.starts_with("go:")
            || full.starts_with("pipx:")
            || full.starts_with("ubi:") =>
        {
            ba.full_with_opts()
        }
        _ => ba.full(),
    };
    let doc = format!("{}\n{}", ba.short, full);
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
