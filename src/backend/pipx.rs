use crate::backend::backend_type::BackendType;
use crate::backend::Backend;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, SETTINGS};
use crate::github;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, ToolVersionOptions, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use eyre::Result;
use indexmap::IndexMap;
use itertools::Itertools;
use std::fmt::Debug;
use std::str::FromStr;
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct PIPXBackend {
    ba: BackendArg,
    latest_version_cache: CacheManager<Option<String>>,
}

impl Backend for PIPXBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pipx
    }

    fn ba(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["pipx"])
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["uv"])
    }

    /*
     * Pipx doesn't have a remote version concept across its backends, so
     * we return a single version.
     */
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        match self.tool_name().parse()? {
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
        }
    }

    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version_cache
            .get_or_try_init(|| match self.tool_name().parse()? {
                PipxRequest::Pypi(package) => {
                    let url = format!("https://pypi.org/pypi/{}/json", package);
                    let pkg: PypiPackage = HTTP_FETCH.json(url)?;
                    Ok(Some(pkg.info.version))
                }
                _ => self.latest_version(Some("latest".into())),
            })
            .cloned()
    }

    fn install_version_impl(
        &self,
        ctx: &InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        let config = Config::try_get()?;
        let pipx_request = self
            .tool_name()
            .parse::<PipxRequest>()?
            .pipx_request(&tv.version, &tv.request.options());

        if SETTINGS.pipx.uvx {
            let mut cmd = CmdLineRunner::new("uv")
                .arg("tool")
                .arg("install")
                .arg(pipx_request)
                .with_pr(ctx.pr.as_ref())
                .env("UV_TOOL_DIR", tv.install_path())
                .env("UV_TOOL_BIN_DIR", tv.install_path().join("bin"))
                .envs(ctx.ts.env_with_path(&config)?)
                .prepend_path(ctx.ts.list_paths())?
                // Prepend install path so pipx doesn't issue a warning about missing path
                .prepend_path(vec![tv.install_path().join("bin")])?
                .prepend_path(self.dependency_toolset()?.list_paths())?;
            if let Some(args) = tv.request.options().get("uvx_args") {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        } else {
            let mut cmd = Self::pipx_cmd(
                &config,
                &["install", &pipx_request],
                self,
                &tv,
                ctx.ts,
                &*ctx.pr,
            )?;
            if let Some(args) = tv.request.options().get("pipx_args") {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        }
        Ok(tv)
    }
}

impl PIPXBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            latest_version_cache: CacheManagerBuilder::new(
                ba.cache_path.join("latest_version.msgpack.z"),
            )
            .with_fresh_duration(SETTINGS.fetch_remote_versions_cache())
            .build(),
            ba,
        }
    }

    pub fn reinstall_all() -> Result<()> {
        if SETTINGS.pipx.uvx {
            debug!("skipping pipx reinstall because uvx is enabled");
            return Ok(());
        }
        let config = Config::load()?;
        let ts = ToolsetBuilder::new().build(&config)?;
        let pipx_tools = ts
            .list_installed_versions()?
            .into_iter()
            .filter(|(b, _tv)| b.ba().backend_type() == BackendType::Pipx)
            .collect_vec();
        let pr = MultiProgressReport::get().add("reinstalling pipx tools");
        for (b, tv) in pipx_tools {
            Self::pipx_cmd(
                &config,
                &["reinstall", &tv.ba().tool_name],
                &*b,
                &tv,
                &ts,
                &*pr,
            )?
            .execute()?;
        }
        Ok(())
    }

    fn pipx_cmd<'a>(
        config: &Config,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a dyn SingleReport,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("pipx");
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .env("PIPX_HOME", tv.install_path())
            .env("PIPX_BIN_DIR", tv.install_path().join("bin"))
            .envs(ts.env_with_path(config)?)
            .prepend_path(ts.list_paths())?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset()?.list_paths())
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
    fn extras_from_opts(&self, opts: &ToolVersionOptions) -> String {
        match opts.get("extras") {
            Some(extras) => format!("[{}]", extras),
            None => String::new(),
        }
    }

    fn pipx_request(&self, v: &str, opts: &ToolVersionOptions) -> String {
        let extras = self.extras_from_opts(opts);

        if v == "latest" {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git"),
                PipxRequest::Pypi(package) => format!("{}{}", package, extras),
            }
        } else {
            match self {
                PipxRequest::Git(url) => format!("git+{}.git@{}", url, v),
                PipxRequest::Pypi(package) => format!("{}{}=={}", package, extras, v),
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
