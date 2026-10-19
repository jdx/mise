use std::path::{Path, PathBuf};
use std::process::Command;
use std::{collections::BTreeMap, collections::BTreeSet, ffi::OsString, sync::Arc};

use crate::args::BackendArg;
use crate::backend::VersionInfo;
use crate::backend::options::BackendOptions;
use crate::backend::{Backend, IdiomaticVersion, platform_target::PlatformTarget};
use crate::build_time::TARGET;
use crate::cmd::{CmdLineRunner, cmd};
use crate::config::{Config, Settings, SettingsExt};
use crate::http::{HTTP, HTTP_FETCH};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ResolveOptions, ToolRequest, ToolVersion, ToolVersionOptions, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, github, plugins};
use async_trait::async_trait;
use eyre::{Context, Result, bail};
use indexmap::IndexMap;
use xx::regex;

mod nightly_channel;

#[derive(Debug)]
pub(super) struct RustPlugin {
    ba: Arc<BackendArg>,
    /// Channels `rustup check` reported an update for in this run, so the install
    /// that follows an upgrade does not ask rustup (and the network) again.
    updatable_channels: std::sync::Mutex<BTreeSet<String>>,
}

const RUST_DEFAULT_DIST_SERVER: &str = "https://static.rust-lang.org";
const RUST_MINIMAL_PROFILE_COMPONENTS: &[&str] = &["cargo", "rust-std", "rustc"];
const RUST_DEFAULT_PROFILE_COMPONENTS: &[&str] = &["clippy", "rust-docs", "rustfmt"];

fn parse_channel_manifest_date(manifest: &str, channel: &str) -> Result<jiff::civil::Date> {
    let manifest: toml::Value = toml::from_str(manifest)
        .wrap_err_with(|| format!("failed to parse the Rust {channel} channel manifest"))?;
    let date = manifest
        .get("date")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre::eyre!("Rust {channel} channel manifest is missing its date"))?;
    date.parse::<jiff::civil::Date>()
        .wrap_err_with(|| format!("invalid Rust {channel} channel manifest date: {date}"))
}

fn parse_nightly_manifest(manifest: &str) -> Result<String> {
    let date = parse_channel_manifest_date(manifest, "nightly")?;
    Ok(format!("nightly-{date}"))
}

/// A channel's date as a `created_at` timestamp. Rolling channels have no release date of their
/// own, so this is the date of the build they point at right now.
fn channel_created_at(date: jiff::civil::Date) -> String {
    format!("{date}T00:00:00Z")
}

/// `stable` resolves under a release-age cutoff to the newest release old enough.
fn cutoff_channel_target(version: &str) -> Option<String> {
    (version == "stable").then(|| "latest".to_string())
}

/// The date of the build a `nightly-YYYY-MM-DD` version names, which is in the name itself.
fn dated_nightly_created_at(version: &str) -> Option<String> {
    let date = version.strip_prefix("nightly-")?.parse().ok()?;
    Some(channel_created_at(date))
}

/// The date the beta or stable channel currently points at, or `None` when its manifest can't be
/// read: an undated entry is better than failing the whole listing.
async fn current_channel_created_at(
    channel: &str,
    config_env: &IndexMap<String, String>,
) -> Option<String> {
    let url = channel_manifest_url_for(rustup_dist_server(config_env).as_deref(), channel);
    let manifest = HTTP_FETCH
        .get_text_cached(&url)
        .await
        .inspect_err(|err| debug!("failed to fetch the Rust {channel} channel manifest: {err:#}"))
        .ok()?;
    parse_channel_manifest_date(&manifest, channel)
        .inspect_err(|err| debug!("{err:#}"))
        .ok()
        .map(channel_created_at)
}

/// Where the nightly channel manifest lives. Honors `RUSTUP_DIST_SERVER`, the
/// variable rustup uses to find toolchains, so a mirror serves the manifest too.
fn nightly_manifest_url(config_env: &IndexMap<String, String>) -> String {
    nightly_manifest_url_for(rustup_dist_server(config_env).as_deref())
}

/// The `RUSTUP_DIST_SERVER` mirror, from the config `[env]` or else the process
/// environment, when one is set.
fn rustup_dist_server(config_env: &IndexMap<String, String>) -> Option<String> {
    config_env
        .get("RUSTUP_DIST_SERVER")
        .cloned()
        .or_else(|| std::env::var("RUSTUP_DIST_SERVER").ok())
        .filter(|server| !server.is_empty())
}

const RUSTUP_MIRROR_VARS: [&str; 2] = ["RUSTUP_DIST_SERVER", "RUSTUP_UPDATE_ROOT"];

/// The non-empty `RUSTUP_DIST_SERVER` and `RUSTUP_UPDATE_ROOT` mirror variables
/// set in the config `[env]`. A `mise install` outside an activated shell does
/// not see them in its own environment, so rustup is handed them explicitly.
fn rustup_mirror_env(
    config_env: &IndexMap<String, String>,
) -> impl Iterator<Item = (&str, &String)> {
    RUSTUP_MIRROR_VARS
        .into_iter()
        .filter_map(|k| config_env.get(k).map(|v| (k, v)))
        .filter(|(_, v)| !v.is_empty())
}

/// Hands rustup the config's mirror variables. One the config sets to an empty
/// string is removed, so an inherited value is masked and older rustup releases
/// never see an empty URL.
fn apply_rustup_mirror_env<'a>(
    mut cmd: CmdLineRunner<'a>,
    config_env: &IndexMap<String, String>,
) -> CmdLineRunner<'a> {
    for key in RUSTUP_MIRROR_VARS {
        if config_env.get(key).is_some_and(String::is_empty) {
            cmd = cmd.env_remove(key);
        }
    }
    cmd.envs(rustup_mirror_env(config_env))
}

/// A mirror can serve the channels without GitHub being reachable (an offline
/// network), so a failed release listing then only loses the numbered releases.
/// The flag says the listing is degraded, so it must not be cached as complete.
fn releases_or_channels_only<T>(
    releases: Result<Vec<T>>,
    mirrored: bool,
) -> Result<(Vec<T>, bool)> {
    match releases {
        Err(err) if mirrored => {
            warn!("failed to list rust releases from GitHub, using channels only: {err:#}");
            Ok((vec![], true))
        }
        other => other.map(|releases| (releases, false)),
    }
}

fn nightly_manifest_url_for(dist_server: Option<&str>) -> String {
    channel_manifest_url_for(dist_server, "nightly")
}

/// How far back from a release-age cutoff to look for the build a channel pointed at then. Betas
/// are published when the train moves, nightlies almost daily.
fn cutoff_lookback_days(channel: &str) -> i64 {
    if channel == "beta" { 90 } else { 30 }
}

