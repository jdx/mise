use std::collections::BTreeMap;
use std::path::PathBuf;
use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use color_eyre::Section;
use eyre::{bail, eyre};
use serde_json::Deserializer;
use url::Url;

mod native_binstall;

use native_binstall::NativeBinstallAction;

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
use crate::errors::Error;
use crate::http::HTTP_FETCH;
use crate::install_context::InstallContext;
use crate::toolset::{ToolRequest, ToolVersion, ToolVersionOptions, Toolset};

#[derive(Debug)]
pub struct CargoBackend {
    ba: Arc<BackendArg>,
}

const CARGO_BINSTALL_NO_FALLBACK_EXIT_CODE: i32 = 94;
const CARGO_BINSTALL_DEFAULT_DISABLED_STRATEGIES: &[&str] = &["compile"];

#[derive(Debug, Clone, Copy)]
pub(super) struct CargoOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> CargoOptions<'a> {
    pub(super) fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    pub(super) fn bin(&self) -> Option<String> {
        self.values.platform_string("bin")
    }

    fn bin_for_target(&self, target: &PlatformTarget) -> Option<String> {
        self.values.platform_string_for_target("bin", target)
    }

    pub(super) fn locked(&self) -> bool {
        self.values
            .raw()
            .get_string("locked")
            .is_none_or(|v| v.to_lowercase() != "false")
    }

    fn features(&self) -> Option<String> {
        match self.values.raw().opts.get("features") {
            Some(toml::Value::Array(features)) => {
                let features = features
                    .iter()
                    .filter_map(|feature| {
                        feature.as_str().map(str::to_string).or_else(|| {
                            warn!(
                                "invalid value in cargo features array: {feature}; expected string"
                            );
                            None
                        })
                    })
                    .collect::<Vec<_>>();
                if features.is_empty() {
                    None
                } else {
                    Some(features.join(" "))
                }
            }
            _ => self.values.raw().get_string("features"),
        }
    }

    fn default_features_disabled(&self) -> bool {
        self.values
            .raw()
            .get_string("default-features")
            .is_some_and(|v| v.to_lowercase() == "false")
    }

    pub(super) fn crate_arg(&self) -> Option<String> {
        self.values.raw().get_string("crate")
    }

    fn lockfile_options(&self, target: &PlatformTarget) -> BTreeMap<String, String> {
        let mut result = BTreeMap::new();
        if let Some(features) = self.features() {
            result.insert("features".to_string(), features);
        }
        for key in ["default-features", "crate", "locked"] {
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

        let response = HTTP_FETCH
            .get_text(get_crate_url(&self.tool_name())?)
            .await?;

        parse_crate_versions(&response)
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

        let cargo_install_required_options =
            Self::cargo_install_required_options(&tv.request.options());
        if self.git_url().is_none() && cargo_install_required_options.is_empty() {
            match self.binstall_status(&config, Some(&ctx.ts)).await {
                BinstallStatus::Enabled(cargo_binstall) => {
                    let mut cmd = CmdLineRunner::new(cargo_binstall).arg("-y");
                    let disabled_strategies = CARGO_BINSTALL_DEFAULT_DISABLED_STRATEGIES
                        .iter()
                        .copied()
                        .chain(
                            (!Settings::get().cargo.binstall_quickinstall)
                                .then_some("quick-install"),
                        )
                        .collect::<Vec<_>>()
                        .join(",");
                    cmd = cmd.args(["--disable-strategies", &disabled_strategies]);
                    if let Some(token) = &*GITHUB_TOKEN {
                        cmd = cmd.env("GITHUB_TOKEN", token)
                    }
                    let result = self
                        .execute_install_command(ctx, &tv, cmd.arg(&install_arg))
                        .await;
                    if result.as_ref().is_ok() {
                        return Ok(tv.clone());
                    }
                    if Settings::get().cargo.binstall_only
                        || !result.as_ref().is_err_and(|err| {
                            Error::get_exit_status(err)
                                == Some(CARGO_BINSTALL_NO_FALLBACK_EXIT_CODE)
                        })
                    {
                        result?;
                    }
                    info!("cargo-binstall found no prebuilt binary; falling back to cargo install");
                }
                BinstallStatus::Disabled if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is disabled, but cargo.binstall_only is set");
                }
                _ if Settings::get().cargo.binstall_only => {
                    bail!("cargo-binstall is not available, but cargo.binstall_only is set");
                }
                BinstallStatus::Unavailable => match Settings::get().cargo.binstall_native {
                    Some(true) => {
                        Settings::get().ensure_experimental("cargo.binstall_native")?;
                        if self
                            .native_binstall(ctx, &tv, NativeBinstallAction::Install)
                            .await?
                        {
                            return Ok(tv.clone());
                        }
                    }
                    Some(false) => {}
                    None if native_binstall::rollout_warning_active() => {
                        self.native_binstall(ctx, &tv, NativeBinstallAction::WarnOnly)
                            .await?;
                    }
                    None => {}
                },
                _ => {}
            }
        } else if Settings::get().cargo.binstall_only
            && self.git_url().is_none()
            && !cargo_install_required_options.is_empty()
        {
            let options = format_tool_options(&cargo_install_required_options);
            bail!(
                "cargo-binstall cannot honor cargo install-only tool option(s): {options}\n\
                hint: Remove the option(s), or disable cargo.binstall_only to allow cargo install"
            );
        } else if !cargo_install_required_options.is_empty() {
            let options = format_tool_options(&cargo_install_required_options);
            info!(
                "not using cargo-binstall because cargo install-only tool option(s) are specified: {options}"
            );
        }

        let mut cmd = CmdLineRunner::new("cargo").arg("install");
        if let Some(url) = self.git_url() {
            cmd = cmd.arg(format!("--git={url}"));
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
        } else {
            cmd = cmd.arg(&install_arg);
        }
        self.execute_install_command(ctx, &tv, cmd).await?;

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

    async fn execute_install_command<'a>(
        &'a self,
        ctx: &'a InstallContext,
        tv: &'a ToolVersion,
        mut cmd: CmdLineRunner<'a>,
    ) -> Result<()> {
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
        if let Some(registry_name) = &Settings::get().cargo.registry_name {
            cmd = cmd.arg(format!("--registry={registry_name}"));
        }

        cmd.arg("--root")
            .arg(tv.install_path())
            .with_pr(ctx.pr.as_ref())
            .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
            .env_values(tv.install_env())
            .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
            .prepend_path(
                self.dependency_toolset(&ctx.config)
                    .await?
                    .list_paths(&ctx.config)
                    .await,
            )?
            .execute()
    }

    fn cargo_install_required_options(opts: &ToolVersionOptions) -> Vec<&'static str> {
        let mut options = vec![];
        if CargoOptions::new(opts)
            .features()
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

    async fn binstall_status(&self, config: &Arc<Config>, ts: Option<&Toolset>) -> BinstallStatus {
        if !Settings::get().cargo.binstall {
            return BinstallStatus::Disabled;
        }
        if let Some(cargo_binstall) = self
            .dependency_path_for_install(config, ts, "cargo-binstall")
            .await
        {
            return BinstallStatus::Enabled(cargo_binstall);
        }
        BinstallStatus::Unavailable
    }

    async fn native_binstall(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        action: NativeBinstallAction,
    ) -> Result<bool> {
        match native_binstall::install(ctx, tv, &self.tool_name(), action).await {
            Ok(true) => Ok(true),
            Ok(false) => Ok(false),
            Err(err) => {
                debug!("native cargo binary install unavailable: {err:#}");
                Ok(false)
            }
        }
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

fn get_crate_url(name: &str) -> eyre::Result<Url> {
    if name.is_empty() || !name.is_ascii() {
        bail!("invalid Cargo crate name: {name:?}");
    }
    let name = name.to_lowercase();
    let url = match name.len() {
        1 => format!("https://index.crates.io/1/{name}"),
        2 => format!("https://index.crates.io/2/{name}"),
        3 => format!("https://index.crates.io/3/{}/{name}", &name[..1]),
        _ => format!(
            "https://index.crates.io/{}/{}/{name}",
            &name[..2],
            &name[2..4]
        ),
    };
    Ok(url.parse()?)
}

fn parse_crate_versions(response: &str) -> eyre::Result<Vec<VersionInfo>> {
    let mut versions = vec![];
    for version in Deserializer::from_str(response).into_iter::<CrateVersion>() {
        let version = version?;
        if !version.yanked {
            versions.push(VersionInfo {
                version: version.vers,
                created_at: version.pubtime,
                ..Default::default()
            });
        }
    }
    Ok(versions)
}

#[derive(Debug, serde::Deserialize)]
struct CrateVersion {
    vers: String,
    yanked: bool,
    pubtime: Option<String>,
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
    fn cargo_options_accepts_array_features() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "features".into(),
            toml::Value::Array(vec![
                toml::Value::String("postgres".into()),
                toml::Value::Integer(1),
                toml::Value::String("rustls".into()),
            ]),
        );

        let target = PlatformTarget::new(Platform::parse("linux-x64").unwrap());
        let lock_opts = CargoOptions::new(&opts).lockfile_options(&target);

        assert_eq!(
            CargoOptions::new(&opts).features().as_deref(),
            Some("postgres rustls")
        );
        assert_eq!(
            lock_opts.get("features").map(String::as_str),
            Some("postgres rustls")
        );
    }

    #[test]
    fn cargo_options_defaults_to_locked() {
        let opts = ToolVersionOptions::default();

        assert!(CargoOptions::new(&opts).locked());
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
    fn cargo_install_required_options_detects_array_features() {
        let mut opts = ToolVersionOptions::default();
        opts.opts.insert(
            "features".into(),
            toml::Value::Array(vec![
                toml::Value::String("postgres".into()),
                toml::Value::String("rustls".into()),
            ]),
        );

        assert_eq!(
            CargoBackend::cargo_install_required_options(&opts),
            vec!["features"]
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

    #[test]
    fn crate_index_url_uses_sparse_index_layout() {
        let cases = [
            ("a", "https://index.crates.io/1/a"),
            ("Ab", "https://index.crates.io/2/ab"),
            ("Abc", "https://index.crates.io/3/a/abc"),
            ("Cargo-Deny", "https://index.crates.io/ca/rg/cargo-deny"),
        ];

        for (name, expected) in cases {
            assert_eq!(get_crate_url(name).unwrap().as_str(), expected);
        }
    }

    #[test]
    fn crate_index_url_rejects_empty_name() {
        let error = get_crate_url("").unwrap_err();

        assert_eq!(error.to_string(), "invalid Cargo crate name: \"\"");
    }

    #[test]
    fn crate_index_url_rejects_non_ascii_name() {
        let error = get_crate_url("aébc").unwrap_err();

        assert_eq!(error.to_string(), "invalid Cargo crate name: \"aébc\"");
    }

    #[test]
    fn parse_crate_versions_uses_pubtime_and_skips_yanked_versions() {
        let response = r#"{"vers":"1.0.0","yanked":false,"pubtime":"2025-01-01T00:00:00Z"}
{"vers":"1.1.0","yanked":true,"pubtime":"2025-02-01T00:00:00Z"}
{"vers":"1.2.0","yanked":false}"#;

        let versions = parse_crate_versions(response).unwrap();
        let versions = versions
            .iter()
            .map(|version| (version.version.as_str(), version.created_at.as_deref()))
            .collect::<Vec<_>>();

        assert_eq!(
            versions,
            vec![("1.0.0", Some("2025-01-01T00:00:00Z")), ("1.2.0", None),]
        );
    }
}
