use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use color_eyre::Section;
use eyre::{bail, eyre};
use url::Url;

use crate::Result;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::options::BackendOptions;
use crate::backend::platform_target::PlatformTarget;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::env::GITHUB_TOKEN;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};

#[derive(Debug)]
pub struct CargoBackend {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct CargoOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> CargoOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn bin(&self) -> Option<String> {
        self.values.platform_string("bin")
    }

    fn bin_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("bin", target)
    }

    fn locked(&self) -> bool {
        self.values
            .raw()
            .get_string("locked")
            .is_none_or(|v| v.to_lowercase() != "false")
    }

    fn features(&self) -> Option<String> {
        self.values.raw().get_string("features")
    }

    fn default_features_disabled(&self) -> bool {
        self.values
            .raw()
            .get_string("default-features")
            .is_some_and(|v| v.to_lowercase() == "false")
    }

    fn crate_arg(&self) -> Option<String> {
        self.values.raw().get_string("crate")
    }

    fn lockfile_options(&self, target: &PlatformTarget) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        for key in ["features", "default-features", "crate", "locked"] {
            if let Some(value) = self.values.raw().get_string(key) {
                result.insert(key.to_string(), value.to_string());
            }
        }
        if let Some(bin) = self.bin_for_target(target) {
            result.insert("bin".to_string(), bin);
        }
        result
    }
}

#[derive(Debug)]
enum BinstallStatus {
    Enabled(PathBuf),
    Disabled,
    Unavailable,
    UnsupportedOptions(Vec<&'static str>),
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

    /// Cargo installs packages from crates.io using version specs (e.g., ripgrep@14.0.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        if self.git_url().is_some() {
            // TODO: maybe fetch tags/branches from git?
            return Ok(vec![VersionInfo {
                version: "HEAD".into(),
                ..Default::default()
            }]);
        }

        // Use crates.io API which includes created_at timestamps
        let url = format!(
            "https://crates.io/api/v1/crates/{}/versions",
            self.tool_name()
        );
        let response: CratesIoVersionsResponse = HTTP_FETCH.json(&url).await?;

        let versions = response
            .versions
            .into_iter()
            .filter(|v| !v.yanked)
            .map(|v| VersionInfo {
                version: v.num,
                created_at: Some(v.created_at),
                ..Default::default()
            })
            .rev() // API returns newest first, we want oldest first
            .collect();

        Ok(versions)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        // Check if cargo is available
        self.warn_if_dependency_missing(
            &ctx.config,
            "cargo",
            &["rust", "cargo"],
            "To use cargo packages with mise, you need to install Rust first:\n\
              mise use rust@latest\n\n\
            Or install Rust via https://rustup.rs/",
        )
        .await;

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
        } else {
            match self.binstall_status(&config, Some(&ctx.ts), &tv).await {
                BinstallStatus::Enabled(cargo_binstall) => {
                    let mut cmd = CmdLineRunner::new(cargo_binstall).arg("-y");
                    if let Some(token) = &*GITHUB_TOKEN {
                        cmd = cmd.env("GITHUB_TOKEN", token)
                    }
                    cmd.arg(install_arg)
                }
                BinstallStatus::UnsupportedOptions(options)
                    if Settings::get().cargo.binstall_only =>
                {
                    let options = format_tool_options(&options);
                    bail!(
                        "cargo-binstall cannot honor cargo install-only tool option(s): {options}\n\
                        hint: Remove the option(s), or disable cargo.binstall_only to allow cargo install"
                    );
                }
                BinstallStatus::Disabled if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is disabled, but cargo.binstall_only is set");
                }
                _ if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is not available, but cargo.binstall_only is set");
                }
                BinstallStatus::UnsupportedOptions(options) => {
                    let options = format_tool_options(&options);
                    info!(
                        "not using cargo-binstall because cargo install-only tool option(s) are specified: {options}"
                    );
                    cmd.arg(install_arg)
                }
                _ => cmd.arg(install_arg),
            }
        };

        let request_options = tv.request.options();
        let opts = CargoOptions::new(&request_options);
        if let Some(bin) = opts.bin() {
            cmd = cmd.arg(format!("--bin={bin}"));
        }
        if opts.locked() {
            cmd = cmd.arg("--locked");
        }
        if let Some(features) = opts.features() {
            cmd = cmd.arg(format!("--features={features}"));
        }
        if opts.default_features_disabled() {
            cmd = cmd.arg("--no-default-features");
        }
        if let Some(c) = opts.crate_arg() {
            cmd = cmd.arg(c);
        }
        if let Some(registry_name) = registry_name {
            cmd = cmd.arg(format!("--registry={registry_name}"));
        }

        cmd.arg("--root")
            .arg(tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
            .envs(tv.install_env())
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

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let opts = request.options();
        Ok(CargoOptions::new(&opts).lockfile_options(target))
    }
}

