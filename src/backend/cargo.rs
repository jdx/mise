use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use color_eyre::Section;
use eyre::{bail, eyre};
use serde_json::Deserializer;
use url::Url;

use crate::Result;
use crate::backend::Backend;
use crate::backend::backend_type::BackendType;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::GITHUB_TOKEN;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::ToolVersion;
use crate::{env, file};

#[derive(Debug)]
pub struct CargoBackend {
    ba: Arc<BackendArg>,
}

#[async_trait]
impl Backend for CargoBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Cargo
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["rust"])
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["cargo-binstall", "sccache"])
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        if self.git_url().is_some() {
            // TODO: maybe fetch tags/branches from git?
            return Ok(vec!["HEAD".into()]);
        }
        let raw = HTTP_FETCH
            .get_text(get_crate_url(&self.tool_name())?)
            .await?;
        let stream = Deserializer::from_str(&raw).into_iter::<CrateVersion>();
        let mut versions = vec![];
        for v in stream {
            let v = v?;
            if !v.yanked {
                versions.push(v.vers);
            }
        }
        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let config = ctx.config.clone();
        let install_arg = format!("{}@{}", self.tool_name(), tv.version);
        let registry_name = &Settings::get().cargo.registry_name;

        let cmd = CmdLineRunner::new("cargo").arg("install");
        let mut cmd = if let Some(url) = self.git_url() {
            let mut cmd = cmd.arg(format!("--git={url}"));
            if let Some(rev) = tv.version.strip_prefix("rev:") {
                cmd = cmd.arg(format!("--rev={rev}"));
            } else if let Some(branch) = tv.version.strip_prefix("branch:") {
                cmd = cmd.arg(format!("--branch={branch}"));
            } else if let Some(tag) = tv.version.strip_prefix("tag:") {
                cmd = cmd.arg(format!("--tag={tag}"));
            } else if tv.version != "HEAD" {
                Err(eyre!("Invalid cargo git version: {}", tv.version).note(
                    r#"You can specify "rev:", "branch:", or "tag:", e.g.:
      * mise use cargo:eza-community/eza@tag:v0.18.0
      * mise use cargo:eza-community/eza@branch:main"#,
                ))?;
            }
            cmd
        } else if self.is_binstall_enabled(&config, &tv).await {
            let mut cmd = CmdLineRunner::new("cargo-binstall").arg("-y");
            if let Some(token) = &*GITHUB_TOKEN {
                cmd = cmd.env("GITHUB_TOKEN", token)
            }
            cmd.arg(install_arg)
        } else if env::var("MISE_CARGO_BINSTALL_ONLY").is_ok_and(|v| v == "1") {
            bail!("cargo-binstall is not available, but MISE_CARGO_BINSTALL_ONLY is set");
        } else {
            cmd.arg(install_arg)
        };

        let opts = tv.request.options();
        if let Some(bin) = opts.get("bin") {
            cmd = cmd.arg(format!("--bin={bin}"));
        }
        if opts
            .get("locked")
            .is_none_or(|v| v.to_lowercase() != "false")
        {
            cmd = cmd.arg("--locked");
        }
        if let Some(features) = opts.get("features") {
            cmd = cmd.arg(format!("--features={features}"));
        }
        if let Some(default_features) = opts.get("default-features") {
            if default_features.to_lowercase() == "false" {
                cmd = cmd.arg("--no-default-features");
            }
        }
        if let Some(c) = opts.get("crate") {
            cmd = cmd.arg(c);
        }
        if let Some(registry_name) = registry_name {
            cmd = cmd.arg(format!("--registry={registry_name}"));
        }

        cmd.arg("--root")
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

        Ok(tv.clone())
    }
}

impl CargoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    async fn is_binstall_enabled(&self, config: &Arc<Config>, tv: &ToolVersion) -> bool {
        if !Settings::get().cargo.binstall {
            return false;
        }
        if file::which_non_pristine("cargo-binstall").is_none() {
            match self.dependency_toolset(config).await {
                Ok(ts) => {
                    if ts.which(config, "cargo-binstall").await.is_none() {
                        return false;
                    }
                }
                Err(_e) => {
                    return false;
                }
            }
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
        if let Ok(url) = Url::parse(&self.tool_name()) {
            Some(url)
        } else if let Some((user, repo)) = self.tool_name().split_once('/') {
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
