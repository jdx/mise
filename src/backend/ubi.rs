use std::fmt::Debug;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::{self, GITHUB_TOKEN};
use crate::github;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct UbiBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
// it can be installed via mise install cargo:ubi-cli
impl Backend for UbiBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["ubi".into()])
    }

    // TODO: v0.0.3 is stripped of 'v' such that it reports incorrectly in tool :-/
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if name_is_url(self.name()) {
            Ok(vec!["latest".to_string()])
        } else {
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
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        let version = &ctx.tv.version;
        settings.ensure_experimental("ubi backend")?;
        // Workaround because of not knowing how to pull out the value correctly without quoting
        let path_with_bin = ctx.tv.install_path().join("bin");

        let mut cmd = CmdLineRunner::new("ubi")
            .arg("--in")
            .arg(path_with_bin)
            .arg("--project")
            .arg(self.name())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path(&config)?)
            .prepend_path(ctx.ts.list_paths())?
            .prepend_path(self.depedency_toolset()?.list_paths())?;

        if let Some(token) = &*GITHUB_TOKEN {
            cmd = cmd.env("GITHUB_TOKEN", token);
        }

        if version != "latest" {
            cmd = cmd.arg("--tag").arg(version);
        }

        cmd.execute()
    }
}

impl UbiBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE)
            .build(),
            ba,
        }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}
