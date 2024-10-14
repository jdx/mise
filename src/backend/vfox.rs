use crate::env;
use eyre::{Report, WrapErr};
use heck::ToKebabCase;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use tokio::runtime::Runtime;
use url::Url;

use crate::backend::{ABackend, Backend, BackendList, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::{Config, SETTINGS};
use crate::git::Git;
use crate::install_context::InstallContext;
use crate::plugins::PluginType;
use crate::toolset::{ToolVersion, Toolset};
use crate::{dirs, file, registry};
use vfox::Vfox;
use xx::regex;

#[derive(Debug)]
pub struct VfoxBackend {
    ba: BackendArg,
    vfox: Vfox,
    plugin_path: PathBuf,
    remote_version_cache: CacheManager<Vec<String>>,
    exec_env_cache: CacheManager<BTreeMap<String, String>>,
    repo: OnceLock<Mutex<Git>>,
    pathname: String,
}

impl Backend for VfoxBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Vfox
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Vfox
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                self.ensure_plugin_installed()?;
                let versions = self
                    .runtime()?
                    .block_on(self.vfox.list_available_versions(&self.pathname))?;
                Ok(versions
                    .into_iter()
                    .rev()
                    .map(|v| v.version)
                    .collect::<Vec<String>>())
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        self.ensure_plugin_installed()?;
        self.runtime()?.block_on(self.vfox.install(
            &self.pathname,
            &ctx.tv.version,
            ctx.tv.install_path(),
        ))?;
        Ok(())
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
        self._exec_env(tv).cloned()
    }
}

fn vfox_to_url(name: &str) -> eyre::Result<Url> {
    if let Some(full) = registry::REGISTRY_VFOX.get(name) {
        // bun -> version-fox/vfox-bun
        return vfox_to_url(full.split_once(':').unwrap().1);
    }
    let res = if let Some(caps) = regex!(r#"^([^/]+)/([^/]+)$"#).captures(name) {
        let user = caps.get(1).unwrap().as_str();
        let repo = caps.get(2).unwrap().as_str();
        format!("https://github.com/{user}/{repo}").parse()
    } else {
        name.to_string().parse()
    };
    res.wrap_err_with(|| format!("Invalid version: {name}"))
}

impl VfoxBackend {
    pub fn list() -> eyre::Result<BackendList> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .into_par_iter()
            .filter(|name| dirs::PLUGINS.join(name).join("metadata.lua").exists())
            .map(|name| Arc::new(Self::from_arg(name.into())) as ABackend)
            .collect())
    }

    pub fn from_arg(ba: BackendArg) -> Self {
        let mut vfox = Vfox::new();
        vfox.plugin_dir = dirs::PLUGINS.to_path_buf();
        vfox.cache_dir = dirs::CACHE.to_path_buf();
        vfox.download_dir = dirs::DOWNLOADS.to_path_buf();
        vfox.install_dir = dirs::INSTALLS.to_path_buf();
        vfox.temp_dir = env::temp_dir().join("mise-vfox");
        let pathname = ba.short.to_kebab_case();
        let plugin_path = dirs::PLUGINS.join(&pathname);
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .with_fresh_file(dirs::DATA.to_path_buf())
            .with_fresh_file(plugin_path.to_path_buf())
            .with_fresh_file(ba.installs_path.to_path_buf())
            .build(),
            exec_env_cache: CacheManagerBuilder::new(ba.cache_path.join("exec_env.msgpack.z"))
                .with_fresh_file(dirs::DATA.to_path_buf())
                .with_fresh_file(plugin_path.to_path_buf())
                .with_fresh_file(ba.installs_path.to_path_buf())
                .build(),
            repo: OnceLock::new(),
            ba,
            vfox,
            plugin_path,
            pathname,
        }
    }

    fn runtime(&self) -> eyre::Result<Runtime, Report> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()?;
        Ok(rt)
    }

    fn _exec_env(&self, tv: &ToolVersion) -> eyre::Result<&BTreeMap<String, String>> {
        self.exec_env_cache.get_or_try_init(|| {
            self.ensure_plugin_installed()?;
            Ok(self
                .runtime()?
                .block_on(self.vfox.env_keys(&self.pathname, &tv.version))?
                .into_iter()
                .map(|envkey| (envkey.key, envkey.value))
                .collect())
        })
    }

    fn get_url(&self) -> eyre::Result<Url> {
        if let Ok(Some(url)) = self.repo().map(|r| r.get_remote_url()) {
            return Ok(Url::parse(&url)?);
        }
        vfox_to_url(&self.ba.name)
    }

    fn repo(&self) -> eyre::Result<MutexGuard<Git>> {
        if let Some(repo) = self.repo.get() {
            Ok(repo.lock().unwrap())
        } else {
            let repo = Mutex::new(Git::new(self.plugin_path.clone()));
            self.repo.set(repo).unwrap();
            self.repo()
        }
    }

    fn ensure_plugin_installed(&self) -> eyre::Result<()> {
        if !self.plugin_path.exists() {
            let url = self.get_url()?;
            trace!("Cloning vfox plugin: {url}");
            self.repo()?.clone(url.as_str())?;
        }
        Ok(())
    }
}
