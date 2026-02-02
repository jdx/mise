use crate::{env, plugins::PluginEnum, timeout};
use async_trait::async_trait;
use eyre::WrapErr;
use heck::ToKebabCase;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use tokio::sync::RwLock;

use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::{Config, Settings};
use crate::dirs;
use crate::env_diff::EnvMap;
use crate::install_context::InstallContext;
use crate::plugins::Plugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::toolset::{ToolVersion, Toolset, install_state};
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

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let this = self;
        timeout::run_with_timeout_async(
            || async {
                let (vfox, _log_rx) = this.plugin.vfox();
                this.ensure_plugin_installed(config).await?;

                // Use backend methods if the plugin supports them
                if this.is_backend_plugin() {
                    Settings::get().ensure_experimental("custom backends")?;
                    debug!("Using backend method for plugin: {}", this.pathname);
                    let tool_name = this.get_tool_name()?;
                    let versions = vfox
                        .backend_list_versions(&this.pathname, tool_name)
                        .await
                        .wrap_err("Backend list versions method failed")?;
                    return Ok(versions
                        .into_iter()
                        .map(|v| VersionInfo {
                            version: v,
                            ..Default::default()
                        })
                        .collect());
                }

                // Use default vfox behavior for traditional plugins
                let versions = vfox.list_available_versions(&this.pathname).await?;
                Ok(versions
                    .into_iter()
                    .rev()
                    .map(|v| VersionInfo {
                        version: v.version,
                        rolling: v.rolling,
                        checksum: v.checksum,
                        ..Default::default()
                    })
                    .collect())
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
        if self.is_backend_plugin() {
            Settings::get().ensure_experimental("custom backends")?;
            let tool_name = self.get_tool_name()?;
            vfox.backend_install(
                &self.pathname,
                tool_name,
                &tv.version,
                tv.install_path(),
                tv.download_path(),
            )
            .await
            .wrap_err("Backend install method failed")?;
            return Ok(tv);
        }

        // Use default vfox behavior for traditional plugins
        let result = vfox
            .install(&self.pathname, &tv.version, tv.install_path())
            .await?;

        // Store checksum for rolling version tracking
        if let Some(sha256) = result.sha256
            && let Err(e) = install_state::write_checksum(&self.ba.short, &tv.version, &sha256)
        {
            warn!("failed to write checksum for {}: {e}", tv);
        }

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

    async fn idiomatic_filenames(&self) -> eyre::Result<Vec<String>> {
        let (vfox, _log_rx) = self.plugin.vfox();

        let metadata = vfox.metadata(&self.pathname).await?;
        Ok(metadata.legacy_filenames)
    }

    async fn parse_idiomatic_file(&self, path: &Path) -> eyre::Result<String> {
        let (vfox, _log_rx) = self.plugin.vfox();
        let response = vfox.parse_legacy_file(&self.pathname, path).await?;
        response.version.ok_or_else(|| {
            eyre::eyre!(
                "Version for {} not found in '{}'",
                self.pathname,
                path.display()
            )
        })
    }

    async fn get_tarball_url(
        &self,
        tv: &ToolVersion,
        target: &PlatformTarget,
    ) -> eyre::Result<Option<String>> {
        let config = Config::get().await?;
        self.ensure_plugin_installed(&config).await?;

        // Map mise platform names to vfox platform names
        let os = match target.os_name() {
            "macos" => "darwin",
            os => os,
        };
        let arch = match target.arch_name() {
            "x64" => "amd64",
            arch => arch,
        };

        let (vfox, _log_rx) = self.plugin.vfox();
        let pre_install = vfox
            .pre_install_for_platform(&self.pathname, &tv.version, os, arch)
            .await?;

        Ok(pre_install.url)
    }
}

impl VfoxBackend {
    fn is_backend_plugin(&self) -> bool {
        matches!(&self.plugin_enum, PluginEnum::VfoxBackend(_))
    }

    fn get_tool_name(&self) -> eyre::Result<&str> {
        self.tool_name
            .as_deref()
            .ok_or_else(|| eyre::eyre!("VfoxBackend requires a tool name (plugin:tool format)"))
    }

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
        let opts = tv.request.options();
        let opts_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            opts.hash(&mut hasher);
            hasher.finish()
        };
        let key = format!("{}:{:x}", tv, opts_hash);
        let cache_file = format!("exec_env_{:x}.msgpack.z", opts_hash);
        if !self.exec_env_cache.read().await.contains_key(&key) {
            let mut caches = self.exec_env_cache.write().await;
            caches.insert(
                key.clone(),
                CacheManagerBuilder::new(tv.cache_path().join(&cache_file))
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
                let env_keys = if self.is_backend_plugin() {
                    let tool_name = self.get_tool_name()?;
                    vfox.backend_exec_env(&self.pathname, tool_name, &tv.version, tv.install_path())
                        .await
                        .wrap_err("Backend exec env method failed")?
                } else {
                    vfox.env_keys(&self.pathname, &tv.version, &opts.opts)
                        .await?
                };

                Ok(env_keys
                    .into_iter()
                    .fold(BTreeMap::new(), |mut acc, env_key| {
                        let key = &env_key.key;
                        if let Some(val) = acc.get(key) {
                            let mut paths = env::split_paths(val).collect::<Vec<PathBuf>>();
                            paths.push(PathBuf::from(&env_key.value));
                            acc.insert(
                                env_key.key.clone(),
                                env::join_paths(paths)
                                    .unwrap()
                                    .to_string_lossy()
                                    .to_string(),
                            );
                        } else {
                            acc.insert(key.clone(), env_key.value.clone());
                        }
                        acc
                    }))
            })
            .await
            .cloned()
    }

    async fn ensure_plugin_installed(&self, config: &Arc<Config>) -> eyre::Result<()> {
        self.plugin
            .ensure_installed(config, &MultiProgressReport::get(), false, false)
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
