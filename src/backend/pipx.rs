use indexmap::IndexMap;
use itertools::Itertools;
use std::fmt::Debug;
use std::str::FromStr;
use versions::Versioning;

use crate::backend::{Backend, BackendType};
use crate::cache::CacheManager;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolRequest;
use crate::{env, github};

#[derive(Debug)]
pub struct PIPXBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Backend for PIPXBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pipx
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["pipx".into(), "uv".into()])
    }

    /*
     * Pipx doesn't have a remote version concept across its backends, so
     * we return a single version.
     */
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.remote_version_cache
            .get_or_try_init(|| match self.name().parse()? {
                PipxRequest::Pypi(package) => {
                    let url = format!("https://pypi.org/pypi/{}/json", package);
                    let data: PypiPackage = HTTP_FETCH.json(url)?;
                    let versions = data
                        .releases
                        .keys()
                        .map(|v| v.to_string())
                        .sorted_by_cached_key(|v| Versioning::new(v))
                        .collect();
                    Ok(versions)
                }
                PipxRequest::Git(url) if url.starts_with("https://github.com/") => {
                    let repo = url.strip_prefix("https://github.com/").unwrap();
                    let data = github::list_releases(repo)?;
                    Ok(data.into_iter().rev().map(|r| r.tag_name).collect())
                }
                PipxRequest::Git { .. } => Ok(vec!["latest".to_string()]),
            })
            .cloned()
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| match self.name().parse()? {
                PipxRequest::Pypi(package) => {
                    let url = format!("https://pypi.org/pypi/{}/json", package);
                    let pkg: PypiPackage = HTTP_FETCH.json(url)?;
                    Ok(Some(pkg.info.version))
                }
                _ => self.latest_version(Some("latest".into())),
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::get();
        settings.ensure_experimental("pipx backend")?;
        let pipx_request = self
            .name()
            .parse::<PipxRequest>()?
            .pipx_request(&ctx.tv.version);

        if settings.pipx_uvx {
            CmdLineRunner::new("uv")
                .arg("tool")
                .arg("install")
                .arg(pipx_request)
                .with_pr(ctx.pr.as_ref())
                .env("UV_TOOL_DIR", ctx.tv.install_path())
                .env("UV_TOOL_BIN_DIR", ctx.tv.install_path().join("bin"))
                .envs(ctx.ts.env_with_path(&config)?)
                .prepend_path(ctx.ts.list_paths())?
                // Prepend install path so pipx doesn't issue a warning about missing path
                .prepend_path(vec![ctx.tv.install_path().join("bin")])?
                .execute()?;
        } else {
            CmdLineRunner::new("pipx")
                .arg("install")
                .arg(pipx_request)
                .with_pr(ctx.pr.as_ref())
                .env("PIPX_HOME", ctx.tv.install_path())
                .env("PIPX_BIN_DIR", ctx.tv.install_path().join("bin"))
                .envs(ctx.ts.env_with_path(&config)?)
                .prepend_path(ctx.ts.list_paths())?
                // Prepend install path so pipx doesn't issue a warning about missing path
                .prepend_path(vec![ctx.tv.install_path().join("bin")])?
                .execute()?;
        }
        Ok(())
    }
}

impl PIPXBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                ba.cache_path.join("remote_versions-$KEY.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            latest_version_cache: CacheManager::new(
                ba.cache_path.join("latest_version-$KEY.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            ba,
        }
    }
}

enum PipxRequest {
    /// git+https://github.com/psf/black.git@24.2.0
    /// psf/black@24.2.0
    Git(String),
    /// black@24.2.0
    Pypi(String),
}

impl PipxRequest {
    fn pipx_request(&self, v: &str) -> String {
        if v == "latest" {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git"),
                PipxRequest::Pypi(package) => package.to_string(),
            }
        } else {
            match self {
                PipxRequest::Git(url) => format!("git+{}.git@{}", url, v),
                PipxRequest::Pypi(package) => format!("{}=={}", package, v),
            }
        }
    }
}

impl FromStr for PipxRequest {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(cap) = regex!(r"(git\+)(.*)(\.git)").captures(s) {
            Ok(PipxRequest::Git(cap.get(2).unwrap().as_str().to_string()))
        } else if s.contains('/') {
            Ok(PipxRequest::Git(format!("https://github.com/{s}")))
        } else {
            Ok(PipxRequest::Pypi(s.to_string()))
        }
    }
}

#[derive(serde::Deserialize)]
struct PypiPackage {
    releases: IndexMap<String, Vec<PypiRelease>>,
    info: PypiInfo,
}

#[derive(serde::Deserialize)]
struct PypiInfo {
    version: String,
}

#[derive(serde::Deserialize)]
struct PypiRelease {}
