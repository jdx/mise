use eyre::Ok;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Debug;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;

#[derive(Debug)]
pub struct BunBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Backend for BunBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Bun
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["bun".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| {
                let url = format!("https://registry.npmjs.org/{}", self.name());
                let package: NpmPackage = HTTP_FETCH.json(url)?;
                let versions = package.versions.keys().cloned().collect();
                Ok(versions)
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| {
                let url = format!("https://registry.npmjs.org/{}", self.name());
                let package: NpmPackage = HTTP_FETCH.json(url)?;
                let latest = match package.dist_tags.get("latest") {
                    Some(s) => Some(s.clone()),
                    None => self.latest_version(Some("latest".into())).unwrap(),
                };
                Ok(latest)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;

        CmdLineRunner::new("bun")
            .arg("install")
            .arg(format!("{}@{}", self.name(), ctx.tv.version))
            .arg("--cwd")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path(&config)?)
            .prepend_path(ctx.ts.list_paths())?
            .prepend_path(self.dependency_toolset()?.list_paths())?
            .execute()?;

        Ok(())
    }

    fn list_bin_paths(
        &self,
        tv: &crate::toolset::ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        Ok(vec![tv.install_path().join("node_modules").join(".bin")])
    }
}

impl BunBackend {
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

#[derive(Debug, Deserialize)]
struct NpmPackage {
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<String, String>,
    versions: HashMap<String, Value>,
}