/// The manifest rustup archives for `channel` on `date`, under the mirror prefix as given.
fn dated_channel_manifest_url_for(
    dist_server: Option<&str>,
    channel: &str,
    date: jiff::civil::Date,
) -> String {
    let server = dist_server
        .map(|s| s.trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .unwrap_or(RUST_DEFAULT_DIST_SERVER);
    format!("{server}/dist/{date}/channel-rust-{channel}.toml")
}

fn channel_manifest_url_for(dist_server: Option<&str>, channel: &str) -> String {
    let server = dist_server
        .map(|s| s.trim_end_matches('/'))
        .filter(|s| !s.is_empty())
        .unwrap_or(RUST_DEFAULT_DIST_SERVER);
    format!("{server}/dist/channel-rust-{channel}.toml")
}

async fn current_nightly_version(config: &Arc<Config>) -> Result<String> {
    let url = nightly_manifest_url(&config.env().await?);
    let manifest = HTTP_FETCH
        .get_text_cached(&url)
        .await
        .wrap_err("failed to fetch the Rust nightly channel manifest")?;
    parse_nightly_manifest(&manifest)
}

fn is_dated_nightly(version: &str) -> bool {
    version
        .strip_prefix("nightly-")
        .is_some_and(|date| date.parse::<jiff::civil::Date>().is_ok())
}

fn is_dated_beta(version: &str) -> bool {
    version
        .strip_prefix("beta-")
        .is_some_and(|date| date.parse::<jiff::civil::Date>().is_ok())
}

fn latest_installed_nightly(versions: impl DoubleEndedIterator<Item = String>) -> Option<String> {
    versions.rev().find(|version| is_dated_nightly(version))
}

/// Returns the installed and available versions that `rustup check` reports for
/// `toolchain`, or `None` when that toolchain has no update. rustup 1.29 changed
/// `Update available : ` to `update available: `, so both spellings match.
fn rustup_check_update<'a>(output: &'a str, toolchain: &str) -> Option<(&'a str, &'a str)> {
    let status_re = regex!(r"^(?i:update available)\s*:\s*(.+?)\s+->\s+(.+?)\s*$");
    output.lines().find_map(|line| {
        let status = line.strip_prefix(toolchain)?.strip_prefix(" - ")?;
        let caps = status_re.captures(status)?;
        Some((caps.get(1)?.as_str(), caps.get(2)?.as_str()))
    })
}

/// The names under which `rustup check` may list the toolchain installed for
/// `version`, most specific first.
fn rustup_check_toolchain_names(version: &str, default_host: Option<&str>) -> Vec<String> {
    if rustup_toolchain_is_host_qualified(version) {
        return vec![version.to_string()];
    }
    let mut names = Vec::new();
    if let Some(host) = default_host {
        names.push(format!("{version}-{host}"));
    }
    let built_for = format!("{version}-{TARGET}");
    if !names.contains(&built_for) {
        names.push(built_for);
    }
    names
}

/// The only `<version>-<host>` toolchain `rustup check` lists, if there is
/// exactly one.
fn rustup_check_sole_host_toolchain(output: &str, version: &str) -> Option<String> {
    let mut names = output.lines().filter_map(|line| {
        let name = line.split_once(" - ")?.0;
        let host = name.strip_prefix(version)?.strip_prefix('-')?;
        rustup_component_suffix_is_host_triple(host).then(|| name.to_string())
    });
    let sole = names.next()?;
    names.next().is_none().then_some(sole)
}

fn rustup_toolchain_is_host_qualified(version: &str) -> bool {
    version
        .split_once('-')
        .is_some_and(|(_, host)| rustup_component_suffix_is_host_triple(host))
}

