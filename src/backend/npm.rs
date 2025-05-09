use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use serde_json::Value;
use std::fmt::Debug;
use std::sync::Mutex;

#[derive(Debug)]
pub struct NPMBackend {
    ba: BackendArg,
    // use a mutex to prevent deadlocks that occurs due to reentrant cache access
    latest_version_cache: Mutex<CacheManager<Option<String>>>,
}

const NPM_PROGRAM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

impl Backend for NPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Npm
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["node", "bun"])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        timeout::run_with_timeout(
            || {
                let raw = cmd!(NPM_PROGRAM, "view", self.tool_name(), "versions", "--json")
                    .full_env(self.dependency_env()?)
                    .read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            },
            SETTINGS.fetch_remote_versions_timeout(),
        )
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        let fetch = || {
            let raw = cmd!(NPM_PROGRAM, "view", self.tool_name(), "dist-tags", "--json")
                .full_env(self.dependency_env()?)
                .read()?;
            let dist_tags: Value = serde_json::from_str(&raw)?;
            match dist_tags["latest"] {
                Value::String(ref s) => Ok(Some(s.clone())),
                _ => self.latest_version(Some("latest".into())),
            }
        };
        timeout::run_with_timeout(
            || {
                if let Ok(cache) = self.latest_version_cache.try_lock() {
                    cache.get_or_try_init(fetch).cloned()
                } else {
                    fetch()
                }
            },
            SETTINGS.fetch_remote_versions_timeout(),
        )
    }

    fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> eyre::Result<ToolVersion> {
        let config = Config::try_get()?;

        if SETTINGS.npm.bun {
            CmdLineRunner::new("bun")
                .arg("install")
                .arg(format!("{}@{}", self.tool_name(), tv.version))
                .arg("--cwd")
                .arg(tv.install_path())
                .arg("--global")
                .arg("--trust")
                .with_pr(&ctx.pr)
                .envs(ctx.ts.env_with_path(&config)?)
                .env("BUN_INSTALL_GLOBAL_DIR", tv.install_path())
                .env("BUN_INSTALL_BIN", tv.install_path().join("bin"))
                .prepend_path(ctx.ts.list_paths())?
                .prepend_path(self.dependency_toolset()?.list_paths())?
                .execute()?;
        } else {
            CmdLineRunner::new(NPM_PROGRAM)
                .arg("install")
                .arg("-g")
                .arg(format!("{}@{}", self.tool_name(), tv.version))
                .arg("--prefix")
                .arg(tv.install_path())
                .with_pr(&ctx.pr)
                .envs(ctx.ts.env_with_path(&config)?)
                .prepend_path(ctx.ts.list_paths())?
                .prepend_path(self.dependency_toolset()?.list_paths())?
                .execute()?;
        }
        Ok(tv)
    }

    #[cfg(windows)]
    fn list_bin_paths(
        &self,
        tv: &crate::toolset::ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        Ok(vec![tv.install_path()])
    }
}

impl NPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            latest_version_cache: Mutex::new(
                CacheManagerBuilder::new(ba.cache_path.join("latest_version.msgpack.z"))
                    .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
                    .build(),
            ),
            ba,
        }
    }
}
