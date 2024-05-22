use std::fmt::Debug;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::GITHUB_TOKEN;
use crate::forge::{Forge, ForgeType};
use crate::github;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct UbiForge {
    fa: ForgeArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
// it can be installed via mise install cargo:ubi
#[async_trait]
impl Forge for UbiForge {
    fn get_type(&self) -> ForgeType {
        ForgeType::Ubi
    }

    fn fa(&self) -> &ForgeArg {
        &self.fa
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<ForgeArg>> {
        Ok(vec!["cargo:ubi".into()])
    }

    // TODO: v0.0.3 is stripped of 'v' such that it reports incorrectly in tool :-/
    async fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if name_is_url(self.name()) {
            Ok(vec!["latest".to_string()])
        } else {
            self.remote_version_cache
                .get_or_try_init(|| async {
                    Ok(github::list_releases(self.name())
                        .await?
                        .into_iter()
                        .map(|r| r.tag_name)
                        .rev()
                        .collect())
                })
                .await
                .cloned()
        }
    }

    async fn install_version_impl<'a>(&'a self, ctx: &'a InstallContext<'a>) -> eyre::Result<()> {
        let config = Config::try_get().await?;
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
            .prepend_path(ctx.ts.list_paths())?;

        if let Some(token) = &*GITHUB_TOKEN {
            cmd = cmd.env("GITHUB_TOKEN", token);
        }

        if version != "latest" {
            cmd = cmd.arg("--tag").arg(version);
        }

        cmd.execute()
    }
}

impl UbiForge {
    pub fn new(name: String) -> Self {
        let fa = ForgeArg::new(ForgeType::Ubi, &name);
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            ),
            fa,
        }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}