/// Returns install-time-only option keys for Cargo backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "features".into(),
        "default-features".into(),
        "bin".into(),
        "crate".into(),
        "locked".into(),
    ]
}

impl CargoBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self { ba: Arc::new(ba) }
    }

    fn cargo_install_required_options(opts: &ToolVersionOptions) -> Vec<&'static str> {
        let mut options = vec![];
        if opts
            .get_string("features")
            .is_some_and(|features| !features.trim().is_empty())
        {
            options.push("features");
        }
        if opts
            .get_string("default-features")
            .is_some_and(|default_features| default_features.to_lowercase() == "false")
        {
            options.push("default-features");
        }
        options
    }

    async fn binstall_status(
        &self,
        config: &Arc<Config>,
        ts: Option<&Toolset>,
        tv: &ToolVersion,
    ) -> BinstallStatus {
        if !Settings::get().cargo.binstall {
            return BinstallStatus::Disabled;
        }
        let opts = tv.request.options();
        let cargo_install_required_options = Self::cargo_install_required_options(&opts);
        if !cargo_install_required_options.is_empty() {
            return BinstallStatus::UnsupportedOptions(cargo_install_required_options);
        }
        if let Some(cargo_binstall) = self
            .dependency_path_for_install(config, ts, "cargo-binstall")
            .await
        {
            return BinstallStatus::Enabled(cargo_binstall);
        }
        BinstallStatus::Unavailable
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

fn format_tool_options(options: &[&'static str]) -> String {
    options
        .iter()
        .map(|option| format!("`{option}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoVersionsResponse {
    versions: Vec<CratesIoVersion>,
}

#[derive(Debug, serde::Deserialize)]
struct CratesIoVersion {
    num: String,
    yanked: bool,
    created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::toolset::parse_tool_options;

    #[test]
    fn test_lockfile_options_uses_target_platform_bin() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("bin".into(), toml::Value::String("base-bin".into()));
        let mut platforms = toml::Table::new();
        let mut linux = toml::Table::new();
        linux.insert("bin".into(), toml::Value::String("linux-bin".into()));
        platforms.insert("linux-x64".into(), toml::Value::Table(linux));
        opts.opts
            .insert("platforms".into(), toml::Value::Table(platforms));

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let lock_opts = CargoOptions::new(&opts).lockfile_options(&target);

        assert_eq!(lock_opts.get("bin").map(String::as_str), Some("linux-bin"));
    }

    #[test]
    fn test_lockfile_options_include_crate_and_locked() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("crate".into(), toml::Value::String("demo".into()));
        opts.opts
            .insert("locked".into(), toml::Value::Boolean(false));

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let lock_opts = CargoOptions::new(&opts).lockfile_options(&target);

        assert_eq!(lock_opts.get("crate").map(String::as_str), Some("demo"));
        assert_eq!(lock_opts.get("locked").map(String::as_str), Some("false"));
    }

    #[test]
    fn cargo_install_required_options_skips_feature_options() {
        let opts = parse_tool_options("features=add,default-features=false");

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            vec!["features", "default-features"]
        );
    }

    #[test]
    fn cargo_install_required_options_allows_binstall_supported_options() {
        let opts =
            parse_tool_options("bin=cargo-add,crate=cargo-edit,locked=false,default-features=true");

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn cargo_install_required_options_skips_toml_bool_default_features() {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert("default-features".into(), toml::Value::Boolean(false));

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            vec!["default-features"]
        );
    }
}
