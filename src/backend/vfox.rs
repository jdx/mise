use crate::env;
use heck::ToKebabCase;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::RwLock;
use std::thread;

use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::{Config, SETTINGS};
use crate::dirs;
use crate::install_context::InstallContext;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::plugins::{Plugin, PluginType};
use crate::tokio::RUNTIME;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;

#[derive(Debug)]
pub struct VfoxBackend {
    ba: BackendArg,
    plugin: Box<VfoxPlugin>,
    remote_version_cache: CacheManager<Vec<String>>,
    exec_env_cache: RwLock<HashMap<String, CacheManager<BTreeMap<String, String>>>>,
    pathname: String,
}

impl Backend for VfoxBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Vfox
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_plugin_type(&self) -> Option<PluginType> {
        Some(PluginType::Vfox)
    }

    fn get_dependencies(&self, tvr: &ToolRequest) -> eyre::Result<Vec<String>> {
        let out = match tvr.ba().tool_name.as_str() {
            "poetry" | "pipenv" | "pipx" => vec!["python"],
            "elixir" => vec!["erlang"],
            _ => vec![],
        };
        Ok(out.into_iter().map(|s| s.into()).collect())
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let (vfox, _log_rx) = self.plugin.vfox();
                self.ensure_plugin_installed()?;
                let versions = RUNTIME.block_on(vfox.list_available_versions(&self.pathname))?;
                Ok(versions
                    .into_iter()
                    .rev()
                    .map(|v| v.version)
                    .collect::<Vec<String>>())
            })
            .cloned()
    }

    fn install_version_impl(
        &self,
        _ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        self.ensure_plugin_installed()?;
        let (vfox, log_rx) = self.plugin.vfox();
        thread::spawn(|| {
            for line in log_rx {
                // TODO: put this in ctx.pr.set_message()
                info!("{}", line);
            }
        });
        RUNTIME.block_on(vfox.install(&self.pathname, &tv.version, tv.install_path()))?;
        Ok(tv)
    }

    fn list_bin_paths(&self, tv: &ToolVersion) -> eyre::Result<Vec<PathBuf>> {
        let path = self
            ._exec_env(tv)?
            .iter()
            .find(|(k, _)| k.to_uppercase() == "PATH")
            .map(|(_, v)| v.to_string())
            .unwrap_or("bin".to_string());
        Ok(env::split_paths(&path).collect())
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        Ok(self
            ._exec_env(tv)?
            .into_iter()
            .filter(|(k, _)| k.to_uppercase() != "PATH")
            .collect())
    }
}

impl VfoxBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let pathname = ba.short.to_kebab_case();
        let plugin_path = dirs::PLUGINS.join(&pathname);
        let mut plugin = VfoxPlugin::new(pathname.clone(), plugin_path.clone());
        plugin.full = Some(ba.full());
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .with_fresh_file(dirs::DATA.to_path_buf())
            .with_fresh_file(plugin_path.to_path_buf())
            .with_fresh_file(ba.installs_path.to_path_buf())
            .build(),
            exec_env_cache: Default::default(),
            plugin: Box::new(plugin),
            ba,
            pathname,
        }
    }

    fn _exec_env(&self, tv: &ToolVersion) -> eyre::Result<BTreeMap<String, String>> {
        let key = tv.to_string();
        if !self.exec_env_cache.read().unwrap().contains_key(&key) {
            let mut caches = self.exec_env_cache.write().unwrap();
            caches.insert(
                key.clone(),
                CacheManagerBuilder::new(tv.cache_path().join("exec_env.msgpack.z"))
                    .with_fresh_file(dirs::DATA.to_path_buf())
                    .with_fresh_file(self.plugin.plugin_path.to_path_buf())
                    .with_fresh_file(self.ba().installs_path.to_path_buf())
                    .build(),
            );
        }
        let exec_env_cache = self.exec_env_cache.read().unwrap();
        let cache = exec_env_cache.get(&key).unwrap();
        cache
            .get_or_try_init(|| {
                self.ensure_plugin_installed()?;
                let (vfox, _log_rx) = self.plugin.vfox();
                Ok(RUNTIME
                    .block_on(vfox.env_keys(&self.pathname, &tv.version))?
                    .into_iter()
                    .fold(BTreeMap::new(), |mut acc, env_key| {
                        let key = &env_key.key;
                        if let Some(val) = acc.get(key) {
                            let mut paths = env::split_paths(val).collect::<Vec<PathBuf>>();
                            paths.push(PathBuf::from(env_key.value));
                            acc.insert(
                                env_key.key,
                                env::join_paths(paths)
                                    .unwrap()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                        } else {
                            acc.insert(key.clone(), env_key.value);
                        }
                        acc
                    }))
            })
            .cloned()
    }

    fn ensure_plugin_installed(&self) -> eyre::Result<()> {
        self.plugin
            .ensure_installed(&MultiProgressReport::get(), false)
    }
}
