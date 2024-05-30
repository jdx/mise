use eyre::{eyre, Report};
use std::collections::BTreeMap;
use std::env;
use std::fmt::Debug;
use tokio::runtime::Runtime;
use url::Url;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::config::{Config, Settings};
use crate::dirs;
use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, Toolset};
use vfox::Vfox;

#[derive(Debug)]
pub struct VfoxForge {
    fa: ForgeArg,
    vfox: Vfox,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for VfoxForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Vfox
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let plugin = self.vfox.install_plugin_from_url(&self.get_url()?)?;
                let versions = self
                    .runtime()?
                    .block_on(self.vfox.list_available_versions(&plugin.name))?;
                Ok(versions
                    .into_iter()
                    .rev()
                    .map(|v| v.version)
                    .collect::<Vec<String>>())
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("vfox backend")?;
        let plugin = self.vfox.install_plugin_from_url(&self.get_url()?)?;
        self.runtime()?.block_on(self.vfox.install(
            &plugin.name,
            &ctx.tv.version,
            ctx.tv.install_path(),
        ))?;
        Ok(())
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        Ok(self
            .runtime()?
            .block_on(self.vfox.env_keys(self.name(), &tv.version))?
            .into_iter()
            .map(|envkey| (envkey.key, envkey.value))
            .collect())
    }
}

fn vfox_to_url(version: &str) -> eyre::Result<Url> {
    let res = if let Some(caps) = regex!(r#"^([^/]+)/([^/]+)$"#).captures(version) {
        let user = caps.get(1).unwrap().as_str();
        let repo = caps.get(2).unwrap().as_str();
        format!("https://github.com/{user}/{repo}").parse()
    } else {
        version.to_string().parse()
    };
    res.map_err(|e| eyre!("Invalid version: {}: {}", version, e))
}

impl VfoxForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Vfox, &name);
        let mut vfox = Vfox::new();
        vfox.plugin_dir = dirs::PLUGINS.to_path_buf();
        vfox.cache_dir = dirs::CACHE.to_path_buf();
        vfox.download_dir = dirs::DOWNLOADS.to_path_buf();
        vfox.install_dir = dirs::INSTALLS.to_path_buf();
        vfox.temp_dir = env::temp_dir().join("mise-vfox");
        Self {
            vfox,
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            fa,
        }
    }

    fn runtime(&self) -> eyre::Result<Runtime, Report> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()?;
        Ok(rt)
    }

    fn get_url(&self) -> eyre::Result<Url> {
        vfox_to_url(&self.fa.name)
    }
}
