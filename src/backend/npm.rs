use crate::Result;
use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use serde_json::Value;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex as TokioMutex;

#[derive(Debug)]
pub struct NPMBackend {
    ba: Arc<BackendArg>,
    // use a mutex to prevent deadlocks that occurs due to reentrant cache access
    latest_version_cache: TokioMutex<CacheManager<Option<String>>>,
}

const NPM_PROGRAM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

#[async_trait]
impl Backend for NPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Npm
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["node", "bun"])
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        timeout::run_with_timeout_async(
            async || {
                let raw = cmd!(NPM_PROGRAM, "view", self.tool_name(), "versions", "--json")
                    .full_env(self.dependency_env(config).await?)
                    .read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        let cache = self.latest_version_cache.lock().await;
        let this = self;
        timeout::run_with_timeout_async(
            async || {
                cache
                    .get_or_try_init_async(async || {
                        let raw =
                            cmd!(NPM_PROGRAM, "view", this.tool_name(), "dist-tags", "--json")
                                .full_env(this.dependency_env(config).await?)
                                .read()?;
                        let dist_tags: Value = serde_json::from_str(&raw)?;
                        match dist_tags["latest"] {
                            Value::String(ref s) => Ok(Some(s.clone())),
                            _ => this.latest_version(config, Some("latest".into())).await,
                        }
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        if Settings::get().npm.bun {
            CmdLineRunner::new("bun")
                .arg("install")
                .arg(format!("{}@{}", self.tool_name(), tv.version))
                .arg("--cwd")
                .arg(tv.install_path())
                .arg("--global")
                .arg("--trust")
                .with_pr(&ctx.pr)
                .envs(ctx.ts.env_with_path(&ctx.config).await?)
                .env("BUN_INSTALL_GLOBAL_DIR", tv.install_path())
                .env("BUN_INSTALL_BIN", tv.install_path().join("bin"))
                .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                .prepend_path(
                    self.dependency_toolset(&ctx.config)
                        .await?
                        .list_paths(&ctx.config)
                        .await,
                )?
                .execute()?;
        } else {
            CmdLineRunner::new(NPM_PROGRAM)
                .arg("install")
                .arg("-g")
                .arg(format!("{}@{}", self.tool_name(), tv.version))
                .arg("--prefix")
                .arg(tv.install_path())
                .with_pr(&ctx.pr)
                .envs(ctx.ts.env_with_path(&ctx.config).await?)
                .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                .prepend_path(
                    self.dependency_toolset(&ctx.config)
                        .await?
                        .list_paths(&ctx.config)
                        .await,
                )?
                .execute()?;
        }
        Ok(tv)
    }

    #[cfg(windows)]
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &crate::toolset::ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        Ok(vec![tv.install_path()])
    }
}

impl NPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            latest_version_cache: TokioMutex::new(
                CacheManagerBuilder::new(ba.cache_path.join("latest_version.msgpack.z"))
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .build(),
            ),
            ba: Arc::new(ba),
        }
    }
}
