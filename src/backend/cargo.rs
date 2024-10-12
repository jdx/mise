use std::fmt::Debug;

use color_eyre::Section;
use eyre::eyre;
use serde_json::Deserializer;
use url::Url;

use crate::backend::{Backend, BackendType};
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::settings::SETTINGS;
use crate::config::{Config, Settings};
use crate::env::GITHUB_TOKEN;
use crate::file;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion};

#[derive(Debug)]
pub struct CargoBackend {
    ba: BackendArg,
    remote_version_cache: CacheManager<Vec<String>>,
}

impl Backend for CargoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Cargo
    }

    fn fa(&self) -> &BackendArg {
        &self.ba
    }

    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec!["cargo".into(), "rust".into()])
    }

    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        if self.git_url().is_some() {
            // TODO: maybe fetch tags/branches from git?
            return Ok(vec!["HEAD".into()]);
        }
        self.remote_version_cache
            .get_or_try_init(|| {
                let raw = HTTP_FETCH.get_text(get_crate_url(self.name())?)?;
                let stream = Deserializer::from_str(&raw).into_iter::<CrateVersion>();
                let mut versions = vec![];
                for v in stream {
                    let v = v?;
                    if !v.yanked {
                        versions.push(v.vers);
                    }
                }
                Ok(versions)
            })
            .cloned()
    }

    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()> {
        let config = Config::try_get()?;
        let install_arg = format!("{}@{}", self.name(), ctx.tv.version);

        let cmd = CmdLineRunner::new("cargo").arg("install");
        let mut cmd = if let Some(url) = self.git_url() {
            let mut cmd = cmd.arg(format!("--git={url}"));
            if let Some(rev) = ctx.tv.version.strip_prefix("rev:") {
                cmd = cmd.arg(format!("--rev={rev}"));
            } else if let Some(branch) = ctx.tv.version.strip_prefix("branch:") {
                cmd = cmd.arg(format!("--branch={branch}"));
            } else if let Some(tag) = ctx.tv.version.strip_prefix("tag:") {
                cmd = cmd.arg(format!("--tag={tag}"));
            } else if ctx.tv.version != "HEAD" {
                Err(eyre!("Invalid cargo git version: {}", ctx.tv.version).note(
                    r#"You can specify "rev:", "branch:", or "tag:", e.g.:
      * mise use cargo:eza-community/eza@tag:v0.18.0
      * mise use cargo:eza-community/eza@branch:main"#,
                ))?;
            }
            cmd
        } else if self.is_binstall_enabled(&ctx.tv) {
            let mut cmd = CmdLineRunner::new("cargo-binstall").arg("-y");
            if let Some(token) = &*GITHUB_TOKEN {
                cmd = cmd.env("GITHUB_TOKEN", token)
            }
            cmd.arg(install_arg)
        } else {
            cmd.arg(install_arg)
        };

        let opts = ctx.tv.request.options();
        if let Some(features) = opts.get("features") {
            cmd = cmd.arg(format!("--features={}", features));
        }
        if let Some(default_features) = opts.get("default-features") {
            if default_features.to_lowercase() == "false" {
                cmd = cmd.arg("--no-default-features");
            }
        }

        cmd.arg("--locked")
            .arg("--root")
            .arg(ctx.tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path(&config)?)
            .prepend_path(ctx.ts.list_paths())?
            .prepend_path(self.dependency_toolset()?.list_paths())?
            .execute()?;

        Ok(())
    }
}

impl CargoBackend {
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

    fn is_binstall_enabled(&self, tv: &ToolVersion) -> bool {
        let settings = Settings::get();
        if !settings.cargo_binstall || file::which_non_pristine("cargo-binstall").is_none() {
            return false;
        }
        let opts = tv.request.options();
        if opts.contains_key("features") || opts.contains_key("default-features") {
            info!("not using cargo-binstall because features are specified");
            return false;
        }
        true
    }

    /// if the name is a git repo, return the git url
    fn git_url(&self) -> Option<Url> {
        if let Ok(url) = Url::parse(self.name()) {
            Some(url)
        } else if let Some((user, repo)) = self.name().split_once('/') {
            format!("https://github.com/{user}/{repo}.git").parse().ok()
        } else {
            None
        }
    }
}

fn get_crate_url(n: &str) -> eyre::Result<Url> {
    let n = n.to_lowercase();
    let url = match n.len() {
        1 => format!("https://index.crates.io/1/{n}"),
        2 => format!("https://index.crates.io/2/{n}"),
        3 => format!("https://index.crates.io/3/{}/{n}", &n[..1]),
        _ => format!("https://index.crates.io/{}/{}/{n}", &n[..2], &n[2..4]),
    };
    Ok(url.parse()?)
}

#[derive(Debug, serde::Deserialize)]
struct CrateVersion {
    //name: String,
    vers: String,
    yanked: bool,
}
