use std::fmt::Debug;

use serde_json::Value;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct NPMForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Forge for NPMForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Npm
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<ForgeArg>> {
        Ok(vec!["node".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("npm", "view", self.name(), "versions", "--json").read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("npm", "view", self.name(), "dist-tags", "--json")
                    .full_env(self.dependency_env()?)
                    .read()?;
                let dist_tags: Value = serde_json::from_str(&raw)?;
                let latest = match dist_tags["latest"] {
                    Value::String(ref s) => Some(s.clone()),
                    _ => self.latest_version(Some("latest".into())).unwrap(),
                };
                Ok(latest)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("npm backend")?;

        CmdLineRunner::new("npm")
            .arg("install")
            .arg("-g")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--prefix")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path(&config)?)
            .prepend_path(ctx.ts.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl NPMForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Npm, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            latest_version_cache: CacheManager::new(
                fa.cache_path.join("latest_version-$KEY.msgpack.z"),
            ),
            fa,
        }
    }
}
