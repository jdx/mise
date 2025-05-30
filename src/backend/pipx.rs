use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::github;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolVersion, ToolVersionOptions, Toolset, ToolsetBuilder};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{backend::Backend, timeout};
use async_trait::async_trait;
use eyre::{Result, eyre};
use indexmap::IndexMap;
use itertools::Itertools;
use regex::Regex;
use std::str::FromStr;
use std::{fmt::Debug, sync::Arc};
use versions::Versioning;
use xx::regex;

#[derive(Debug)]
pub struct PIPXBackend {
    ba: Arc<BackendArg>,
    latest_version_cache: CacheManager<Option<String>>,
}

#[async_trait]
impl Backend for PIPXBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Pipx
    }

    fn ba(&self) -> &Arc<BackendArg> {
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
    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        match self.tool_name().parse()? {
            PipxRequest::Pypi(package) => {
                let registry_url = self.get_registry_url()?;
                if registry_url.contains("/json") {
                    debug!("Fetching JSON for {}", package);
                    let url = format!("https://pypi.org/pypi/{package}/json");
                    let data: PypiPackage = HTTP_FETCH.json(url).await?;
                    let versions = data
                        .releases
                        .keys()
                        .map(|v| v.to_string())
                        .sorted_by_cached_key(|v| Versioning::new(v))
                        .collect();

                    Ok(versions)
                } else {
                    debug!("Fetching HTML for {}", package);
                    let url = format!("https://pypi.org/simple/{package}/");
                    let html = HTTP_FETCH.get_html(url).await?;

                    let version_re = regex!(r#"href=["'].*?/([^/]+)\.tar\.gz["']"#);
                    let versions: Vec<String> = version_re
                        .captures_iter(&html)
                        .filter_map(|cap| {
                            let filename = cap.get(1)?.as_str();
                            let escaped_package = regex::escape(&package);
                            let re_str = format!("^{escaped_package}-(.+)$");
                            let pkg_re = regex::Regex::new(&re_str).ok()?;
                            let pkg_version = pkg_re.captures(filename)?.get(1)?.as_str();
                            Some(pkg_version.to_string())
                        })
                        .sorted_by_cached_key(|v| Versioning::new(v))
                        .collect();

                    Ok(versions)
                }
            }
            PipxRequest::Git(url) if url.starts_with("https://github.com/") => {
                let repo = url.strip_prefix("https://github.com/").unwrap();
                let data = github::list_releases(repo).await?;
                Ok(data.into_iter().rev().map(|r| r.tag_name).collect())
            }
            PipxRequest::Git { .. } => Ok(vec!["latest".to_string()]),
        }
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        let this = self;
        timeout::run_with_timeout_async(
            async || {
                this.latest_version_cache
                    .get_or_try_init_async(async || match this.tool_name().parse()? {
                        PipxRequest::Pypi(package) => {
                            let registry_url = this.get_registry_url()?;
                            if registry_url.contains("/json") {
                                debug!("Fetching JSON for {}", package);
                                let url = format!("https://pypi.org/pypi/{package}/json");
                                let pkg: PypiPackage = HTTP_FETCH.json(url).await?;
                                Ok(Some(pkg.info.version))
                            } else {
                                debug!("Fetching HTML for {}", package);
                                let url = format!("https://pypi.org/simple/{package}/");
                                let html = HTTP_FETCH.get_html(url).await?;

                                let version_re = regex!(r#"href=["'].*?/([^/]+)\.tar\.gz["']"#);
                                let version = version_re
                                    .captures_iter(&html)
                                    .filter_map(|cap| {
                                        let filename = cap.get(1)?.as_str();
                                        let escaped_package = regex::escape(&package);
                                        let re_str = format!("^{escaped_package}-(.+)$");
                                        let pkg_re = regex::Regex::new(&re_str).ok()?;
                                        let pkg_version =
                                            pkg_re.captures(filename)?.get(1)?.as_str();
                                        Some(pkg_version.to_string())
                                    })
                                    .filter(|v| {
                                        !v.contains("dev")
                                            && !v.contains("a")
                                            && !v.contains("b")
                                            && !v.contains("rc")
                                    })
                                    .sorted_by_cached_key(|v| Versioning::new(v))
                                    .next_back();

                                Ok(version)
                            }
                        }
                        _ => this.latest_version(config, Some("latest".into())).await,
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let pipx_request = self
            .tool_name()
            .parse::<PipxRequest>()?
            .pipx_request(&tv.version, &tv.request.options());

        if self.uv_is_installed(&ctx.config).await
            && Settings::get().pipx.uvx != Some(false)
            && tv.request.options().get("uvx") != Some(&"false".to_string())
        {
            ctx.pr
                .set_message(format!("uv tool install {pipx_request}"));
            let mut cmd = Self::uvx_cmd(
                &ctx.config,
                &["tool", "install", &pipx_request],
                self,
                &tv,
                &ctx.ts,
                &ctx.pr,
            )
            .await?;
            if let Some(args) = tv.request.options().get("uvx_args") {
                cmd = cmd.args(shell_words::split(args)?);
            }
            cmd.execute()?;
        } else {
            ctx.pr.set_message(format!("pipx install {pipx_request}"));
            let mut cmd = Self::pipx_cmd(
                &ctx.config,
                &["install", &pipx_request],
                self,
                &tv,
                &ctx.ts,
                &ctx.pr,
            )
            .await?;
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
            .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
            .build(),
            ba: Arc::new(ba),
        }
    }

    fn get_registry_url(&self) -> eyre::Result<String> {
        let registry_url = Settings::get().pipx.registry_url.clone();

        debug!("Pipx registry URL: {}", registry_url);

        let re = Regex::new(r"^(http|https)://.*\{\}.*$").unwrap();

        if !re.is_match(&registry_url) {
            return Err(eyre!(
                "Registry URL must be a valid URL and contain a {{}} placeholder"
            ));
        }

        Ok(registry_url)
    }

    pub async fn reinstall_all(config: &Arc<Config>) -> Result<()> {
        let ts = ToolsetBuilder::new().build(config).await?;
        let pipx_tools = ts
            .list_installed_versions(config)
            .await?
            .into_iter()
            .filter(|(b, _tv)| b.ba().backend_type() == BackendType::Pipx)
            .collect_vec();
        if Settings::get().pipx.uvx != Some(false) {
            let pr = MultiProgressReport::get().add("reinstalling pipx tools with uvx");
            for (b, tv) in pipx_tools {
                for (cmd, tool) in &[
                    ("uninstall", tv.ba().tool_name.to_string()),
                    ("install", format!("{}=={}", tv.ba().tool_name, tv.version)),
                ] {
                    let args = &["tool", cmd, tool];
                    Self::uvx_cmd(config, args, &*b, &tv, &ts, &pr)
                        .await?
                        .execute()?;
                }
            }
        } else {
            let pr = MultiProgressReport::get().add("reinstalling pipx tools");
            for (b, tv) in pipx_tools {
                let args = &["reinstall", &tv.ba().tool_name];
                Self::pipx_cmd(config, args, &*b, &tv, &ts, &pr)
                    .await?
                    .execute()?;
            }
        }
        Ok(())
    }

    async fn uvx_cmd<'a>(
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a Box<dyn SingleReport>,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("uv");
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .env("UV_TOOL_DIR", tv.install_path())
            .env("UV_TOOL_BIN_DIR", tv.install_path().join("bin"))
            .envs(ts.env_with_path(config).await?)
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn pipx_cmd<'a>(
        config: &Arc<Config>,
        args: &[&str],
        b: &dyn Backend,
        tv: &ToolVersion,
        ts: &Toolset,
        pr: &'a Box<dyn SingleReport>,
    ) -> Result<CmdLineRunner<'a>> {
        let mut cmd = CmdLineRunner::new("pipx");
        for arg in args {
            cmd = cmd.arg(arg);
        }
        cmd.with_pr(pr)
            .env("PIPX_HOME", tv.install_path())
            .env("PIPX_BIN_DIR", tv.install_path().join("bin"))
            .envs(ts.env_with_path(config).await?)
            .prepend_path(ts.list_paths(config).await)?
            .prepend_path(vec![tv.install_path().join("bin")])?
            .prepend_path(b.dependency_toolset(config).await?.list_paths(config).await)
    }

    async fn uv_is_installed(&self, config: &Arc<Config>) -> bool {
        self.dependency_which(config, "uv").await.is_some()
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
            Some(extras) => format!("[{extras}]"),
            None => String::new(),
        }
    }

    fn pipx_request(&self, v: &str, opts: &ToolVersionOptions) -> String {
        let extras = self.extras_from_opts(opts);

        if v == "latest" {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git"),
                PipxRequest::Pypi(package) => format!("{package}{extras}"),
            }
        } else {
            match self {
                PipxRequest::Git(url) => format!("git+{url}.git@{v}"),
                PipxRequest::Pypi(package) => format!("{package}{extras}=={v}"),
            }
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
