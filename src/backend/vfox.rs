use crate::{env, plugins::PluginEnum, timeout};
use async_trait::async_trait;
use eyre::WrapErr;
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
use crate::plugins::Plugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;

#[derive(Debug)]
pub struct VfoxBackend {
    ba: Arc<BackendArg>,
    plugin: Arc<VfoxPlugin>,
    plugin_enum: PluginEnum,
    exec_env_cache: RwLock<HashMap<String, CacheManager<EnvMap>>>,
    pathname: String,
    tool_name: Option<String>,
}

#[async_trait]
impl Backend for VfoxBackend {
    fn get_type(&self) -> BackendType {
        match self.plugin_enum {
            PluginEnum::VfoxBackend(_) => BackendType::VfoxBackend(self.plugin.name().to_string()),
            PluginEnum::Vfox(_) => BackendType::Vfox,
            _ => unreachable!(),
        }
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        let this = self;
        timeout::run_with_timeout_async(
            || async {
                let (vfox, _log_rx) = this.plugin.vfox();
                this.ensure_plugin_installed(config).await?;

                // Use backend methods if the plugin supports them
                if matches!(&this.plugin_enum, PluginEnum::VfoxBackend(_)) {
                    debug!("Using backend method for plugin: {}", this.pathname);
                    let tool_name = this.tool_name.as_ref().ok_or_else(|| {
                        eyre::eyre!("VfoxBackend requires a tool name (plugin:tool format)")
                    })?;
                    match vfox.backend_list_versions(&this.pathname, tool_name).await {
                        Ok(versions) => {
                            return Ok(versions);
                        }
                        Err(e) => {
                            debug!("Backend method failed: {}", e);
                            return Err(e).wrap_err("Backend list versions method failed");
                        }
                    }
                }

                // Use default vfox behavior for traditional plugins
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

        // Use backend methods if the plugin supports them
        if matches!(&self.plugin_enum, PluginEnum::VfoxBackend(_)) {
            let tool_name = self.tool_name.as_ref().ok_or_else(|| {
                eyre::eyre!("VfoxBackend requires a tool name (plugin:tool format)")
            })?;
            match vfox
                .backend_install(&self.pathname, tool_name, &tv.version, tv.install_path())
                .await
            {
                Ok(_response) => {
                    return Ok(tv);
                }
                Err(e) => {
                    return Err(e).wrap_err("Backend install method failed");
                }
            }
        }

        // Use default vfox behavior for traditional plugins
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
    pub fn from_arg(ba: BackendArg, backend_plugin_name: Option<String>) -> Self {
        let pathname = match &backend_plugin_name {
            Some(plugin_name) => plugin_name.clone(),
            None => ba.short.to_kebab_case(),
        };

        let plugin_path = dirs::PLUGINS.join(&pathname);
        let mut plugin = VfoxPlugin::new(pathname.clone(), plugin_path.clone());
        plugin.full = Some(ba.full());
        let plugin = Arc::new(plugin);

        // Extract tool name for plugin:tool format
        let tool_name = if ba.short.contains(':') {
            ba.short.split_once(':').map(|(_, tool)| tool.to_string())
        } else {
            None
        };

        Self {
            exec_env_cache: Default::default(),
            plugin: plugin.clone(),
            plugin_enum: match backend_plugin_name {
                Some(_) => PluginEnum::VfoxBackend(plugin),
                None => PluginEnum::Vfox(plugin),
            },
            ba: Arc::new(ba),
            pathname,
            tool_name,
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

                // Use backend methods if the plugin supports them
                if matches!(&self.plugin_enum, PluginEnum::VfoxBackend(_)) {
                    let tool_name = self.tool_name.as_ref().ok_or_else(|| {
                        eyre::eyre!("VfoxBackend requires a tool name (plugin:tool format)")
                    })?;
                    match vfox
                        .backend_exec_env(&self.pathname, tool_name, &tv.version, tv.install_path())
                        .await
                    {
                        Ok(response) => {
                            return Ok(response.into_iter().fold(
                                BTreeMap::new(),
                                |mut acc, env_key| {
                                    let key = &env_key.key;
                                    if let Some(val) = acc.get(key) {
                                        let mut paths =
                                            env::split_paths(val).collect::<Vec<PathBuf>>();
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
                                },
                            ));
                        }
                        Err(e) => {
                            debug!("Backend method failed: {}", e);
                            return Err(e).wrap_err("Backend exec env method failed");
                        }
                    }
                }

                // Use default vfox behavior for traditional plugins
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

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_vfox_props() {
        let _config = Config::get().await.unwrap();
        let backend = VfoxBackend::from_arg("vfox:version-fox/vfox-golang".into(), None);
        assert_eq!(backend.pathname, "vfox-version-fox-vfox-golang");
        assert_eq!(
            backend.plugin.full,
            Some("vfox:version-fox/vfox-golang".to_string())
        );
    }
}
