use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};

use crate::file;
use crate::forge::{Forge, ForgeType};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use serde_json::Value;
use url::Url;

#[derive(Debug)]
pub struct UbiForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
// it can be installed via mise install cargo:ubi
// TODO: doesn't currently work when ubi installed via mise :-/
impl Forge for UbiForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Ubi
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec!["cargo:ubi".into()])
    }

    // TODO: v0.0.3 is stripped of 'v' such that it reports incorrectly in tool :-/
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if name_is_url(self.name()) {
            Ok(vec!["latest".to_string()])
        } else {
            self.remote_version_cache
                .get_or_try_init(|| {
                    let url = get_binary_url(self.name())?;
                    let raw = HTTP_FETCH.get_text(url)?;
                    let releases: Value = serde_json::from_str(&raw)?;
                    let mut versions = vec![];
                    for v in releases.as_array().unwrap() {
                        versions.push(v["tag_name"].as_str().unwrap().to_string());
                    }
                    Ok(versions)
                })
                .cloned()
        }
    }

    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        if !is_ubi_installed() {
            bail!(
                "ubi is not installed. Please install it in order to install {}",
                self.name()
            );
        }
        Ok(())
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        if name_is_url(self.name()) {
            Ok(Some("latest".to_string()))
        } else {
            self.latest_version_cache
                .get_or_try_init(|| Ok(Some(self.list_remote_versions()?.last().unwrap().into())))
                .cloned()
        }
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("ubi backend")?;
        // Workaround because of not knowing how to pull out the value correctly without quoting
        let matching_version = self
            .list_remote_versions()?
            .into_iter()
            .find(|v| v.contains(&ctx.tv.version))
            .unwrap()
            .replace('"', "");
        let path_with_bin = ctx.tv.install_path().join("bin");

        let cmd = CmdLineRunner::new("ubi")
            .arg("--in")
            .arg(path_with_bin)
            .arg("--project")
            .arg(self.name())
            .with_pr(ctx.pr.as_ref())
            .envs(config.env()?)
            .prepend_path(ctx.ts.list_paths())?;

        if name_is_url(self.name()) {
            cmd.execute()?;
        } else {
            cmd.arg("--tag").arg(matching_version).execute()?;
        }

        Ok(())
    }
}

impl UbiForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Ubi, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            ),
            latest_version_cache: CacheManager::new(fa.cache_path.join("latest_version.msgpack.z")),
            fa,
        }
    }
}

// NOTE: Releases version works, but url approach isn't working yet
// and related to https://github.com/jdx/mise/pull/1926
fn get_binary_url(n: &str) -> eyre::Result<Url> {
    match (n.starts_with("http"), n.split('/').count()) {
        (true, _) => Ok(n.parse()?),
        (_, 2) => {
            let url = format!("https://api.github.com/repos/{n}/releases");
            Ok(url.parse()?)
        }
        (_, _) => Err(eyre::eyre!("Invalid binary name: {}", n)),
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}

fn is_ubi_installed() -> bool {
    file::which("ubi").is_some()
}
