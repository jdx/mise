use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::Backend;
use crate::build_time::TARGET;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolSource::IdiomaticVersionFile;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, github, plugins};
use async_trait::async_trait;
use eyre::Result;
use xx::regex;

#[derive(Debug)]
pub struct RustPlugin {
    ba: Arc<BackendArg>,
}

impl RustPlugin {
    pub fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("rust").into(),
        }
    }

    async fn setup_rustup(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let settings = Settings::get();
        if rustup_home().join("settings.toml").exists() && cargo_bin().exists() {
            return Ok(());
        }
        ctx.pr.set_message("Downloading rustup-init".into());
        HTTP.download_file(rustup_url(&settings), &rustup_path(), Some(&ctx.pr))
            .await?;
        file::make_executable(rustup_path())?;
        file::create_dir_all(rustup_home())?;
        let ts = ctx.config.get_toolset().await?;
        let cmd = CmdLineRunner::new(rustup_path())
            .with_pr(&ctx.pr)
            .arg("--no-modify-path")
            .arg("--default-toolchain")
            .arg("none")
            .arg("-y")
            .envs(self.exec_env(&ctx.config, ts, tv).await?);
        cmd.execute()?;
        Ok(())
    }

    async fn test_rust(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message(format!("{RUSTC_BIN} -V"));
        let ts = ctx.config.get_toolset().await?;
        CmdLineRunner::new(RUSTC_BIN)
            .with_pr(&ctx.pr)
            .arg("-V")
            .envs(self.exec_env(&ctx.config, ts, tv).await?)
            .prepend_path(self.list_bin_paths(&ctx.config, tv).await?)?
            .execute()
    }

    fn target_triple(&self, tv: &ToolVersion) -> String {
        format!("{}-{}", tv.version, TARGET)
    }
}

#[async_trait]
impl Backend for RustPlugin {
    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<String>> {
        let versions = github::list_releases("rust-lang/rust")
            .await?
            .into_iter()
            .map(|r| r.tag_name)
            .rev()
            .chain(vec!["nightly".into(), "beta".into(), "stable".into()])
            .collect();
        Ok(versions)
    }

    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        if Settings::get().experimental {
            Ok(vec!["rust-toolchain.toml".into()])
        } else {
            Ok(vec![])
        }
    }

    fn parse_idiomatic_file(&self, path: &Path) -> Result<String> {
        let rt = parse_idiomatic_file(path)?;
        Ok(rt.channel)
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        self.setup_rustup(ctx, &tv).await?;
        let ts = ctx.config.get_toolset().await?;

        let (profile, components, targets) = get_args(&tv);

        CmdLineRunner::new(RUSTUP_BIN)
            .with_pr(&ctx.pr)
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .opt_arg(profile.as_ref().map(|_| "--profile"))
            .opt_arg(profile)
            .opt_args("--component", components)
            .opt_args("--target", targets)
            .prepend_path(self.list_bin_paths(&ctx.config, &tv).await?)?
            .envs(self.exec_env(&ctx.config, ts, &tv).await?)
            .execute()?;

        file::remove_all(tv.install_path())?;
        file::make_symlink(&cargo_home().join("bin"), &tv.install_path())?;

        self.test_rust(ctx, &tv).await?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        config: &Arc<Config>,
        pr: &Box<dyn SingleReport>,
        tv: &ToolVersion,
    ) -> Result<()> {
        let ts = config.get_toolset().await?;
        let mut env = self.exec_env(config, ts, tv).await?;
        env.remove("RUSTUP_TOOLCHAIN");
        CmdLineRunner::new(RUSTUP_BIN)
            .with_pr(pr)
            .arg("toolchain")
            .arg("uninstall")
            .arg(&tv.version)
            .prepend_path(self.list_bin_paths(config, tv).await?)?
            .envs(env)
            .execute()
    }

    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        _tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        Ok(vec![cargo_bindir()])
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        let toolchain = tv.version.to_string();
        Ok([
            (
                "CARGO_HOME".to_string(),
                cargo_home().to_string_lossy().to_string(),
            ),
            (
                "RUSTUP_HOME".to_string(),
                rustup_home().to_string_lossy().to_string(),
            ),
            ("RUSTUP_TOOLCHAIN".to_string(), toolchain),
        ]
        .into())
    }

    async fn outdated_info(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        bump: bool,
    ) -> Result<Option<OutdatedInfo>> {
        let v_re = regex!(r#"Update available : (.*) -> (.*)"#);
        if regex!(r"(\d+)\.(\d+)\.(\d+)").is_match(&tv.version) {
            let oi = OutdatedInfo::resolve(config, tv.clone(), bump).await?;
            Ok(oi)
        } else {
            let ts = config.get_toolset().await?;
            let mut cmd =
                cmd!(RUSTUP_BIN, "check").env("PATH", self.path_env_for_cmd(config, tv).await?);
            for (k, v) in self.exec_env(config, ts, tv).await? {
                cmd = cmd.env(k, v);
            }
            let out = cmd.read()?;
            for line in out.lines() {
                if line.starts_with(&self.target_triple(tv)) {
                    if let Some(_cap) = v_re.captures(line) {
                        // let requested = cap.get(1).unwrap().as_str().to_string();
                        // let latest = cap.get(2).unwrap().as_str().to_string();
                        let oi = OutdatedInfo::new(config, tv.clone(), tv.version.clone())?;
                        return Ok(Some(oi));
                    }
                }
            }
            Ok(None)
        }
    }
}

