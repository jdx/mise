use std::fmt::Debug;

use eyre::eyre;
use ubi::UbiBuilder;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::env::GITHUB_TOKEN;
use crate::github;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct UbiBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

// Uses ubi for installations https://github.com/houseabsolute/ubi
impl Backend for UbiBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Ubi
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec![])
    }

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
        SETTINGS.ensure_experimental("ubi backend")?;
        // Workaround because of not knowing how to pull out the value correctly without quoting
        let path_with_bin = ctx.tv.install_path().join("bin");

        let mut builder = UbiBuilder::new()
            .project(self.name())
            .install_dir(path_with_bin);

        if let Some(token) = &*GITHUB_TOKEN {
            builder = builder.github_token(token);
        }

        let version = &ctx.tv.version;
        if version != "latest" {
            builder = builder.tag(version);
        }

        let exe = std::env::var("MISE_TOOL_OPTS__EXE").unwrap_or_default();
        if !exe.is_empty() {
            builder = builder.exe(&exe);
        }
        let matching = std::env::var("MISE_TOOL_OPTS__MATCHING").unwrap_or_default();
        if !matching.is_empty() {
            builder = builder.matching(&matching);
        }

        let u = builder.build().map_err(|e| eyre!(e))?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()?;
        rt.block_on(u.install_binary()).map_err(|e| eyre!(e))
    }
}

impl UbiBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }
}

fn name_is_url(n: &str) -> bool {
    n.starts_with("http")
}