/// `stable` and `beta` (and a host-qualified `nightly-<host>`) move under a
/// fixed name, so a toolchain installed under one only ever updates when
/// `rustup check` says so. Dated nightlies and exact releases are pinned, and
/// bare `nightly` is resolved to a dated one.
fn is_moving_channel(version: &str) -> bool {
    let channel = version
        .split_once('-')
        .map_or(version, |(channel, _)| channel);
    match channel {
        "stable" | "beta" => channel == version || rustup_toolchain_is_host_qualified(version),
        "nightly" => rustup_toolchain_is_host_qualified(version),
        _ => false,
    }
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
    pub(super) fn new() -> Self {
        Self {
            ba: plugins::core::new_backend_arg("rust").into(),
            updatable_channels: Default::default(),
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
        let config_env = ctx.config.env().await?;
        HTTP.download_file(
            rustup_url(&settings, &config_env),
            &rustup_path(),
            Some(ctx.pr.as_ref()),
        )
        .await?;
        file::make_executable(rustup_path())?;
        file::create_dir_all(&homes.rustup)?;
        let mut cmd = CmdLineRunner::new(rustup_path())
            .with_pr(ctx.pr.as_ref())
            .arg("--no-modify-path")
            .arg("--default-toolchain")
            .arg("none")
            .arg("-y");
        cmd = apply_rustup_mirror_env(cmd, &config_env)
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

    /// Gives rustup a `nightly` toolchain matching the dated nightly that
    /// the rolling `nightly` request resolved to, so `cargo +nightly` keeps
    /// working. An explicitly dated request leaves rustup's `nightly` alone.
    fn link_nightly_channel(&self, tv: &ToolVersion, runtime: &RustRuntime) -> Result<()> {
        if !is_dated_nightly(&tv.version) || tv.request.version() == tv.version {
            return Ok(());
        }
        let mut cmd = cmd(runtime.bin_dir.join(RUSTC_BIN), ["--print", "sysroot"])
            .env("PATH", rustup_path_env(runtime)?)
            .stderr_capture();
        for (key, value) in rustup_env(&runtime.homes, &tv.version) {
            cmd = cmd.env(key, value);
        }
        let sysroot = PathBuf::from(cmd.read()?.trim());
        // Only touch a rustup-managed `toolchains/<name>-<host>` directory.
        let (Some(toolchains), Some(host_suffix)) = (
            sysroot
                .parent()
                .filter(|dir| dir.file_name().is_some_and(|name| name == "toolchains")),
            sysroot
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix(tv.version.as_str())),
        ) else {
            debug!(
                "not linking rustup nightly: unexpected sysroot {}",
                file::display_path(&sysroot)
            );
            return Ok(());
        };
        let dated = format!("{}{host_suffix}", tv.version);
        let alias = format!("nightly{host_suffix}");
        if nightly_channel::refresh(toolchains, &dated, &alias)? {
            debug!("linked rustup {alias} to {dated}");
        }
        Ok(())
    }

    /// Runs `rustup check` for the toolchain's rustup and returns its stdout.
    async fn rustup_check(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<String> {
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
        Ok(String::from_utf8_lossy(&result.stdout).into_owned())
    }

    /// Finds the update `rustup check` reports for `tv`'s toolchain, trying the
    /// names rustup may list it under: the version itself when it is already
    /// host-qualified, otherwise the configured default host and then the host
    /// mise was built for.
    fn rustup_check_update_for<'a>(
        &self,
        output: &'a str,
        tv: &ToolVersion,
    ) -> Option<(String, &'a str, &'a str)> {
        let settings = Settings::get();
        let named =
            rustup_check_toolchain_names(&tv.version, settings.rust.default_host.as_deref())
                .into_iter()
                .find_map(|name| {
                    let (current, available) = rustup_check_update(output, &name)?;
                    Some((name, current, available))
                });
        if named.is_some() {
            return named;
        }
        // rustup may have been initialized for a host other than the configured
        // one or the one mise was built for. A single host-qualified listing of
        // this toolchain is unambiguous; with several, give up.
        let name = rustup_check_sole_host_toolchain(output, &tv.version)?;
        let (current, available) = rustup_check_update(output, &name)?;
        Some((name, current, available))
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

    /// Returns the profile rustup applies when an install omits `--profile`.
    fn rustup_default_profile(&self, tv: &ToolVersion, runtime: &RustRuntime) -> Result<String> {
        let args = vec!["show".to_string(), "profile".to_string()];
        let mut cmd = cmd(runtime.bin_dir.join(RUSTUP_BIN), args)
            .env("PATH", rustup_path_env(runtime)?)
            .stdout_capture()
            .stderr_capture()
            .unchecked();
        for (key, value) in rustup_env(&runtime.homes, &tv.version) {
            cmd = cmd.env(key, value);
        }
        let output = cmd.run()?;
        if !output.status.success() {
            bail!(
                "rustup show profile failed for {}: {}",
                tv.style(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let profile = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if profile.is_empty() {
            bail!(
                "rustup show profile returned an empty profile for {}",
                tv.style()
            );
        }
        Ok(profile)
    }

    fn missing_components(
        &self,
        requested: &[String],
        installed: &BTreeSet<String>,
    ) -> Vec<String> {
        let host = rustup_installed_host(installed);
        requested
            .iter()
            .filter(|component| !rustup_component_installed(installed, component, host))
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

    /// Rust toolchains can be absent or missing requested components/targets
    /// while mise's install symlink still exists because rustup owns that
    /// mutable state outside mise's install directory.
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
        let (profile, components, targets) = RustOptions::new(&raw_opts).install_args();
        let effective_profile = match profile {
            Some(profile) => profile,
            None => self.rustup_default_profile(tv, &runtime)?,
        };
        let effective_profile = normalize_rustup_profile(&effective_profile)?;

        // Query components even when none were explicitly requested. This
        // verifies that rustup still has the toolchain represented by mise's
        // install symlink after restoring only mise's data directory.
        let Some(installed_components) = self.rustup_installed_items(tv, "component", &runtime)?
        else {
            return Ok(false);
        };

        let mut required_components = components.unwrap_or_default();
        required_components.extend(profile_components(effective_profile));
        required_components.sort();
        required_components.dedup();

        if !required_components.is_empty() {
            let missing = self.missing_components(&required_components, &installed_components);
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

    async fn remote_version_cache_context(&self, config: &Arc<Config>) -> Result<Option<String>> {
        // The nightly entry comes from the mirror's manifest, so a listing cached
        // for one mirror must not serve another. The default server keeps the
        // shared cache and versions host.
        Ok(rustup_dist_server(&config.env().await?)
            .map(|server| crate::hash::hash_to_str(&server.trim_end_matches('/'))))
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> Result<Vec<VersionInfo>> {
        Ok(self._list_remote_versions_checked(config).await?.0)
    }

    async fn _list_remote_versions_checked(
        &self,
        config: &Arc<Config>,
    ) -> Result<(Vec<VersionInfo>, bool)> {
        let mirrored = rustup_dist_server(&config.env().await?).is_some();
        let (releases, degraded) =
            releases_or_channels_only(github::list_releases("rust-lang/rust").await, mirrored)?;
        let mut versions: Vec<VersionInfo> = releases
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
            .collect();
        let config_env = config.env().await?;
        let (beta_created_at, stable_created_at) = tokio::join!(
            current_channel_created_at("beta", &config_env),
            current_channel_created_at("stable", &config_env),
        );
        let nightly_created_at = match current_nightly_version(config).await {
            Ok(current_nightly) => {
                let created_at = dated_nightly_created_at(&current_nightly);
                versions.push(VersionInfo {
                    version: current_nightly,
                    created_at: created_at.clone(),
                    ..Default::default()
                });
                created_at
            }
            Err(_) => None,
        };
        // The rolling channels carry no release date of their own, so each takes the date of the
        // build it points at right now. A cutoff then hides `nightly` (built today) and any
        // channel whose current build is newer than the cutoff, instead of passing it undated.
        versions.extend([
            VersionInfo {
                version: "nightly".into(),
                created_at: nightly_created_at,
                rolling: true,
                ..Default::default()
            },
            VersionInfo {
                version: "beta".into(),
                created_at: beta_created_at,
                rolling: true,
                ..Default::default()
            },
            VersionInfo {
                version: "stable".into(),
                created_at: stable_created_at,
                rolling: true,
                ..Default::default()
            },
        ]);
        Ok((versions, degraded))
    }

    async fn channel_for_cutoff(
        &self,
        config: &Arc<Config>,
        version: &str,
        before: jiff::Timestamp,
        prefer_offline: bool,
    ) -> Result<Option<String>> {
        if let Some(target) = cutoff_channel_target(version) {
            return Ok(Some(target));
        }
        if !matches!(version, "beta" | "nightly") {
            return Ok(None);
        }
        // Looking up the build costs the network, which prefer-offline avoids.
        if prefer_offline {
            return Ok(None);
        }
        // Manifests are dated by day, so only a build from a day before the cutoff's own day is
        // surely old enough; the newest such one is the build the cutoff allows.
        let config_env = config.env().await?;
        let day = before.to_zoned(jiff::tz::TimeZone::UTC).date();
        let current = current_channel_created_at(version, &config_env)
            .await
            .and_then(|ts| ts.parse::<jiff::Timestamp>().ok())
            .map(|ts| ts.to_zoned(jiff::tz::TimeZone::UTC).date());
        // Already old enough: pin to that build so a lockfile or an upgrade cannot move it.
        if let Some(date) = current.filter(|date| *date < day) {
            return Ok(Some(format!("{version}-{date}")));
        }
        let server = rustup_dist_server(&config_env);
        for date in (1..=cutoff_lookback_days(version))
            .filter_map(|n| day.checked_sub(jiff::Span::new().days(n)).ok())
        {
            let url = dated_channel_manifest_url_for(server.as_deref(), version, date);
            if HTTP_FETCH
                .head(&url)
                .await
                .is_ok_and(|rsp| rsp.status().is_success())
            {
                return Ok(Some(format!("{version}-{date}")));
            }
        }
        bail!(
            "no rust {version} build found within {} days before the release-age cutoff",
            cutoff_lookback_days(version)
        )
    }

    fn is_rolling_channel(&self, version: &str) -> bool {
        version == "nightly"
    }

    fn latest_installed_channel_version(&self, channel: &str) -> Option<String> {
        if !self.is_rolling_channel(channel) {
            return None;
        }
        latest_installed_nightly(self.list_installed_versions().into_iter())
    }

    async fn resolve_channel_version(
        &self,
        config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        if !self.is_rolling_channel(version) {
            return Ok(None);
        }
        current_nightly_version(config).await.map(Some)
    }

    fn requires_concrete_channel_version(&self, version: &str) -> bool {
        self.is_rolling_channel(version)
    }

    fn is_exact_version(&self, version: &str) -> bool {
        is_dated_nightly(version) || is_dated_beta(version)
    }

    async fn resolve_exact_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        Ok((is_dated_nightly(version) || is_dated_beta(version)).then(|| version.to_string()))
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
        let effective_profile = match profile.as_deref() {
            Some(profile) => profile.to_string(),
            None => self.rustup_default_profile(&tv, &runtime)?,
        };
        let effective_profile = normalize_rustup_profile(&effective_profile)?;
        let mut components = components.unwrap_or_default();
        components.extend(profile_components(effective_profile));
        components.sort();
        components.dedup();

        let config_env = ctx.config.env().await?;
        let cmd = CmdLineRunner::new(runtime.bin_dir.join(RUSTUP_BIN))
            .with_pr(ctx.pr.as_ref())
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .opt_args("--component", Some(components))
            .opt_args("--target", targets)
            .prepend_path(vec![runtime.bin_dir.clone()])?;
        let mut cmd = apply_rustup_mirror_env(cmd, &config_env)
            .env_values(tv.install_env())
            .envs(rustup_env(&runtime.homes, &tv.version));
        if let Some(profile) = profile.as_ref() {
            cmd = cmd.arg("--profile").arg(profile);
        }
        cmd.execute()?;

        file::remove_all(tv.install_path())?;
        file::make_symlink(&runtime.bin_dir, &tv.install_path())?;

        self.test_rust(ctx, &tv, &runtime).await?;

        if let Err(err) = self.link_nightly_channel(&tv, &runtime) {
            warn!("failed to make rustup's nightly toolchain match {tv}: {err:#}");
        }
        self.updatable_channels.lock().unwrap().remove(&tv.version);

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
        let runtime = RustRuntime::resolve_recorded_for_tool_version(config, tv).await?;
        runtime.bin_paths(&dirs::DATA.join("rust-proxies"))
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
        let requested = tv.request.version();
        if requested == "nightly" {
            if Settings::get().offline() || opts.offline {
                let oi = OutdatedInfo::new(config, tv.clone(), tv.version.clone())?;
                return Ok((oi.current.as_ref() != Some(&tv.version)).then_some(oi));
            }
            let latest = current_nightly_version(config).await?;
            let oi = OutdatedInfo::new(config, tv.clone(), latest.clone())?;
            return Ok((oi.current.as_ref() != Some(&latest)).then_some(oi));
        }
        if is_dated_nightly(&requested) {
            let latest = if bump && !Settings::get().offline() && !opts.offline {
                current_nightly_version(config).await?
            } else {
                tv.version.clone()
            };
            let mut oi = OutdatedInfo::new(config, tv.clone(), latest.clone())?;
            if bump && requested != latest {
                oi.bump = Some(latest.clone());
                oi.tool_request = ToolRequest::new_with_options(
                    tv.request.ba().clone(),
                    &latest,
                    tv.request.options(),
                    tv.request.source().clone(),
                )?;
                oi.tool_request
                    .set_lockfile_scope(tv.request.lockfile_scope().clone());
            }
            return Ok((oi.current.as_ref() != Some(&latest)).then_some(oi));
        }
        if regex!(r"(\d+)\.(\d+)\.(\d+)").is_match(&tv.version) {
            let oi = OutdatedInfo::resolve(config, tv.clone(), bump, opts).await?;
            Ok(oi)
        } else {
            if Settings::get().offline() || opts.offline {
                return Ok(None);
            }
            let out = self.rustup_check(config, tv).await?;
            let update = self.rustup_check_update_for(&out, tv);
            if is_moving_channel(&tv.version) {
                let mut updatable = self.updatable_channels.lock().unwrap();
                if update.is_some() {
                    updatable.insert(tv.version.clone());
                } else {
                    updatable.remove(&tv.version);
                }
            }
            let Some((toolchain, current, available)) = update else {
                return Ok(None);
            };
            debug!("rustup check: {toolchain} {current} -> {available}");
            // `mise upgrade` keeps a channel installed and updates it in place only
            // when latest equals current, so latest stays the channel name.
            let oi = OutdatedInfo::new(config, tv.clone(), tv.version.clone())?;
            Ok(Some(oi))
        }
    }

    /// A `stable` or `beta` toolchain keeps its name when rustup updates it, so
    /// mise's install marker never goes stale. Reuse the answer `outdated_info`
    /// got from `rustup check`, so that `mise upgrade` reinstalls the channel
    /// instead of skipping it as installed while a plain `mise install` of an
    /// installed channel never goes to the network.
    async fn is_rolling_version_outdated(&self, _config: &Arc<Config>, tv: &ToolVersion) -> bool {
        is_moving_channel(&tv.version)
            && self
                .updatable_channels
                .lock()
                .unwrap()
                .contains(&tv.version)
    }

    /// Confirm the compiler of a restored install still runs, which
    /// `is_install_satisfied` (rustup's component state) does not.
    async fn verify_repaired_install(&self, ctx: &InstallContext, tv: &ToolVersion) -> Result<()> {
        let runtime = RustRuntime::resolve_for_tool_version(&ctx.config, tv).await?;
        self.test_rust(ctx, tv, &runtime).await
    }

    /// `rustup toolchain install stable` updates the installed toolchain in place.
    fn updates_rolling_version_in_place(&self) -> bool {
        true
    }

    fn uses_custom_outdated_info(&self) -> bool {
        true
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

/// Where to download `rustup-init` from. Honors `RUSTUP_UPDATE_ROOT`, the variable
/// rustup itself uses to find its own releases, so a mirror such as panamax works
/// for the bootstrap download as well as for the toolchains.
fn rustup_url(settings: &Settings, config_env: &IndexMap<String, String>) -> String {
    let root = config_env
        .get("RUSTUP_UPDATE_ROOT")
        .cloned()
        .or_else(|| std::env::var("RUSTUP_UPDATE_ROOT").ok());
    rustup_url_for(root.as_deref(), &rustup_host_triple(settings))
}

fn rustup_url_for(update_root: Option<&str>, triple: &str) -> String {
    match update_root.map(|r| r.trim_end_matches('/')) {
        Some(root) if !root.is_empty() => {
            format!("{root}/dist/{triple}/{RUSTUP_INIT_BIN}")
        }
        _ => default_rustup_url(triple),
    }
}

#[cfg(unix)]
fn default_rustup_url(_triple: &str) -> String {
    "https://sh.rustup.rs".to_string()
}

#[cfg(windows)]
fn default_rustup_url(triple: &str) -> String {
    let arch = triple.split('-').next().unwrap_or(triple);
    format!("https://win.rustup.rs/{arch}")
}

/// The triple of the machine `rustup-init` runs on. `rust.default_host` is the
/// toolchain host rustup is told to install, which may differ, so it is not used here.
fn rustup_host_triple(settings: &Settings) -> String {
    let target = crate::build_time::TARGET;
    // An explicit `arch` setting (e.g. x64 under emulation) swaps the architecture
    // of the build target, keeping its vendor, OS and ABI.
    if settings.arch.is_some() {
        let arch = match settings.arch() {
            "x64" => "x86_64",
            "arm64" => "aarch64",
            other => other,
        };
        if let Some((_, rest)) = target.split_once('-') {
            // `eabi`/`eabihf` belong to 32-bit ARM targets only.
            let rest = if arch.starts_with("arm") {
                rest
            } else {
                rest.trim_end_matches("eabihf").trim_end_matches("eabi")
            };
            return format!("{arch}-{rest}");
        }
    }
    target.to_string()
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

/// Serializes rustup state changes for the homes `config` resolves to. Callers that spawn
/// `cargo` (which may run rustup itself) must hold this so they don't race the rust plugin.
pub(crate) async fn lock_rust_state_for_config(
    config_env: &IndexMap<String, String>,
    toolset_env: &BTreeMap<String, String>,
    install_env: &IndexMap<String, crate::config::env_directive::EnvValue>,
) -> Result<Vec<fslock::LockFile>> {
    // The homes the rust plugin locks. `install_env` belongs to this tool only, and other
    // tools' exec env never reaches `RustHomes::resolve`, so neither affects them.
    let plugin_homes = RustHomes::from_config_env(config_env);

    // The homes the spawned `cargo` uses: it inherits the process env, then the toolset
    // env and `install_env` override it. Cargo only reads CARGO_HOME/RUSTUP_HOME.
    let mut effective: BTreeMap<String, String> = env::vars_safe().collect();
    effective.extend(toolset_env.iter().map(|(k, v)| (k.clone(), v.clone())));
    for (key, value) in install_env {
        match value.clone().into_string() {
            Some(value) => effective.insert(key.clone(), value),
            None => effective.remove(key),
        };
    }
    let child_homes = RustHomes::from_sources(
        &IndexMap::new(),
        None,
        effective.get("CARGO_HOME").map(PathBuf::from),
        None,
        effective.get("RUSTUP_HOME").map(PathBuf::from),
    );

    // These differ when `install_env` changes or removes a home, or when a settings home
    // never reaches the child, so lock both.
    let mut identities = rust_state_lock_identities(&plugin_homes.rustup, &plugin_homes.cargo);
    identities.extend(rust_state_lock_identities(
        &child_homes.rustup,
        &child_homes.cargo,
    ));
    lock_rust_state_identities(identities).await
}

async fn lock_rust_state(homes: &RustHomes) -> Result<Vec<fslock::LockFile>> {
    lock_rust_state_identities(rust_state_lock_identities(&homes.rustup, &homes.cargo)).await
}

/// Takes the locks in sorted order so overlapping lock sets can't deadlock.
async fn lock_rust_state_identities(mut identities: Vec<PathBuf>) -> Result<Vec<fslock::LockFile>> {
    identities.sort();
    identities.dedup();
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
        Ok(Self::from_config_env(&config.env().await?))
    }

    fn from_config_env(config_env: &IndexMap<String, String>) -> Self {
        let settings = Settings::get();
        Self::from_sources(
            config_env,
            settings.rust.cargo_home.clone(),
            env::var_path("CARGO_HOME"),
            settings.rust.rustup_home.clone(),
            env::var_path("RUSTUP_HOME"),
        )
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

    /// The directories mise puts on PATH for this runtime. An external rustup's proxies often
    /// share a directory with unrelated tools (pacman's `/usr/bin`), and prepending that would
    /// shadow the versions mise manages, so expose only the proxies through a mise-owned dir.
    fn bin_paths(&self, proxies_root: &Path) -> Result<Vec<PathBuf>> {
        if !self.is_external() {
            return Ok(vec![self.bin_dir.clone()]);
        }
        // Falling back to the shared dir on failure would reintroduce the PATH shadowing.
        let dir = link_rustup_proxies(&self.bin_dir, proxies_root)
            .wrap_err("failed to isolate rustup proxies")?;
        Ok(vec![dir])
    }
}

const RUSTUP_PROXY_NAMES: &[&str] = &[
    "rustup",
    "cargo",
    "rustc",
    "rustdoc",
    "rustfmt",
    "cargo-fmt",
    "cargo-clippy",
    "clippy-driver",
    "cargo-miri",
    "rust-analyzer",
    "rust-gdb",
    "rust-gdbgui",
    "rust-lldb",
];

/// Symlinks the rustup proxies found in `external` into a dir under `proxies_root`.
fn link_rustup_proxies(external: &Path, proxies_root: &Path) -> Result<PathBuf> {
    let key = crate::hash::hash_sha256_to_str(&external.to_string_lossy());
    let dir = proxies_root.join(&key[..16]);
    file::create_dir_all(&dir)?;
    for name in RUSTUP_PROXY_NAMES {
        let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let target = std::path::absolute(external)?.join(&name);
        let link = dir.join(&name);
        if !file::is_executable(&target) {
            // Drop a link left over from a proxy the provider no longer ships.
            match std::fs::remove_file(&link) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
                    return Err(err)
                        .wrap_err_with(|| format!("failed to remove {}", link.display()));
                }
                _ => {}
            }
            continue;
        }
        let up_to_date = if cfg!(windows) {
            // Windows gets copies, which are current when the same size and not older than the proxy.
            match (link.metadata(), target.metadata()) {
                (Ok(l), Ok(t)) => {
                    l.len() == t.len()
                        && matches!((l.modified(), t.modified()), (Ok(lm), Ok(tm)) if lm >= tm)
                }
                _ => false,
            }
        } else {
            file::resolve_symlink(&link).ok().flatten().as_deref() == Some(target.as_path())
        };
        if up_to_date {
            continue;
        }
        file::make_symlink_or_copy(&target, &link)?;
    }
    Ok(dir)
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
        .filter(|path| !file::is_mise_dispatch_dir(path))
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

fn normalize_rustup_profile(profile: &str) -> Result<&'static str> {
    match profile {
        "minimal" | "m" => Ok("minimal"),
        "default" | "d" | "" => Ok("default"),
        "complete" | "c" => Ok("complete"),
        _ => bail!(
            "unknown rustup profile name: {profile}; valid profile names are: minimal, default, complete"
        ),
    }
}

/// Components a profile installs that `mise install` restores when they are
/// missing, by naming them to rustup as `--component`.
fn profile_components(profile: &str) -> Vec<String> {
    let components: &[&[&str]] = match profile {
        "minimal" => &[RUST_MINIMAL_PROFILE_COMPONENTS],
        "default" => &[
            RUST_MINIMAL_PROFILE_COMPONENTS,
            RUST_DEFAULT_PROFILE_COMPONENTS,
        ],
        _ => &[],
    };
    components
        .iter()
        .flat_map(|list| list.iter().map(|component| (*component).to_string()))
        .collect()
}

/// The toolchain's host triple, read from its `rustc-<triple>` component;
/// `rustc` only ever exists for the host.
fn rustup_installed_host(installed: &BTreeSet<String>) -> Option<&str> {
    installed.iter().find_map(|item| {
        item.strip_prefix("rustc-")
            .filter(|triple| rustup_component_suffix_is_host_triple(triple))
    })
}

/// With a known host, only the host's component counts, so a cross target's
/// `rust-std-<target>` does not stand in for the host's `rust-std`.
fn rustup_component_installed(
    installed: &BTreeSet<String>,
    component: &str,
    host: Option<&str>,
) -> bool {
    installed.iter().any(|item| {
        item == component
            || item
                .strip_prefix(component)
                .and_then(|suffix| suffix.strip_prefix('-'))
                .is_some_and(|suffix| match host {
                    Some(host) => suffix == host,
                    None => rustup_component_suffix_is_host_triple(suffix),
                })
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
    "riscv64gc",
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
    use confique::Layer;

    #[test]
    fn test_releases_or_channels_only() {
        let fail = || Err(eyre::eyre!("dns error"));
        assert_eq!(
            releases_or_channels_only::<u8>(fail(), true).unwrap(),
            (vec![], true)
        );
        assert!(releases_or_channels_only::<u8>(fail(), false).is_err());
        assert_eq!(
            releases_or_channels_only(Ok(vec![1u8]), true).unwrap(),
            (vec![1], false)
        );
    }

    #[test]
    fn test_nightly_manifest_url_for_mirror() {
        assert_eq!(
            nightly_manifest_url_for(Some("https://m.example.com/")),
            "https://m.example.com/dist/channel-rust-nightly.toml"
        );
        for unset in [None, Some("")] {
            assert_eq!(
                nightly_manifest_url_for(unset),
                "https://static.rust-lang.org/dist/channel-rust-nightly.toml"
            );
        }
    }

    #[test]
    fn test_rustup_mirror_env_forwards_only_mirror_vars() {
        let config_env: IndexMap<String, String> = [
            ("RUSTUP_DIST_SERVER", "https://m.example.com"),
            ("RUSTUP_UPDATE_ROOT", ""),
            ("RUSTUP_HOME", "/elsewhere"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let forwarded: Vec<_> = rustup_mirror_env(&config_env).collect();
        assert_eq!(
            forwarded,
            [("RUSTUP_DIST_SERVER", &"https://m.example.com".to_string())]
        );
        assert_eq!(
            rustup_dist_server(&config_env).as_deref(),
            Some("https://m.example.com")
        );
    }

    #[test]
    fn test_rustup_url_for_mirror() {
        let triple = "x86_64-unknown-linux-gnu";
        assert_eq!(
            rustup_url_for(Some("https://m.example.com/rustup/"), triple),
            format!("https://m.example.com/rustup/dist/{triple}/{RUSTUP_INIT_BIN}")
        );
        assert_eq!(rustup_url_for(Some(""), triple), default_rustup_url(triple));
        assert_eq!(rustup_url_for(None, triple), default_rustup_url(triple));
    }

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
    fn parses_nightly_manifest_date() {
        assert_eq!(
            parse_nightly_manifest("manifest-version = \"2\"\ndate = \"2026-08-13\"\n").unwrap(),
            "nightly-2026-08-13"
        );
    }

    #[test]
    fn channels_are_dated_by_the_build_they_point_at() {
        let date = parse_channel_manifest_date(
            "manifest-version = \"2\"\ndate = \"2026-09-18\"\n",
            "stable",
        )
        .unwrap();
        assert_eq!(channel_created_at(date), "2026-09-18T00:00:00Z");
        assert_eq!(
            dated_nightly_created_at("nightly-2026-10-18").as_deref(),
            Some("2026-10-18T00:00:00Z")
        );
        assert_eq!(dated_nightly_created_at("nightly"), None);
        assert_eq!(dated_nightly_created_at("nightly-2026-13-40"), None);
        assert_eq!(dated_nightly_created_at("1.90.0"), None);
    }

    #[test]
    fn a_cutoff_resolves_stable_to_the_newest_old_enough_release() {
        assert_eq!(cutoff_channel_target("stable").as_deref(), Some("latest"));
        assert_eq!(cutoff_channel_target("beta"), None);
        assert_eq!(cutoff_channel_target("1.90.0"), None);
    }

    #[test]
    fn test_channel_manifest_url_for_each_channel() {
        assert_eq!(
            channel_manifest_url_for(Some("https://m.example.com/"), "stable"),
            "https://m.example.com/dist/channel-rust-stable.toml"
        );
        assert_eq!(
            channel_manifest_url_for(None, "beta"),
            "https://static.rust-lang.org/dist/channel-rust-beta.toml"
        );
    }

    #[test]
    fn rejects_missing_or_invalid_nightly_manifest_date() {
        assert!(parse_nightly_manifest("manifest-version = \"2\"\n").is_err());
        assert!(parse_nightly_manifest("date = \"2026-13-40\"\n").is_err());
        assert!(parse_nightly_manifest("not toml").is_err());
    }

    #[test]
    fn recognizes_only_valid_dated_nightlies() {
        assert!(is_dated_nightly("nightly-2026-08-13"));
        assert!(!is_dated_nightly("nightly"));
        assert!(!is_dated_nightly("nightly-2026-13-40"));
        assert!(!is_dated_nightly("1.90.0"));
    }

    #[test]
    fn selects_latest_installed_dated_nightly() {
        let versions = vec![
            "nightly-2026-08-11".to_string(),
            "nightly".to_string(),
            "1.90.0".to_string(),
            "nightly-2026-08-13".to_string(),
        ];
        assert_eq!(
            latest_installed_nightly(versions.into_iter()).as_deref(),
            Some("nightly-2026-08-13")
        );
        assert_eq!(
            latest_installed_nightly(["nightly".to_string()].into_iter()),
            None
        );
    }

    #[test]
    fn parses_rustup_check_update_before_1_29() {
        let out = "\
stable-aarch64-apple-darwin - Update available : 1.98.1 (48a229cea 2026-09-01) -> 1.99.0 (b940084d7 2026-09-28)
rustup - Up to date : 1.28.2
";
        assert_eq!(
            rustup_check_update(out, "stable-aarch64-apple-darwin"),
            Some((
                "1.98.1 (48a229cea 2026-09-01)",
                "1.99.0 (b940084d7 2026-09-28)"
            ))
        );
    }

    #[test]
    fn parses_rustup_check_update_since_1_29() {
        let out = "\
stable-aarch64-apple-darwin - update available: 1.97.1 (8bab26f4f 2026-07-14) -> 1.99.0 (b940084d7 2026-09-28)
rustup - update available : 1.29.0 -> 1.29.1
";
        assert_eq!(
            rustup_check_update(out, "stable-aarch64-apple-darwin"),
            Some((
                "1.97.1 (8bab26f4f 2026-07-14)",
                "1.99.0 (b940084d7 2026-09-28)"
            ))
        );
    }

    #[test]
    fn rustup_check_update_reads_only_the_selected_toolchain() {
        let out = "\
stable-aarch64-apple-darwin - up to date: 1.99.0 (b940084d7 2026-09-28)
nightly-aarch64-apple-darwin - update available: 1.100.0-nightly (fb6531d55 2026-08-23) -> 1.101.0-nightly (c36f14571 2026-10-01)
rustup - update available : 1.29.0 -> 1.29.1
";
        assert_eq!(
            rustup_check_update(out, "stable-aarch64-apple-darwin"),
            None
        );
        assert_eq!(
            rustup_check_update(out, "nightly-aarch64-apple-darwin"),
            Some((
                "1.100.0-nightly (fb6531d55 2026-08-23)",
                "1.101.0-nightly (c36f14571 2026-10-01)"
            ))
        );
    }

    #[test]
    fn rustup_check_update_is_none_when_up_to_date() {
        let out = "\
stable-aarch64-apple-darwin - up to date: 1.99.0 (b940084d7 2026-09-28)
rustup - up to date : 1.29.1
";
        assert_eq!(
            rustup_check_update(out, "stable-aarch64-apple-darwin"),
            None
        );
    }

    #[test]
    fn rustup_check_names_follow_the_configured_host() {
        assert_eq!(
            rustup_check_toolchain_names("stable", Some("x86_64-unknown-linux-musl")),
            vec![
                "stable-x86_64-unknown-linux-musl".to_string(),
                format!("stable-{TARGET}")
            ]
        );
        assert_eq!(
            rustup_check_toolchain_names("stable", None),
            vec![format!("stable-{TARGET}")]
        );
        assert_eq!(
            rustup_check_toolchain_names("stable", Some(TARGET)),
            vec![format!("stable-{TARGET}")]
        );
        // A host-qualified request already is the name rustup lists.
        assert_eq!(
            rustup_check_toolchain_names("stable-x86_64-pc-windows-gnu", Some(TARGET)),
            vec!["stable-x86_64-pc-windows-gnu".to_string()]
        );
    }

    #[test]
    fn rustup_check_finds_a_sole_toolchain_under_any_host() {
        let out = "\
stable-riscv64gc-unknown-linux-gnu - update available: 1.98.1 (aaaa 2026-09-01) -> 1.99.0 (bbbb 2026-09-28)
nightly-riscv64gc-unknown-linux-gnu - up to date : 1.100.0-nightly (cccc 2026-08-23)
rustup - up to date : 1.29.1
";
        assert_eq!(
            rustup_check_sole_host_toolchain(out, "stable").as_deref(),
            Some("stable-riscv64gc-unknown-linux-gnu")
        );
        // Two hosts for one channel are ambiguous, and non-hosts never match.
        let two = format!(
            "{out}stable-x86_64-unknown-linux-gnu - up to date : 1.99.0 (bbbb 2026-09-28)\n"
        );
        assert_eq!(rustup_check_sole_host_toolchain(&two, "stable"), None);
        assert_eq!(rustup_check_sole_host_toolchain(out, "beta"), None);
        assert_eq!(rustup_check_sole_host_toolchain(out, "rustup"), None);
    }

    #[test]
    fn moving_channels_are_stable_and_beta_only() {
        assert!(is_moving_channel("stable"));
        assert!(is_moving_channel("beta"));
        assert!(is_moving_channel("stable-aarch64-apple-darwin"));
        assert!(is_moving_channel("nightly-aarch64-apple-darwin"));
        assert!(!is_moving_channel("nightly"));
        assert!(!is_moving_channel("nightly-2026-10-01"));
        assert!(!is_moving_channel("1.99.0"));
        assert!(!is_moving_channel("stable-2026"));
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

        assert!(rustup_component_installed(&installed, "rust-src", None));
        assert!(rustup_component_installed(&installed, "llvm-tools", None));
        assert!(!rustup_component_installed(&installed, "rustfmt", None));
        assert!(!rustup_component_installed(&installed, "rust", None));
        assert!(!rustup_component_installed(&installed, "llvm", None));
    }

    #[test]
    fn rustup_component_matching_uses_the_toolchains_host() {
        let installed = BTreeSet::from([
            "cargo-x86_64-unknown-linux-gnu".to_string(),
            "rustc-x86_64-unknown-linux-gnu".to_string(),
            "rust-std-wasm32-unknown-unknown".to_string(),
        ]);
        let host = rustup_installed_host(&installed);

        assert_eq!(host, Some("x86_64-unknown-linux-gnu"));
        assert!(rustup_component_installed(&installed, "cargo", host));
        // a cross target's standard library is not the host's
        assert!(!rustup_component_installed(&installed, "rust-std", host));
        assert!(rustup_component_installed(&installed, "rust-std", None));
    }

    #[test]
    fn profiles_require_their_components() {
        assert_eq!(
            profile_components("minimal"),
            vec!["cargo", "rust-std", "rustc"]
        );
        assert_eq!(
            profile_components("default"),
            vec![
                "cargo",
                "rust-std",
                "rustc",
                "clippy",
                "rust-docs",
                "rustfmt"
            ]
        );
        assert!(profile_components("complete").is_empty());
    }

    #[test]
    fn profile_aliases_match_rustup() {
        assert_eq!(normalize_rustup_profile("minimal").unwrap(), "minimal");
        assert_eq!(normalize_rustup_profile("m").unwrap(), "minimal");
        assert_eq!(normalize_rustup_profile("default").unwrap(), "default");
        assert_eq!(normalize_rustup_profile("d").unwrap(), "default");
        assert_eq!(normalize_rustup_profile("").unwrap(), "default");
        assert_eq!(normalize_rustup_profile("complete").unwrap(), "complete");
        assert_eq!(normalize_rustup_profile("c").unwrap(), "complete");
        assert!(normalize_rustup_profile("custom").is_err());
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
    fn rust_runtime_external_bin_paths_expose_only_proxies() {
        let root = tempfile::tempdir().unwrap();
        let homes = rust_homes_at(root.path(), false);
        let external = root.path().join("external/bin");
        create_proxy_dir(&external, &[RUSTUP_BIN, CARGO_BIN, RUSTC_BIN, "node"]);
        let runtime =
            RustRuntime::from_paths_with(homes.clone(), std::slice::from_ref(&external), |_| true);
        let proxies = root.path().join("proxies");

        let paths = runtime.bin_paths(&proxies).unwrap();

        assert_eq!(paths.len(), 1);
        assert_ne!(paths[0], external);
        assert!(paths[0].join(CARGO_BIN).exists());
        assert!(paths[0].join(RUSTC_BIN).exists());
        assert!(!paths[0].join("node").exists());
        assert_eq!(runtime.bin_paths(&proxies).unwrap(), paths);

        let managed = RustRuntime::from_paths_with(homes.clone(), &[], |_| true);
        assert_eq!(
            managed.bin_paths(&proxies).unwrap(),
            vec![homes.cargo_bindir()]
        );
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

    /// The lock `lock_rust_state_for_config` takes must be the same lock the rust
    /// plugin resolves for the same environment, or `cargo install` fallbacks can
    /// still race the plugin on the shared rustup state (#14041/#14042).
    fn assert_plugin_locks_are_covered(homes: &RustHomes) {
        let identities = rust_state_lock_identities(&homes.rustup, &homes.cargo);
        for identity in identities {
            let probe = LockFile::new(&identity).try_lock().unwrap();
            assert!(
                probe.is_none(),
                "cargo fallback lock does not cover plugin-resolved home {}",
                file::display_path(&identity)
            );
        }
    }

    fn ambient_homes_guard() -> crate::test::EnvVarGuard {
        let mut guard = crate::test::EnvVarGuard::new();
        guard.remove("CARGO_HOME");
        guard.remove("RUSTUP_HOME");
        guard
    }

    #[tokio::test]
    async fn config_lock_resolves_tilde_homes_like_the_rust_plugin() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let _env_guard = ambient_homes_guard();
        let cargo_home_dir = tempfile::Builder::new()
            .prefix("mise-lock-probe-cargo-")
            .tempdir_in(*dirs::HOME)
            .unwrap();
        let rustup_home_dir = tempfile::Builder::new()
            .prefix("mise-lock-probe-rustup-")
            .tempdir_in(*dirs::HOME)
            .unwrap();
        let cargo_home = format!(
            "~/{}",
            cargo_home_dir.path().file_name().unwrap().to_string_lossy()
        );
        let rustup_home = format!(
            "~/{}",
            rustup_home_dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
        let mut toolset_env = BTreeMap::new();
        toolset_env.insert("CARGO_HOME".to_string(), cargo_home.clone());
        toolset_env.insert("RUSTUP_HOME".to_string(), rustup_home.clone());

        // `[env]` values reach both `config.env()` and the toolset env
        let config_env = toolset_env.clone().into_iter().collect();
        let _locks = lock_rust_state_for_config(&config_env, &toolset_env, &IndexMap::new())
            .await
            .unwrap();

        let plugin_homes = RustHomes::from_sources(
            &IndexMap::new(),
            None,
            Some(file::replace_path(&cargo_home)),
            None,
            Some(file::replace_path(&rustup_home)),
        );
        assert_plugin_locks_are_covered(&plugin_homes);
    }

    #[tokio::test]
    async fn config_lock_honors_mise_prefixed_homes_from_config_env() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let _env_guard = ambient_homes_guard();
        let root = tempfile::tempdir().unwrap();
        let cargo_home = root.path().join("cargo");
        let rustup_home = root.path().join("rustup");
        let mut toolset_env = BTreeMap::new();
        toolset_env.insert(
            "MISE_CARGO_HOME".to_string(),
            cargo_home.to_string_lossy().to_string(),
        );
        toolset_env.insert(
            "MISE_RUSTUP_HOME".to_string(),
            rustup_home.to_string_lossy().to_string(),
        );

        // `[env]` values reach both `config.env()` and the toolset env
        let config_env = toolset_env.clone().into_iter().collect();
        let _locks = lock_rust_state_for_config(&config_env, &toolset_env, &IndexMap::new())
            .await
            .unwrap();

        let plugin_homes = RustHomes::from_sources(&config_env, None, None, None, None);
        assert_plugin_locks_are_covered(&plugin_homes);
    }

    #[tokio::test]
    async fn config_lock_covers_settings_configured_homes() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let _env_guard = ambient_homes_guard();
        let root = tempfile::tempdir().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.rust.cargo_home = Some(root.path().join("cargo"));
        settings.rust.rustup_home = Some(root.path().join("rustup"));
        Settings::reset(Some(settings));

        let _locks =
            lock_rust_state_for_config(&IndexMap::new(), &BTreeMap::new(), &IndexMap::new())
                .await
                .unwrap();

        let plugin_homes = RustHomes::from_sources(
            &IndexMap::new(),
            Some(root.path().join("cargo")),
            None,
            Some(root.path().join("rustup")),
            None,
        );
        assert_plugin_locks_are_covered(&plugin_homes);
    }

    #[tokio::test]
    async fn config_lock_ignores_ambient_home_removed_by_install_env() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let mut env_guard = ambient_homes_guard();
        let root = tempfile::tempdir().unwrap();
        env_guard.set("CARGO_HOME", root.path().join("ambient-cargo"));
        let install_env = IndexMap::from([(
            "CARGO_HOME".to_string(),
            crate::config::env_directive::EnvValue::Boolean(false),
        )]);

        let _locks = lock_rust_state_for_config(&IndexMap::new(), &BTreeMap::new(), &install_env)
            .await
            .unwrap();

        // the child `cargo` no longer sees CARGO_HOME, so it uses the default home
        let child_homes = RustHomes::from_sources(&IndexMap::new(), None, None, None, None);
        assert_plugin_locks_are_covered(&child_homes);
    }

    #[tokio::test]
    async fn config_lock_covers_settings_home_when_another_tool_exports_cargo_home() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let _env_guard = ambient_homes_guard();
        let root = tempfile::tempdir().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.rust.cargo_home = Some(root.path().join("settings-cargo"));
        Settings::reset(Some(settings));
        // another tool's exec env reaches the toolset env, but not `config.env()`
        let toolset_env = BTreeMap::from([(
            "CARGO_HOME".to_string(),
            root.path().join("tool-cargo").to_string_lossy().to_string(),
        )]);

        let _locks = lock_rust_state_for_config(&IndexMap::new(), &toolset_env, &IndexMap::new())
            .await
            .unwrap();

        // the rust plugin resolves the settings home
        let plugin_homes = RustHomes::from_sources(
            &IndexMap::new(),
            Some(root.path().join("settings-cargo")),
            None,
            None,
            None,
        );
        assert_plugin_locks_are_covered(&plugin_homes);
        // while the child `cargo` uses the exported one
        let child_homes = RustHomes::from_sources(
            &IndexMap::new(),
            None,
            Some(root.path().join("tool-cargo")),
            None,
            None,
        );
        assert_plugin_locks_are_covered(&child_homes);
    }

    #[tokio::test]
    async fn config_lock_covers_settings_home_and_child_default_when_install_env_removes_it() {
        let _settings_guard = crate::test::SettingsGuard::lock();
        let _env_guard = ambient_homes_guard();
        let root = tempfile::tempdir().unwrap();
        let mut settings = crate::config::settings::SettingsPartial::empty();
        settings.rust.cargo_home = Some(root.path().join("cargo"));
        Settings::reset(Some(settings));
        // the rust plugin's exec_env exports the settings home to the toolset env
        let toolset_env = BTreeMap::from([(
            "CARGO_HOME".to_string(),
            root.path().join("cargo").to_string_lossy().to_string(),
        )]);
        let install_env = IndexMap::from([(
            "CARGO_HOME".to_string(),
            crate::config::env_directive::EnvValue::Boolean(false),
        )]);

        let _locks = lock_rust_state_for_config(&IndexMap::new(), &toolset_env, &install_env)
            .await
            .unwrap();

        // the plugin still locks the settings home
        let plugin_homes = RustHomes::from_sources(
            &IndexMap::new(),
            Some(root.path().join("cargo")),
            None,
            None,
            None,
        );
        assert_plugin_locks_are_covered(&plugin_homes);
        // while the child `cargo` falls back to the default home
        let child_homes = RustHomes::from_sources(&IndexMap::new(), None, None, None, None);
        assert_plugin_locks_are_covered(&child_homes);
    }
}
