use crate::{env, plugins::PluginEnum, timeout};
use async_trait::async_trait;
use heck::ToKebabCase;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use tokio::sync::RwLock;

use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::dirs;
use crate::env_diff::EnvMap;
use crate::install_context::InstallContext;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::plugins::{Plugin, PluginType};
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;

#[derive(Debug)]
pub struct VfoxBackend {
    ba: Arc<BackendArg>,
    plugin: Arc<VfoxPlugin>,
    plugin_enum: PluginEnum,
    exec_env_cache: RwLock<HashMap<String, CacheManager<EnvMap>>>,
    pathname: String,
}

#[async_trait]
impl Backend for VfoxBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Vfox
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_plugin_type(&self) -> Option<PluginType> {
        Some(PluginType::Vfox)
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        let this = self;
        timeout::run_with_timeout_async(
            || async {
                let (vfox, _log_rx) = this.plugin.vfox();
                this.ensure_plugin_installed(config).await?;
                let versions = vfox.list_available_versions(&this.pathname).await?;
                Ok(versions
                    .into_iter()
                    .rev()
                    .map(|v| v.version)
                    .collect::<Vec<String>>())
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn install_version_(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        self.ensure_plugin_installed(&ctx.config).await?;
        let (vfox, log_rx) = self.plugin.vfox();
        thread::spawn(|| {
            for line in log_rx {
                // TODO: put this in ctx.pr.set_message()
                info!("{}", line);
            }
        });
        vfox.install(&self.pathname, &tv.version, tv.install_path())
            .await?;
        Ok(tv)
    }

    async fn list_bin_paths(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<Vec<PathBuf>> {
        let path = self
            ._exec_env(config, tv)
            .await?
            .iter()
            .find(|(k, _)| k.to_uppercase() == "PATH")
            .map(|(_, v)| v.to_string())
            .unwrap_or("bin".to_string());
        Ok(env::split_paths(&path).collect())
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<EnvMap> {
        Ok(self
            ._exec_env(config, tv)
            .await?
            .into_iter()
            .filter(|(k, _)| k.to_uppercase() != "PATH")
            .collect())
    }

    fn plugin(&self) -> Option<&PluginEnum> {
        Some(&self.plugin_enum)
    }
}

impl VfoxBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        let pathname = ba.short.to_kebab_case();
        let plugin_path = dirs::PLUGINS.join(&pathname);
        let mut plugin = VfoxPlugin::new(pathname.clone(), plugin_path.clone());
        plugin.full = Some(ba.full());
        let plugin = Arc::new(plugin);
        Self {
            exec_env_cache: Default::default(),
            plugin: plugin.clone(),
            plugin_enum: PluginEnum::Vfox(plugin),
            ba: Arc::new(ba),
            pathname,
        }
    }

    async fn _exec_env(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        let key = tv.to_string();
        if !self.exec_env_cache.read().await.contains_key(&key) {
            let mut caches = self.exec_env_cache.write().await;
            caches.insert(
                key.clone(),
                CacheManagerBuilder::new(tv.cache_path().join("exec_env.msgpack.z"))
                    .with_fresh_file(dirs::DATA.to_path_buf())
                    .with_fresh_file(self.plugin.plugin_path.to_path_buf())
                    .with_fresh_file(self.ba().installs_path.to_path_buf())
                    .build(),
            );
        }
        let exec_env_cache = self.exec_env_cache.read().await;
        let cache = exec_env_cache.get(&key).unwrap();
        cache
            .get_or_try_init_async(async || {
                self.ensure_plugin_installed(config).await?;
                let (vfox, _log_rx) = self.plugin.vfox();
                Ok(vfox
                    .env_keys(&self.pathname, &tv.version)
                    .await?
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
            .await
            .cloned()
    }

    async fn ensure_plugin_installed(&self, config: &Arc<Config>) -> eyre::Result<()> {
        self.plugin
            .ensure_installed(config, &MultiProgressReport::get(), false)
            .await
    }
}
