use serde_json::Value;
use std::fmt::Debug;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::Config;
use crate::config::SETTINGS;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct NPMBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

const PROGRAM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

impl Backend for NPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Npm
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["node".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = cmd!(PROGRAM, "view", self.name(), "versions", "--json").read()?;
                let versions: Vec<String> = serde_json::from_str(&raw)?;
                Ok(versions)
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| {
                let raw = cmd!(PROGRAM, "view", self.name(), "dist-tags", "--json")
                    .full_env(self.dependency_env()?)
                    .read()?;
                let dist_tags: Value = serde_json::from_str(&raw)?;
                let latest = match dist_tags["latest"] {
                    Value::String(ref s) => Some(s.clone()),
                    _ => self.latest_version(Some("latest".into())).unwrap(),
                };
                Ok(latest)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;

        CmdLineRunner::new(PROGRAM)
            .arg("install")
            .arg("-g")
            .arg(format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--prefix")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path(&config)?)
            .prepend_path(ctx.ts.list_paths())?
            .prepend_path(self.dependency_toolset()?.list_paths())?
            .execute()?;

        Ok(())
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
            remote_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            latest_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("latest_version.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }
}
