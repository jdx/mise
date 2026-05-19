use std::path::{Path, PathBuf};
use std::{collections::BTreeMap, sync::Arc};

use crate::backend::VersionInfo;
use crate::backend::options::BackendOptions;
use crate::backend::{Backend, platform_target::PlatformTarget};
use crate::build_time::TARGET;
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::toolset::ToolSource::IdiomaticVersionFile;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ResolveOptions, ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, github, plugins};
use async_trait::async_trait;
use eyre::Result;
use xx::regex;

#[derive(Debug)]
pub struct RustPlugin {
    ba: Arc<BackendArg>,
}

#[derive(Debug, Clone, Copy)]
struct RustOptions<'a> {
    values: BackendOptions<'a>,
}

impl<'a> RustOptions<'a> {
    fn new(raw: &'a ToolVersionOptions) -> Self {
        Self {
            values: BackendOptions::new(raw),
        }
    }

    fn profile(&self) -> Option<&'a str> {
        self.values.str("profile")
    }

    fn comma_list(&self, name: &str) -> Option<Vec<String>> {
        self.values
            .str(name)
            .map(|c| c.split(',').map(|s| s.trim().to_string()).collect())
    }

    fn install_args(
        &self,
        rt: Option<&RustToolchain>,
    ) -> (Option<String>, Option<Vec<String>>, Option<Vec<String>>) {
        let profile = rt
            .and_then(|rt| rt.profile.clone())
            .or_else(|| self.profile().map(str::to_string));
        let components = rt
            .and_then(|rt| rt.components.clone())
            .or_else(|| self.comma_list("components"));
        let targets = rt
            .and_then(|rt| rt.targets.clone())
            .or_else(|| self.comma_list("targets"));

        (profile, components, targets)
    }

    fn lockfile_options(&self, rt: Option<&RustToolchain>) -> BTreeMap<String, String> {
        let (profile, components, targets) = self.install_args(rt);
        let mut opts = BTreeMap::new();

        if let Some(profile) = profile {
            opts.insert("profile".into(), profile);
        }
        if let Some(components) = components
            && !components.is_empty()
        {
            opts.insert("components".into(), components.join(","));
        }
        if let Some(targets) = targets
            && !targets.is_empty()
        {
            opts.insert("targets".into(), targets.join(","));
        }

        opts
    }
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
        HTTP.download_file(rustup_url(&settings), &rustup_path(), Some(ctx.pr.as_ref()))
            .await?;
        file::make_executable(rustup_path())?;
        file::create_dir_all(rustup_home())?;
        let ts = ctx.config.get_toolset().await?;
        let mut cmd = CmdLineRunner::new(rustup_path())
            .with_pr(ctx.pr.as_ref())
            .arg("--no-modify-path")
            .arg("--default-toolchain")
            .arg("none")
            .arg("-y")
            .envs(tv.install_env())
            .envs(self.exec_env(&ctx.config, ts, tv).await?);
        if let Some(host) = settings.rust.default_host.as_ref() {
            cmd = cmd.arg("--default-host").arg(host);
        }
        cmd.execute()?;
        Ok(())
    }

    async fn test_rust(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        ctx.pr.set_message(format!("{RUSTC_BIN} -V"));
        let ts = ctx.config.get_toolset().await?;
        CmdLineRunner::new(RUSTC_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("-V")
            .envs(tv.install_env())
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

    /// Rust uses rustup for installation, which handles its own downloads.
    /// Lockfile URLs are not applicable since we don't download artifacts directly.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let rt = if request.source().is_idiomatic_version_file() {
            match request.source() {
                IdiomaticVersionFile(path) => parse_idiomatic_file(path).ok(),
                _ => None,
            }
        } else {
            None
        };

        let raw_opts = request.options();
        RustOptions::new(&raw_opts).lockfile_options(rt.as_ref())
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions: Vec<VersionInfo> = github::list_releases("rust-lang/rust")
            .await?
            .into_iter()
            .map(|r| VersionInfo {
                release_url: Some(format!("https://releases.rs/docs/{}/", r.tag_name)),
                version: r.tag_name,
                created_at: Some(r.created_at),
                ..Default::default()
            })
            .rev()
            .chain(vec![
                // Special channels - these are rolling releases that should always be updated
                VersionInfo {
                    version: "nightly".into(),
                    rolling: true,
                    ..Default::default()
                },
                VersionInfo {
                    version: "beta".into(),
                    rolling: true,
                    ..Default::default()
                },
                VersionInfo {
                    version: "stable".into(),
                    rolling: true,
                    ..Default::default()
                },
            ])
            .collect();
        Ok(versions)
    }

    async fn _idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(vec!["rust-toolchain.toml".into()])
    }

    async fn _parse_idiomatic_file(&self, path: &Path) -> Result<Vec<String>> {
        let rt = parse_idiomatic_file(path)?;
        if rt.channel.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![rt.channel])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        self.setup_rustup(ctx, &tv).await?;
        let ts = ctx.config.get_toolset().await?;

        let (profile, components, targets) = get_args(&tv);

        let mut cmd = CmdLineRunner::new(RUSTUP_BIN)
            .with_pr(ctx.pr.as_ref())
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .opt_args("--component", components)
            .opt_args("--target", targets)
            .prepend_path(self.list_bin_paths(&ctx.config, &tv).await?)?
            .envs(tv.install_env())
            .envs(self.exec_env(&ctx.config, ts, &tv).await?);
        if let Some(profile) = profile.as_ref() {
            cmd = cmd.arg("--profile").arg(profile);
        }
        cmd.execute()?;

        file::remove_all(tv.install_path())?;
        file::make_symlink(&cargo_home().join("bin"), &tv.install_path())?;

        self.test_rust(ctx, &tv).await?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        config: &Arc<Config>,
        pr: &dyn SingleReport,
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
        opts: &ResolveOptions,
    ) -> Result<Option<OutdatedInfo>> {
        let v_re = regex!(r#"Update available : (.*) -> (.*)"#);
        if regex!(r"(\d+)\.(\d+)\.(\d+)").is_match(&tv.version) {
            let oi = OutdatedInfo::resolve(config, tv.clone(), bump, opts).await?;
            Ok(oi)
        } else {
            let ts = config.get_toolset().await?;
            let mut cmd =
                cmd!(RUSTUP_BIN, "check").env("PATH", self.path_env_for_cmd(config, tv).await?);
            for (k, v) in self.exec_env(config, ts, tv).await? {
                cmd = cmd.env(k, v);
            }
            // rustup check returns exit code 100 when updates are available
            // This is not an error, so we use unchecked() and check status manually
            let result = cmd.stdout_capture().stderr_capture().unchecked().run()?;
            let exit_code = result.status.code().unwrap_or(-1);
            if exit_code != 0 && exit_code != 100 {
                let stderr = String::from_utf8_lossy(&result.stderr);
                eyre::bail!(
                    "command [\"rustup\", \"check\"] exited with code {}. stderr: {}",
                    exit_code,
                    stderr.trim()
                );
            }
            let out = String::from_utf8_lossy(&result.stdout);
            for line in out.lines() {
                if line.starts_with(&self.target_triple(tv))
                    && let Some(_cap) = v_re.captures(line)
                {
                    // let requested = cap.get(1).unwrap().as_str().to_string();
                    // let latest = cap.get(2).unwrap().as_str().to_string();
                    let oi = OutdatedInfo::new(config, tv.clone(), tv.version.clone())?;
                    return Ok(Some(oi));
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

    let raw_opts = tv.request.options();
    RustOptions::new(&raw_opts).install_args(rt.as_ref())
}

fn parse_idiomatic_file(path: &Path) -> Result<RustToolchain> {
    let content = file::read_to_string(path)?;
    let toml: toml::Value = toml::de::from_str(&content)?;
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
    let arch = match settings.arch() {
        "x64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    };
    format!("https://win.rustup.rs/{arch}")
}

fn rustup_path() -> PathBuf {
    dirs::CACHE.join("rust").join(RUSTUP_INIT_BIN)
}

fn rustup_home() -> PathBuf {
    let path = Settings::get()
        .rust
        .rustup_home
        .clone()
        .or(env::var_path("RUSTUP_HOME"))
        .unwrap_or(dirs::HOME.join(".rustup"));
    if path.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    } else {
        path
    }
}

fn cargo_home() -> PathBuf {
    let path = Settings::get()
        .rust
        .cargo_home
        .clone()
        .or(env::var_path("CARGO_HOME"))
        .unwrap_or(dirs::HOME.join(".cargo"));
    if path.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    } else {
        path
    }
}

fn cargo_bin() -> PathBuf {
    cargo_bindir().join(CARGO_BIN)
}
fn cargo_bindir() -> PathBuf {
    cargo_home().join("bin")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts_with(key: &str, value: &str) -> ToolVersionOptions {
        let mut opts = ToolVersionOptions::default();
        opts.opts
            .insert(key.to_string(), toml::Value::String(value.to_string()));
        opts
    }

    #[test]
    fn rust_options_reads_install_args() {
        let mut opts = opts_with("profile", "minimal");
        opts.opts.insert(
            "components".to_string(),
            toml::Value::String("clippy, rustfmt".to_string()),
        );
        opts.opts.insert(
            "targets".to_string(),
            toml::Value::String("wasm32-wasip1".to_string()),
        );

        let (profile, components, targets) = RustOptions::new(&opts).install_args(None);

        assert_eq!(profile, Some("minimal".to_string()));
        assert_eq!(
            components,
            Some(vec!["clippy".to_string(), "rustfmt".to_string()])
        );
        assert_eq!(targets, Some(vec!["wasm32-wasip1".to_string()]));
    }

    #[test]
    fn rust_idiomatic_options_override_tool_options() {
        let opts = opts_with("profile", "minimal");
        let rt = RustToolchain {
            profile: Some("default".to_string()),
            ..Default::default()
        };

        let (profile, _, _) = RustOptions::new(&opts).install_args(Some(&rt));

        assert_eq!(profile, Some("default".to_string()));
    }

    #[test]
    fn rust_lockfile_options_include_install_args() {
        let mut opts = opts_with("profile", "minimal");
        opts.opts.insert(
            "components".to_string(),
            toml::Value::String("clippy, rustfmt".to_string()),
        );
        opts.opts.insert(
            "targets".to_string(),
            toml::Value::String("wasm32-wasip1".to_string()),
        );

        assert_eq!(
            RustOptions::new(&opts).lockfile_options(None),
            BTreeMap::from([
                ("components".to_string(), "clippy,rustfmt".to_string()),
                ("profile".to_string(), "minimal".to_string()),
                ("targets".to_string(), "wasm32-wasip1".to_string()),
            ])
        );
    }

    #[test]
    fn rust_lockfile_options_use_idiomatic_options() {
        let opts = opts_with("profile", "minimal");
        let rt = RustToolchain {
            profile: Some("default".to_string()),
            components: Some(vec!["rustfmt".to_string()]),
            ..Default::default()
        };

        assert_eq!(
            RustOptions::new(&opts).lockfile_options(Some(&rt)),
            BTreeMap::from([
                ("components".to_string(), "rustfmt".to_string()),
                ("profile".to_string(), "default".to_string()),
            ])
        );
    }
}
