use std::path::{Path, PathBuf};
use std::process::Command;
use std::{collections::BTreeMap, collections::BTreeSet, ffi::OsString, sync::Arc};

use crate::backend::VersionInfo;
use crate::backend::options::BackendOptions;
use crate::backend::{Backend, IdiomaticVersion, platform_target::PlatformTarget};
use crate::build_time::TARGET;
use crate::cli::args::BackendArg;
use crate::cmd::{CmdLineRunner, cmd};
use crate::config::{Config, Settings};
use crate::http::HTTP;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ResolveOptions, ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, github, plugins};
use async_trait::async_trait;
use eyre::Result;
use indexmap::IndexMap;
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
        let normalize = |value: &str| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_string())
        };
        match self.values.raw().opts.get(name) {
            Some(toml::Value::String(value)) => {
                Some(value.split(',').filter_map(normalize).collect())
            }
            Some(toml::Value::Array(values)) => Some(
                values
                    .iter()
                    .filter_map(|value| value.as_str().and_then(normalize))
                    .collect(),
            ),
            _ => None,
        }
    }

    fn install_args(&self) -> (Option<String>, Option<Vec<String>>, Option<Vec<String>>) {
        let profile = self.profile().map(str::to_string);
        let components = self.comma_list("components");
        let targets = self.comma_list("targets");

        (profile, components, targets)
    }

    fn lockfile_options(&self) -> BTreeMap<String, String> {
        let (profile, components, targets) = self.install_args();
        let mut opts = BTreeMap::new();

        if let Some(profile) = profile
            && !profile.is_empty()
        {
            opts.insert("profile".into(), profile);
        }
        if let Some(components) = components
            && !components.is_empty()
        {
            let mut components = components;
            components.sort();
            components.dedup();
            opts.insert("components".into(), components.join(","));
        }
        if let Some(targets) = targets
            && !targets.is_empty()
        {
            let mut targets = targets;
            targets.sort();
            targets.dedup();
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

    async fn setup_rustup(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        runtime: &RustRuntime,
    ) -> Result<()> {
        if runtime.is_external() {
            return Ok(());
        }
        let homes = &runtime.homes;
        let settings = Settings::get();
        if rustup_is_initialized(homes) {
            return Ok(());
        }
        let _installer_lock = tokio::task::spawn_blocking(|| {
            LockFile::new(&rustup_path())
                .with_callback(|path| {
                    debug!(
                        "waiting for rustup-init lock on {}",
                        file::display_path(path)
                    );
                })
                .lock()
        })
        .await??;
        if rustup_is_initialized(homes) {
            return Ok(());
        }
        ctx.pr.set_message("Downloading rustup-init".into());
        HTTP.download_file(rustup_url(&settings), &rustup_path(), Some(ctx.pr.as_ref()))
            .await?;
        file::make_executable(rustup_path())?;
        file::create_dir_all(&homes.rustup)?;
        let mut cmd = CmdLineRunner::new(rustup_path())
            .with_pr(ctx.pr.as_ref())
            .arg("--no-modify-path")
            .arg("--default-toolchain")
            .arg("none")
            .arg("-y")
            .env_values(tv.install_env())
            .envs(rustup_env(homes, &tv.version));
        if let Some(host) = settings.rust.default_host.as_ref() {
            cmd = cmd.arg("--default-host").arg(host);
        }
        cmd.execute()?;
        Ok(())
    }

    async fn test_rust(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        runtime: &RustRuntime,
    ) -> Result<()> {
        ctx.pr.set_message(format!("{RUSTC_BIN} -V"));
        CmdLineRunner::new(runtime.bin_dir.join(RUSTC_BIN))
            .with_pr(ctx.pr.as_ref())
            .arg("-V")
            .env_values(tv.install_env())
            .envs(rustup_env(&runtime.homes, &tv.version))
            .prepend_path(vec![runtime.bin_dir.clone()])?
            .execute()
    }

    fn target_triple(&self, tv: &ToolVersion) -> String {
        format!("{}-{}", tv.version, TARGET)
    }

    fn rustup_installed_items(
        &self,
        tv: &ToolVersion,
        subcommand: &str,
        runtime: &RustRuntime,
    ) -> Result<Option<BTreeSet<String>>> {
        let args = vec![
            subcommand.to_string(),
            "list".to_string(),
            "--installed".to_string(),
            "--toolchain".to_string(),
            tv.version.clone(),
        ];
        let mut cmd = cmd(runtime.bin_dir.join(RUSTUP_BIN), args)
            .env("PATH", rustup_path_env(runtime)?)
            .stdout_capture()
            .stderr_capture()
            .unchecked();
        for (key, value) in rustup_env(&runtime.homes, &tv.version) {
            cmd = cmd.env(key, value);
        }
        let output = match cmd.run() {
            Ok(output) => output,
            Err(err) => {
                debug!(
                    "rustup {subcommand} list failed for {}: {err:#}",
                    tv.style()
                );
                return Ok(None);
            }
        };
        if !output.status.success() {
            debug!(
                "rustup {subcommand} list failed for {}: {}",
                tv.style(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(String::from)
                .collect(),
        ))
    }

    fn missing_components(
        &self,
        requested: &[String],
        installed: &BTreeSet<String>,
    ) -> Vec<String> {
        requested
            .iter()
            .filter(|component| !rustup_component_installed(installed, component))
            .cloned()
            .collect()
    }

    fn missing_targets(&self, requested: &[String], installed: &BTreeSet<String>) -> Vec<String> {
        requested
            .iter()
            .filter(|target| !installed.contains(*target))
            .cloned()
            .collect()
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

    /// Rust toolchains can be installed while requested components/targets are
    /// still absent because rustup owns that mutable state outside mise's
    /// install directory.
    async fn is_install_satisfied(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> Result<bool> {
        if !self.is_version_installed(config, tv, check_symlink) {
            return Ok(false);
        }

        let runtime = RustRuntime::resolve_for_tool_version(config, tv).await?;
        if !file::is_symlink_to(&tv.install_path(), &runtime.bin_dir) {
            debug!(
                "{} points outside the selected Rust proxy directory",
                tv.install_path().display()
            );
            return Ok(false);
        }

        let raw_opts = tv.request.options();
        let (_, components, targets) = RustOptions::new(&raw_opts).install_args();

        if let Some(components) = components
            && !components.is_empty()
        {
            let Some(installed) = self.rustup_installed_items(tv, "component", &runtime)? else {
                return Ok(false);
            };
            let missing = self.missing_components(&components, &installed);
            if !missing.is_empty() {
                debug!(
                    "{} missing rustup component(s): {}",
                    tv.style(),
                    missing.join(", ")
                );
                return Ok(false);
            }
        }

        if let Some(targets) = targets
            && !targets.is_empty()
        {
            let Some(installed) = self.rustup_installed_items(tv, "target", &runtime)? else {
                return Ok(false);
            };
            let missing = self.missing_targets(&targets, &installed);
            if !missing.is_empty() {
                debug!(
                    "{} missing rustup target(s): {}",
                    tv.style(),
                    missing.join(", ")
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> Result<BTreeMap<String, String>> {
        let raw_opts = request.options();
        Ok(RustOptions::new(&raw_opts).lockfile_options())
    }

    async fn _list_remote_versions(&self, _config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        let versions: Vec<VersionInfo> = github::list_releases("rust-lang/rust")
            .await?
            .into_iter()
            .map(|r| {
                let created_at = Some(r.released_at().to_string());
                VersionInfo {
                    release_url: Some(format!("https://releases.rs/docs/{}/", r.tag_name)),
                    version: r.tag_name,
                    created_at,
                    ..Default::default()
                }
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

    async fn _parse_idiomatic_file(&self, path: &Path) -> Result<Vec<String>> {
        Ok(self
            ._parse_idiomatic_file_with_options(path)
            .await?
            .into_iter()
            .map(|(version, _)| version)
            .collect())
    }

    async fn _parse_idiomatic_file_with_options(
        &self,
        path: &Path,
    ) -> Result<Vec<IdiomaticVersion>> {
        let rt = parse_idiomatic_file(path)?;
        if rt.channel.is_empty() {
            return Ok(vec![]);
        }
        let options = rt.apply_to_options(ToolVersionOptions::default());
        Ok(vec![(
            rt.channel.clone(),
            (!options.is_empty()).then_some(options),
        )])
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let runtime = RustRuntime::resolve_for_tool_version(&ctx.config, &tv).await?;
        let _state_locks = lock_rust_state(&runtime.homes).await?;
        self.setup_rustup(ctx, &tv, &runtime).await?;

        let raw_opts = tv.request.options();
        let (profile, components, targets) = RustOptions::new(&raw_opts).install_args();

        let mut cmd = CmdLineRunner::new(runtime.bin_dir.join(RUSTUP_BIN))
            .with_pr(ctx.pr.as_ref())
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .opt_args("--component", components)
            .opt_args("--target", targets)
            .prepend_path(vec![runtime.bin_dir.clone()])?
            .env_values(tv.install_env())
            .envs(rustup_env(&runtime.homes, &tv.version));
        if let Some(profile) = profile.as_ref() {
            cmd = cmd.arg("--profile").arg(profile);
        }
        cmd.execute()?;

        file::remove_all(tv.install_path())?;
        file::make_symlink(&runtime.bin_dir, &tv.install_path())?;

        self.test_rust(ctx, &tv, &runtime).await?;

        Ok(tv)
    }

    async fn uninstall_version_impl(
        &self,
        config: &Arc<Config>,
        pr: &dyn SingleReport,
        tv: &ToolVersion,
    ) -> Result<()> {
        let runtime = RustRuntime::resolve_recorded_for_tool_version(config, tv).await?;
        let mut env = rustup_env(&runtime.homes, &tv.version);
        env.remove("RUSTUP_TOOLCHAIN");
        CmdLineRunner::new(runtime.bin_dir.join(RUSTUP_BIN))
            .with_pr(pr)
            .arg("toolchain")
            .arg("uninstall")
            .arg(&tv.version)
            .prepend_path(vec![runtime.bin_dir])?
            .envs(env)
            .execute()
    }

    async fn list_bin_paths(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(vec![
            RustRuntime::resolve_recorded_for_tool_version(config, tv)
                .await?
                .bin_dir,
        ])
    }

    async fn exec_env(
        &self,
        config: &Arc<Config>,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        Ok(rustup_env(&RustHomes::resolve(config).await?, &tv.version))
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
            let runtime = RustRuntime::resolve_recorded_for_tool_version(config, tv).await?;
            let mut cmd = cmd(runtime.bin_dir.join(RUSTUP_BIN), ["check"])
                .env("PATH", self.path_env_for_cmd(config, tv).await?);
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

impl RustToolchain {
    fn apply_to_options(&self, options: ToolVersionOptions) -> ToolVersionOptions {
        let mut opts = options;
        if let Some(profile) = &self.profile {
            opts.opts
                .insert("profile".into(), toml::Value::String(profile.clone()));
        }
        if let Some(components) = &self.components {
            opts.opts
                .insert("components".into(), string_array(components));
        }
        if let Some(targets) = &self.targets {
            opts.opts.insert("targets".into(), string_array(targets));
        }
        opts
    }
}

fn string_array(values: &[String]) -> toml::Value {
    toml::Value::Array(
        values
            .iter()
            .map(|value| toml::Value::String(value.clone()))
            .collect(),
    )
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

fn rust_state_lock_identities(rustup_home: &Path, cargo_home: &Path) -> Vec<PathBuf> {
    let mut identities = vec![
        file::desymlink_path(rustup_home),
        file::desymlink_path(cargo_home),
    ];
    identities.sort();
    identities.dedup();
    identities
}

async fn lock_rust_state(homes: &RustHomes) -> Result<Vec<fslock::LockFile>> {
    let identities = rust_state_lock_identities(&homes.rustup, &homes.cargo);
    tokio::task::spawn_blocking(move || {
        identities
            .into_iter()
            .map(|identity| {
                let display_identity = identity.clone();
                LockFile::new(&identity)
                    .with_callback(move |_| {
                        debug!(
                            "waiting for Rust state lock on {}",
                            file::display_path(&display_identity)
                        );
                    })
                    .lock()
            })
            .collect()
    })
    .await?
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RustHomes {
    cargo: PathBuf,
    rustup: PathBuf,
    explicit: bool,
}

impl RustHomes {
    async fn resolve(config: &Arc<Config>) -> Result<Self> {
        let config_env = config.env().await?;
        let settings = Settings::get();
        Ok(Self::from_sources(
            &config_env,
            settings.rust.cargo_home.clone(),
            env::var_path("CARGO_HOME"),
            settings.rust.rustup_home.clone(),
            env::var_path("RUSTUP_HOME"),
        ))
    }

    fn from_sources(
        config_env: &IndexMap<String, String>,
        configured_cargo: Option<PathBuf>,
        ambient_cargo: Option<PathBuf>,
        configured_rustup: Option<PathBuf>,
        ambient_rustup: Option<PathBuf>,
    ) -> Self {
        let cargo_explicit = config_env.contains_key("CARGO_HOME")
            || config_env.contains_key("MISE_CARGO_HOME")
            || configured_cargo.is_some()
            || ambient_cargo.is_some();
        let rustup_explicit = config_env.contains_key("RUSTUP_HOME")
            || config_env.contains_key("MISE_RUSTUP_HOME")
            || configured_rustup.is_some()
            || ambient_rustup.is_some();
        Self {
            cargo: select_rust_home(
                config_env,
                "CARGO_HOME",
                "MISE_CARGO_HOME",
                configured_cargo,
                ambient_cargo,
                dirs::HOME.join(".cargo"),
            ),
            rustup: select_rust_home(
                config_env,
                "RUSTUP_HOME",
                "MISE_RUSTUP_HOME",
                configured_rustup,
                ambient_rustup,
                dirs::HOME.join(".rustup"),
            ),
            explicit: cargo_explicit || rustup_explicit,
        }
    }

    fn cargo_bindir(&self) -> PathBuf {
        self.cargo.join("bin")
    }

    fn cargo_bin(&self) -> PathBuf {
        self.cargo_bindir().join(CARGO_BIN)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RustProvider {
    Managed,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RustRuntime {
    homes: RustHomes,
    bin_dir: PathBuf,
    provider: RustProvider,
}

impl RustRuntime {
    async fn resolve_for_tool_version(config: &Arc<Config>, tv: &ToolVersion) -> Result<Self> {
        let homes = RustHomes::resolve(config).await?;
        Ok(Self::from_paths_with_install(
            homes,
            &env::PATH,
            &tv.install_path(),
            rustup_proxy_dir_is_usable,
        ))
    }

    async fn resolve_recorded_for_tool_version(
        config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Self> {
        let homes = RustHomes::resolve(config).await?;
        Ok(Self::from_paths_with_install(
            homes,
            &env::PATH,
            &tv.install_path(),
            rustup_proxy_dir_is_usable,
        ))
    }

    fn from_paths_with(
        homes: RustHomes,
        paths: &[PathBuf],
        provider_is_usable: impl Fn(&Path) -> bool,
    ) -> Self {
        if !rustup_is_initialized(&homes)
            && !homes.explicit
            && let Some(bin_dir) = external_rustup_bin_dir(paths, provider_is_usable)
        {
            debug!(
                "using external rustup proxies from {}",
                file::display_path(&bin_dir)
            );
            return Self {
                homes,
                bin_dir,
                provider: RustProvider::External,
            };
        }
        let bin_dir = homes.cargo_bindir();
        Self {
            homes,
            bin_dir,
            provider: RustProvider::Managed,
        }
    }

    fn from_paths_with_install(
        homes: RustHomes,
        paths: &[PathBuf],
        install_path: &Path,
        provider_is_usable: impl Fn(&Path) -> bool,
    ) -> Self {
        if !homes.explicit
            && let Some(bin_dir) =
                recorded_external_rustup_bin_dir(&homes, install_path, &provider_is_usable)
        {
            debug!(
                "using recorded external rustup proxies from {}",
                file::display_path(&bin_dir)
            );
            return Self {
                homes,
                bin_dir,
                provider: RustProvider::External,
            };
        }
        Self::from_paths_with(homes, paths, provider_is_usable)
    }

    fn is_external(&self) -> bool {
        self.provider == RustProvider::External
    }
}

fn recorded_external_rustup_bin_dir(
    homes: &RustHomes,
    install_path: &Path,
    provider_is_usable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    file::resolve_symlink(install_path)
        .ok()
        .flatten()
        .map(|target| {
            if target.is_absolute() {
                target
            } else {
                install_path.parent().unwrap_or(Path::new(".")).join(target)
            }
        })
        .filter(|target| target != &homes.cargo_bindir())
        .filter(|target| provider_is_usable(target))
}

fn external_rustup_bin_dir(
    paths: &[PathBuf],
    provider_is_usable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    paths
        .iter()
        .filter(|path| !file::is_mise_shims_dir(path))
        .find(|path| {
            [RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]
                .iter()
                .all(|bin| file::is_executable(&path.join(bin)))
                && provider_is_usable(path)
        })
        .cloned()
}

fn rustup_proxy_dir_is_usable(bin_dir: &Path) -> bool {
    if !Command::new(bin_dir.join(RUSTUP_BIN))
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return false;
    }

    const PROBE_TOOLCHAIN: &str = "mise-proxy-probe:invalid";
    [CARGO_BIN, RUSTC_BIN].iter().all(|bin| {
        Command::new(bin_dir.join(bin))
            .arg("--version")
            .env("RUSTUP_TOOLCHAIN", PROBE_TOOLCHAIN)
            .env("RUSTUP_AUTO_INSTALL", "0")
            .output()
            .is_ok_and(|output| {
                !output.status.success()
                    && (String::from_utf8_lossy(&output.stdout).contains(PROBE_TOOLCHAIN)
                        || String::from_utf8_lossy(&output.stderr).contains(PROBE_TOOLCHAIN))
            })
    })
}

fn rustup_is_initialized(homes: &RustHomes) -> bool {
    homes.rustup.join("settings.toml").exists() && homes.cargo_bin().exists()
}

fn select_rust_home(
    config_env: &IndexMap<String, String>,
    direct_key: &str,
    mise_key: &str,
    configured: Option<PathBuf>,
    ambient: Option<PathBuf>,
    default: PathBuf,
) -> PathBuf {
    let path = config_env
        .get(direct_key)
        .or_else(|| config_env.get(mise_key))
        .map(PathBuf::from)
        .or(configured)
        .or(ambient)
        .unwrap_or(default);
    resolve_rust_home(path)
}

fn resolve_rust_home(path: PathBuf) -> PathBuf {
    let path = file::replace_path(path);
    if path.is_relative() {
        std::env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    } else {
        path
    }
}

fn rustup_env(homes: &RustHomes, toolchain: &str) -> BTreeMap<String, String> {
    [
        (
            "CARGO_HOME".to_string(),
            homes.cargo.to_string_lossy().to_string(),
        ),
        (
            "RUSTUP_HOME".to_string(),
            homes.rustup.to_string_lossy().to_string(),
        ),
        ("RUSTUP_TOOLCHAIN".to_string(), toolchain.to_string()),
    ]
    .into()
}

fn rustup_path_env(runtime: &RustRuntime) -> Result<OsString> {
    Ok(env::join_paths(
        std::iter::once(runtime.bin_dir.clone()).chain(env::PATH.clone()),
    )?)
}

fn rustup_component_installed(installed: &BTreeSet<String>, component: &str) -> bool {
    installed.iter().any(|item| {
        item == component
            || item
                .strip_prefix(component)
                .and_then(|suffix| suffix.strip_prefix('-'))
                .is_some_and(rustup_component_suffix_is_host_triple)
    })
}

fn rustup_component_suffix_is_host_triple(suffix: &str) -> bool {
    let Some((arch, rest)) = suffix.split_once('-') else {
        return false;
    };
    !rest.is_empty() && RUST_TARGET_ARCHES.contains(&arch)
}

const RUST_TARGET_ARCHES: &[&str] = &[
    "aarch64",
    "arm",
    "armeb",
    "arm64ec",
    "armv4t",
    "armv5te",
    "armv6",
    "armv7",
    "armv7a",
    "armv7r",
    "armv7s",
    "avr",
    "bpfeb",
    "bpfel",
    "csky",
    "hexagon",
    "i386",
    "i586",
    "i686",
    "loongarch64",
    "m68k",
    "mips",
    "mips64",
    "mips64el",
    "mipsel",
    "msp430",
    "nvptx64",
    "powerpc",
    "powerpc64",
    "powerpc64le",
    "riscv32",
    "riscv64",
    "s390x",
    "sparc",
    "sparc64",
    "thumbv4t",
    "thumbv5te",
    "thumbv6m",
    "thumbv7a",
    "thumbv7em",
    "thumbv7m",
    "thumbv7neon",
    "wasm32",
    "wasm64",
    "x86_64",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn rust_homes_at(root: &Path, explicit: bool) -> RustHomes {
        RustHomes {
            cargo: root.join("cargo"),
            rustup: root.join("rustup"),
            explicit,
        }
    }

    fn create_proxy_dir(path: &Path, bins: &[&str]) {
        std::fs::create_dir_all(path).unwrap();
        for bin in bins {
            let path = path.join(bin);
            std::fs::write(&path, b"proxy").unwrap();
            file::make_executable(path).unwrap();
        }
    }

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

        let (profile, components, targets) = RustOptions::new(&opts).install_args();

        assert_eq!(profile, Some("minimal".to_string()));
        assert_eq!(
            components,
            Some(vec!["clippy".to_string(), "rustfmt".to_string()])
        );
        assert_eq!(targets, Some(vec!["wasm32-wasip1".to_string()]));
    }

    #[test]
    fn rust_options_reads_array_install_args() {
        let mut opts = opts_with("profile", "minimal");
        opts.opts.insert(
            "components".to_string(),
            string_array(&[" clippy ".to_string(), "rustfmt".to_string(), String::new()]),
        );
        opts.opts.insert(
            "targets".to_string(),
            string_array(&[
                "wasm32-wasip1".to_string(),
                " wasm32-unknown-unknown ".to_string(),
            ]),
        );

        let (profile, components, targets) = RustOptions::new(&opts).install_args();

        assert_eq!(profile, Some("minimal".to_string()));
        assert_eq!(
            components,
            Some(vec!["clippy".to_string(), "rustfmt".to_string()])
        );
        assert_eq!(
            targets,
            Some(vec![
                "wasm32-wasip1".to_string(),
                "wasm32-unknown-unknown".to_string()
            ])
        );
    }

    #[test]
    fn rustup_component_matching_allows_host_suffixes() {
        let installed = BTreeSet::from([
            "rust-src".to_string(),
            "llvm-tools-x86_64-unknown-linux-gnu".to_string(),
        ]);

        assert!(rustup_component_installed(&installed, "rust-src"));
        assert!(rustup_component_installed(&installed, "llvm-tools"));
        assert!(!rustup_component_installed(&installed, "rustfmt"));
        assert!(!rustup_component_installed(&installed, "rust"));
        assert!(!rustup_component_installed(&installed, "llvm"));
    }

    #[test]
    fn rust_idiomatic_options_override_tool_options() {
        let opts = opts_with("profile", "minimal");
        let rt = RustToolchain {
            profile: Some("default".to_string()),
            ..Default::default()
        };
        let opts = rt.apply_to_options(opts);

        let (profile, _, _) = RustOptions::new(&opts).install_args();

        assert_eq!(profile, Some("default".to_string()));
    }

    #[tokio::test]
    async fn rust_idiomatic_options_are_tool_options() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("rust-toolchain.toml");
        std::fs::write(
            &path,
            r#"
[toolchain]
channel = "1.85.0"
profile = "minimal"
components = [" rustfmt ", "clippy", "clippy"]
targets = ["wasm32-wasip1", " wasm32-wasip1 "]
"#,
        )?;

        let plugin = RustPlugin::new();
        let versions = plugin.parse_idiomatic_file_with_options(&path).await?;
        let (version, options) = versions.into_iter().next().unwrap();

        assert_eq!(version, "1.85.0");
        assert_eq!(
            RustOptions::new(&options).lockfile_options(),
            BTreeMap::from([
                ("components".to_string(), "clippy,rustfmt".to_string()),
                ("profile".to_string(), "minimal".to_string()),
                ("targets".to_string(), "wasm32-wasip1".to_string()),
            ])
        );
        Ok(())
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
            RustOptions::new(&opts).lockfile_options(),
            BTreeMap::from([
                ("components".to_string(), "clippy,rustfmt".to_string()),
                ("profile".to_string(), "minimal".to_string()),
                ("targets".to_string(), "wasm32-wasip1".to_string()),
            ])
        );
    }

    #[test]
    fn rust_idiomatic_options_override_inline_options() {
        let opts = opts_with("profile", "minimal");
        let rt = RustToolchain {
            profile: Some("default".to_string()),
            components: Some(vec!["rustfmt".to_string()]),
            ..Default::default()
        };
        let opts = rt.apply_to_options(opts);

        assert_eq!(
            RustOptions::new(&opts).lockfile_options(),
            BTreeMap::from([
                ("components".to_string(), "rustfmt".to_string()),
                ("profile".to_string(), "default".to_string()),
            ])
        );
    }

    #[test]
    fn rust_lockfile_options_skip_empty_profile() {
        let opts = opts_with("profile", "");

        assert_eq!(RustOptions::new(&opts).lockfile_options(), BTreeMap::new());
    }

    #[test]
    fn rust_home_paths_expand_tilde() {
        assert_eq!(
            resolve_rust_home(PathBuf::from("~/.cargo-custom")),
            dirs::HOME.join(".cargo-custom")
        );
        assert_eq!(
            resolve_rust_home(PathBuf::from("~/.rustup-custom")),
            dirs::HOME.join(".rustup-custom")
        );
    }

    #[test]
    fn rust_state_locks_are_shared_by_either_home() {
        let first = rust_state_lock_identities(Path::new("/rustup/shared"), Path::new("/cargo/a"));
        let same_rustup =
            rust_state_lock_identities(Path::new("/rustup/shared"), Path::new("/cargo/b"));
        let same_cargo =
            rust_state_lock_identities(Path::new("/rustup/other"), Path::new("/cargo/a"));
        let separate =
            rust_state_lock_identities(Path::new("/rustup/other"), Path::new("/cargo/b"));

        assert!(first.iter().any(|identity| same_rustup.contains(identity)));
        assert!(first.iter().any(|identity| same_cargo.contains(identity)));
        assert!(!first.iter().any(|identity| separate.contains(identity)));
    }

    #[test]
    fn rust_state_locks_are_ordered_and_deduplicated() {
        let cargo = file::desymlink_path(Path::new("/a/cargo"));
        let rustup = file::desymlink_path(Path::new("/z/rustup"));
        assert_eq!(
            rust_state_lock_identities(Path::new("/z/rustup"), Path::new("/a/cargo")),
            vec![cargo, rustup]
        );
        assert_eq!(
            rust_state_lock_identities(Path::new("/shared"), Path::new("/shared")),
            vec![file::desymlink_path(Path::new("/shared"))]
        );
    }

    #[cfg(unix)]
    #[test]
    fn rust_state_locks_resolve_filesystem_aliases() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let alias = root.path().join("alias");
        std::fs::create_dir_all(&state).unwrap();
        symlink(&state, &alias).unwrap();

        assert_eq!(
            rust_state_lock_identities(&state, &state),
            rust_state_lock_identities(&alias, &alias)
        );
    }

    #[test]
    fn rust_homes_use_defaults_without_overrides() {
        let homes = RustHomes::from_sources(&indexmap::IndexMap::new(), None, None, None, None);

        assert_eq!(homes.cargo, dirs::HOME.join(".cargo"));
        assert_eq!(homes.rustup, dirs::HOME.join(".rustup"));
        assert!(!homes.explicit);
    }

    #[test]
    fn rust_homes_follow_config_environment_precedence() {
        let config_env = indexmap::IndexMap::from([
            (
                "MISE_CARGO_HOME".to_string(),
                "/config/mise-cargo".to_string(),
            ),
            ("CARGO_HOME".to_string(), "/config/cargo".to_string()),
            (
                "MISE_RUSTUP_HOME".to_string(),
                "/config/mise-rustup".to_string(),
            ),
            ("RUSTUP_HOME".to_string(), "/config/rustup".to_string()),
        ]);

        let homes = RustHomes::from_sources(
            &config_env,
            Some("/settings/cargo".into()),
            Some("/ambient/cargo".into()),
            Some("/settings/rustup".into()),
            Some("/ambient/rustup".into()),
        );

        assert_eq!(
            homes.cargo,
            resolve_rust_home(PathBuf::from("/config/cargo"))
        );
        assert_eq!(
            homes.rustup,
            resolve_rust_home(PathBuf::from("/config/rustup"))
        );
        assert!(homes.explicit);
    }

    #[test]
    fn rust_homes_use_config_mise_environment_before_existing_sources() {
        let config_env = indexmap::IndexMap::from([(
            "MISE_CARGO_HOME".to_string(),
            "/config/cargo".to_string(),
        )]);

        let homes = RustHomes::from_sources(
            &config_env,
            Some("/settings/cargo".into()),
            Some("/ambient/cargo".into()),
            Some("/settings/rustup".into()),
            Some("/ambient/rustup".into()),
        );

        assert_eq!(
            homes.cargo,
            resolve_rust_home(PathBuf::from("/config/cargo"))
        );
        assert_eq!(
            homes.rustup,
            resolve_rust_home(PathBuf::from("/settings/rustup"))
        );
        assert!(homes.explicit);
    }

    #[test]
    fn rust_runtime_prefers_initialized_homes() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        create_proxy_dir(&homes.cargo_bindir(), &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        std::fs::create_dir_all(&homes.rustup).unwrap();
        std::fs::write(homes.rustup.join("settings.toml"), b"").unwrap();
        let external = root.path().join("external/bin");
        create_proxy_dir(&external, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);

        let runtime = RustRuntime::from_paths_with(homes.clone(), &[external], |_| true);

        assert_eq!(runtime.provider, RustProvider::Managed);
        assert_eq!(runtime.bin_dir, homes.cargo_bindir());
    }

    #[test]
    fn rust_runtime_uses_complete_external_provider() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        let external = root.path().join("external/bin");
        create_proxy_dir(&external, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);

        let runtime =
            RustRuntime::from_paths_with(homes, std::slice::from_ref(&external), |_| true);

        assert_eq!(runtime.provider, RustProvider::External);
        assert_eq!(runtime.bin_dir, external);
    }

    #[test]
    fn rust_runtime_prefers_recorded_external_provider() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        let recorded = root.path().join("recorded/bin");
        let earlier_on_path = root.path().join("earlier/bin");
        create_proxy_dir(&recorded, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        create_proxy_dir(&earlier_on_path, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        let install_path = root.path().join("installs/rust/1.80.0");
        std::fs::create_dir_all(install_path.parent().unwrap()).unwrap();
        file::make_symlink(&recorded, &install_path).unwrap();

        let runtime =
            RustRuntime::from_paths_with_install(homes, &[earlier_on_path], &install_path, |_| {
                true
            });

        assert_eq!(runtime.provider, RustProvider::External);
        assert_eq!(runtime.bin_dir, recorded);
    }

    #[test]
    fn rust_runtime_keeps_recorded_external_provider_after_default_home_initialization() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        create_proxy_dir(&homes.cargo_bindir(), &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        std::fs::create_dir_all(&homes.rustup).unwrap();
        std::fs::write(homes.rustup.join("settings.toml"), b"").unwrap();
        let recorded = root.path().join("recorded/bin");
        create_proxy_dir(&recorded, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        let install_path = root.path().join("installs/rust/1.80.0");
        std::fs::create_dir_all(install_path.parent().unwrap()).unwrap();
        file::make_symlink(&recorded, &install_path).unwrap();

        let runtime = RustRuntime::from_paths_with_install(
            homes,
            std::slice::from_ref(&recorded),
            &install_path,
            |_| true,
        );

        assert_eq!(runtime.provider, RustProvider::External);
        assert_eq!(runtime.bin_dir, recorded);
    }

    #[test]
    fn rust_runtime_explicit_homes_override_recorded_external_provider() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), true);
        create_proxy_dir(&homes.cargo_bindir(), &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        std::fs::create_dir_all(&homes.rustup).unwrap();
        std::fs::write(homes.rustup.join("settings.toml"), b"").unwrap();
        let recorded = root.path().join("recorded/bin");
        create_proxy_dir(&recorded, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);
        let install_path = root.path().join("installs/rust/1.80.0");
        std::fs::create_dir_all(install_path.parent().unwrap()).unwrap();
        file::make_symlink(&recorded, &install_path).unwrap();

        let runtime = RustRuntime::from_paths_with_install(
            homes.clone(),
            std::slice::from_ref(&recorded),
            &install_path,
            |_| true,
        );

        assert_eq!(runtime.provider, RustProvider::Managed);
        assert_eq!(runtime.bin_dir, homes.cargo_bindir());
    }

    #[test]
    fn rust_runtime_rejects_incomplete_external_provider() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        let external = root.path().join("external/bin");
        create_proxy_dir(&external, &[RUSTUP_BIN, CARGO_BIN]);

        let runtime = RustRuntime::from_paths_with(homes.clone(), &[external], |_| true);

        assert_eq!(runtime.provider, RustProvider::Managed);
        assert_eq!(runtime.bin_dir, homes.cargo_bindir());
    }

    #[test]
    fn rust_runtime_does_not_replace_explicit_homes() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), true);
        let external = root.path().join("external/bin");
        create_proxy_dir(&external, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN]);

        let runtime = RustRuntime::from_paths_with(homes.clone(), &[external], |_| true);

        assert_eq!(runtime.provider, RustProvider::Managed);
        assert_eq!(runtime.bin_dir, homes.cargo_bindir());
    }
}