#[derive(Debug, Default)]
struct RustToolchain {
    channel: String,
    profile: Option<String>,
    components: Option<Vec<String>>,
    targets: Option<Vec<String>>,
}

fn get_args(tv: &ToolVersion) -> (Option<String>, Option<Vec<String>>, Option<Vec<String>>) {
    let rt = if tv.request.source().is_idiomatic_version_file() {
        match tv.request.source() {
            IdiomaticVersionFile(path) => parse_idiomatic_file(path).ok(),
            _ => None,
        }
    } else {
        None
    };

    let get_tooloption = |name: &str| {
        tv.request
            .options()
            .get(name)
            .map(|c| c.split(',').map(|s| s.to_string()).collect())
    };
    let profile = rt
        .as_ref()
        .and_then(|rt| rt.profile.clone())
        .or_else(|| tv.request.options().get("profile").cloned());
    let components = rt
        .as_ref()
        .and_then(|rt| rt.components.clone())
        .or_else(|| get_tooloption("components"));
    let targets = rt
        .as_ref()
        .and_then(|rt| rt.targets.clone())
        .or_else(|| get_tooloption("targets"));

    (profile, components, targets)
}

fn parse_idiomatic_file(path: &Path) -> Result<RustToolchain> {
    let toml = file::read_to_string(path)?;
    let toml = toml.parse::<toml::Value>()?;
    let mut rt = RustToolchain::default();
    if let Some(toolchain) = toml.get("toolchain") {
        if let Some(channel) = toolchain.get("channel") {
            rt.channel = channel.as_str().unwrap().to_string();
        }
        if let Some(profile) = toolchain.get("profile") {
            rt.profile = Some(profile.as_str().unwrap().to_string());
        }
        if let Some(components) = toolchain.get("components") {
            let components = components
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            if !components.is_empty() {
                rt.components = Some(components);
            }
        }
        if let Some(targets) = toolchain.get("targets") {
            let targets = targets
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            if !targets.is_empty() {
                rt.targets = Some(targets);
            }
        }
    }
    Ok(rt)
}

#[cfg(unix)]
const RUSTC_BIN: &str = "rustc";

#[cfg(windows)]
const RUSTC_BIN: &str = "rustc.exe";

#[cfg(unix)]
const RUSTUP_INIT_BIN: &str = "rustup-init";

#[cfg(windows)]
const RUSTUP_INIT_BIN: &str = "rustup-init.exe";

#[cfg(unix)]
const RUSTUP_BIN: &str = "rustup";

#[cfg(windows)]
const RUSTUP_BIN: &str = "rustup.exe";

#[cfg(unix)]
const CARGO_BIN: &str = "cargo";

#[cfg(windows)]
const CARGO_BIN: &str = "cargo.exe";

#[cfg(unix)]
fn rustup_url(_settings: &Settings) -> String {
    "https://sh.rustup.rs".to_string()
}

#[cfg(windows)]
fn rustup_url(settings: &Settings) -> String {
    let arch = settings.arch();
    format!("https://win.rustup.rs/{arch}")
}

fn rustup_path() -> PathBuf {
    dirs::CACHE.join("rust").join(RUSTUP_INIT_BIN)
}

fn rustup_home() -> PathBuf {
    Settings::get()
        .rust
        .rustup_home
        .clone()
        .or(env::var_path("RUSTUP_HOME"))
        .unwrap_or(dirs::HOME.join(".rustup"))
}

fn cargo_home() -> PathBuf {
    Settings::get()
        .rust
        .cargo_home
        .clone()
        .or(env::var_path("CARGO_HOME"))
        .unwrap_or(dirs::HOME.join(".cargo"))
}

fn cargo_bin() -> PathBuf {
    cargo_bindir().join(CARGO_BIN)
}
fn cargo_bindir() -> PathBuf {
    cargo_home().join("bin")
}
