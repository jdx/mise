use std::fmt::Debug;

use eyre::eyre;
use ubi::UbiBuilder;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::env::GITHUB_TOKEN;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;
use crate::{github, http};

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
        let opts = ctx.tv.request.options();
        let mut v = ctx.tv.version.to_string();

        if let Err(err) = github::get_release(self.name(), &ctx.tv.version) {
            if http::error_code(&err) == Some(404) {
                // no tag found, try prefixing with 'v'
                v = format!("v{v}");
            }
        }

        // Workaround because of not knowing how to pull out the value correctly without quoting
        let path_with_bin = ctx.tv.install_path().join("bin");

        let mut builder = UbiBuilder::new()
            .project(self.name())
            .install_dir(path_with_bin);

        if let Some(token) = &*GITHUB_TOKEN {
            builder = builder.github_token(token);
        }

        if v != "latest" {
            builder = builder.tag(&v);
        }

        if let Some(exe) = opts.get("exe") {
            builder = builder.exe(exe);
        }
        if let Some(matching) = opts.get("matching") {
            builder = builder.matching(matching);
        }

        let u = builder.build().map_err(|e| eyre!(e))?;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
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
