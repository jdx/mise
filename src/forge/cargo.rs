use std::fmt::Debug;

use serde_json::Deserializer;
use url::Url;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::Settings;
use crate::forge::{Forge, ForgeType};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::{dirs, file};

#[derive(Debug)]
pub struct CargoForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Forge for CargoForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Cargo
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = HTTP_FETCH.get_text(get_crate_url(self.name())?)?;
                let stream = Deserializer::from_str(&raw).into_iter::<CrateVersion>();
                let mut versions = vec![];
                for v in stream {
                    let v = v?;
                    if !v.yanked {
                        versions.push(v.vers);
                    }
                }
                Ok(versions)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let settings = Settings::get();
        settings.ensure_experimental()?;
        let cmd = match settings.cargo_binstall {
            true if file::which("cargo-binstall").is_some() => {
                CmdLineRunner::new("cargo-binstall").arg("-y")
            }
            _ => CmdLineRunner::new("cargo").arg("install"),
        };

        cmd.arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--root")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .execute()?;

        Ok(())
    }
}

impl CargoForge {
    pub fn new(fa: ForgeArg) -> Self {
        let cache_dir = dirs::CACHE.join(&fa.id);
        Self {
            fa,
            remote_version_cache: CacheManager::new(cache_dir.join("remote_versions.msgpack.z")),
        }
    }
}

fn get_crate_url(n: &str) -> eyre::Result<Url> {
    let n = n.to_lowercase();
    let url = match n.len() {
        1 => format!("https://index.crates.io/1/{n}"),
        2 => format!("https://index.crates.io/2/{n}"),
        3 => format!("https://index.crates.io/3/{}/{n}", &n[..1]),
        _ => format!("https://index.crates.io/{}/{}/{n}", &n[..2], &n[2..4]),
    };
    Ok(url.parse()?)
}

#[derive(Debug, serde::Deserialize)]
struct CrateVersion {
    //name: String,
    vers: String,
    yanked: bool,
}
