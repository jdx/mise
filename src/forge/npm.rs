use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::dirs;
use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;

#[derive(Debug)]
pub struct NPMForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for NPMForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Npm
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = cmd!("npm", "view", self.name(), "versions", "--json").read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental()?;

        CmdLineRunner::new("npm")
            .arg("install")
            .arg("-g")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--prefix")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .execute()?;

        Ok(())
    }
}

impl NPMForge {
    pub fn new(fa: ForgeArg) -> Self {
        let cache_dir = dirs::CACHE.join(&fa.id);
        Self {
            fa,
            remote_version_cache: CacheManager::new(cache_dir.join("remote_versions.msgpack.z")),
        }
    }
}
