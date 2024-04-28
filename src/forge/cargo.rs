use std::fmt::Debug;

use serde_json::Deserializer;
use url::Url;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file;
use crate::forge::{Forge, ForgeType};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;

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

    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec!["cargo".into(), "rust".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
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

    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        if !is_cargo_installed() {
            bail!(
                "cargo is not installed. Please install it in order to install {}",
                self.name()
            );
        }
        Ok(())
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("cargo backend")?;
        let cmd = match settings.cargo_binstall {
            true if file::which_non_pristine("cargo-binstall").is_some() => {
                CmdLineRunner::new("cargo-binstall").arg("-y")
            }
            _ => CmdLineRunner::new("cargo").arg("install"),
        };

        cmd.arg(&format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--root")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .prepend_path(ctx.ts.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl CargoForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Cargo, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            fa,
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

fn is_cargo_installed() -> bool {
    file::which("cargo").is_some()
}

#[derive(Debug, serde::Deserialize)]
struct CrateVersion {
    //name: String,
    vers: String,
    yanked: bool,
}
