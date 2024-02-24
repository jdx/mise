use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::forge::{Forge, ForgeType};
use crate::install_context::InstallContext;

#[derive(Debug)]
pub struct SwiftForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for SwiftForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Npm
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        eyre::Result::Ok((vec![]))
        // TODO
        // 1. Look up the package in this list: https://raw.githubusercontent.com/SwiftPackageIndex/PackageList/main/packages.json
        //      Since there's no API (yet) we can match the name against the repository name.
        // 2. If the package is found, we can then get the versions from the list of tags in the repository.
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("swift backend")?;

        CmdLineRunner::new("npm")
            .arg("install")
            .arg("-g")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--prefix")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .prepend_path(ctx.ts.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl SwiftForge {
    pub fn new(fa: ForgeArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            fa,
        }
    }
}
