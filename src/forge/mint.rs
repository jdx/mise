use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::forge::{Forge, ForgeType};
use crate::github;

#[derive(Debug)]
pub struct MintForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// Uses Mint for installations https://github.com/yonaskolb/Mint
impl Forge for MintForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Mint
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tvr: &crate::toolset::ToolRequest) -> eyre::Result<Vec<ForgeArg>> {
        // TODO: mint as dependencies (first need to impl asdf plugin for mint)
        // TODO: swift as dependencies (first need to impl asdf plugin for swift)
        Ok(vec![])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                Ok(github::list_releases(self.name())?
                   .into_iter()
                   .map(|r| r.tag_name)
                   .rev()
                   .collect())
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &crate::install_context::InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental("mint backend")?;

        let install_name = if ctx.tv.version == "latest" {
            self.name().to_string()
        } else {
            format!("{}@{}", self.name(), &ctx.tv.version)
        };

        CmdLineRunner::new("mint")
            .arg("install")
            .arg(install_name)
            .env("MINT_PATH", ctx.tv.install_path())
            .env("MINT_LINK_PATH", ctx.tv.install_path().join("bin"))
            .with_pr(ctx.pr.as_ref())
            .execute()?;

        Ok(())
    }
}

impl MintForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Mint, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            fa,
        }
    }
}
