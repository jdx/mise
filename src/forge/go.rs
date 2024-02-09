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
                let mut mod_path = Some(self.name());

                while let Some(cur_mod_path) = mod_path {
                    let raw =
                        cmd!("go", "list", "-m", "-versions", "-json", cur_mod_path).read()?;

                    let result = serde_json::from_str::<GoModInfo>(&raw);
                    if let Ok(mod_info) = result {
                        return Ok(mod_info.versions);
                    }

                    mod_path = trim_after_last_slash(cur_mod_path);
                }

                Err(eyre!("couldn't find module versions"))
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("go backend")?;

        CmdLineRunner::new("go")
            .arg("install")
            .arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
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

fn trim_after_last_slash(s: &str) -> Option<&str> {
    match s.rsplit_once('/') {
        Some((new_path, _)) => Some(new_path),
        None => None,
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GoModInfo {
    versions: Vec<String>,
}
