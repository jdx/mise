use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;

#[derive(Debug)]
pub struct GoForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for GoForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Go
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("go", "list", "-m", "-versions", "-json", self.name()).read()?;
                let mod_info: GoModInfo = serde_json::from_str(&raw)?;
                Ok(mod_info.versions)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental()?;

        CmdLineRunner::new("go")
            .arg("install")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .with_pr(ctx.pr.as_ref())
            .envs(&config.env)
            .env("GOPATH", ctx.tv.cache_path())
            .env("GOBIN", ctx.tv.install_path().join("bin"))
            .execute()?;

        Ok(())
    }
}

impl GoForge {
    pub fn new(fa: ForgeArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            fa,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
