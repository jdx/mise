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

#[derive(Debug)]
pub(super) struct RustPlugin {
    ba: Arc<BackendArg>,
}

const RUST_NIGHTLY_MANIFEST_URL: &str =
    "https://static.rust-lang.org/dist/channel-rust-nightly.toml";
const RUST_DIST_ROOT: &str = "https://static.rust-lang.org/dist";
const RUST_MINIMAL_PROFILE_COMPONENTS: &[&str] = &["cargo", "rust-std", "rustc"];
const RUST_DEFAULT_PROFILE_COMPONENTS: &[&str] = &["clippy", "rust-docs", "rustfmt"];

#[derive(Debug, serde::Deserialize)]
struct RustupManifest {
    #[serde(default)]
    profiles: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    renames: BTreeMap<String, RustupManifestRename>,
    pkg: RustupManifestPackages,
}

#[derive(Debug, serde::Deserialize)]
struct RustupManifestRename {
    to: String,
}

#[derive(Debug, serde::Deserialize)]
struct RustupManifestPackages {
    rust: RustupManifestPackage,
}

#[derive(Debug, serde::Deserialize)]
struct RustupManifestPackage {
    target: BTreeMap<String, RustupManifestTarget>,
}

#[derive(Debug, serde::Deserialize)]
struct RustupManifestTarget {
    #[serde(default)]
    components: Vec<RustupManifestComponent>,
    #[serde(default)]
    extensions: Vec<RustupManifestComponent>,
}

#[derive(Debug, serde::Deserialize)]
struct RustupManifestComponent {
    pkg: String,
    target: Option<String>,
}

#[derive(Debug, PartialEq)]
struct RustupProfileComponents {
    components: Vec<String>,
    host: String,
}

#[derive(Debug)]
struct InstalledRustupManifest {
    toolchain: String,
    contents: Option<String>,
}

fn parse_nightly_manifest(manifest: &str) -> Result<String> {
    let manifest: toml::Value =
        toml::from_str(manifest).wrap_err("failed to parse the Rust nightly channel manifest")?;
    let date = manifest
        .get("date")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| eyre::eyre!("Rust nightly channel manifest is missing its date"))?;
    date.parse::<jiff::civil::Date>()
        .wrap_err_with(|| format!("invalid Rust nightly channel manifest date: {date}"))?;
    Ok(format!("nightly-{date}"))
}

async fn current_nightly_version() -> Result<String> {
    let manifest = HTTP_FETCH
        .get_text_cached(RUST_NIGHTLY_MANIFEST_URL)
        .await
        .wrap_err("failed to fetch the Rust nightly channel manifest")?;
    parse_nightly_manifest(&manifest)
}

fn is_dated_nightly(version: &str) -> bool {
    version
        .strip_prefix("nightly-")
        .is_some_and(|date| date.parse::<jiff::civil::Date>().is_ok())
}

fn latest_installed_nightly(versions: impl DoubleEndedIterator<Item = String>) -> Option<String> {
    versions.rev().find(|version| is_dated_nightly(version))
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

    fn rustup_active_toolchain(
        &self,
        tv: &ToolVersion,
        runtime: &RustRuntime,
    ) -> Result<Option<String>> {
        let args = vec!["show".to_string(), "active-toolchain".to_string()];
        let mut cmd = cmd(runtime.bin_dir.join(RUSTUP_BIN), args)
            .env("PATH", rustup_path_env(runtime)?)
            .stdout_capture()
            .stderr_capture()
            .unchecked();
        for (key, value) in rustup_env(&runtime.homes, &tv.version) {
            cmd = cmd.env(key, value);
        }
        let output = match cmd.run() {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                debug!(
                    "rustup show active-toolchain failed for {}: {}",
                    tv.style(),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                return Ok(None);
            }
            Err(err) => {
                debug!(
                    "rustup show active-toolchain failed for {}: {err:#}",
                    tv.style()
                );
                return Ok(None);
            }
        };
        let output = String::from_utf8_lossy(&output.stdout);
        let Some(toolchain) = output.split_whitespace().next() else {
            debug!(
                "rustup show active-toolchain returned no toolchain for {}",
                tv.style()
            );
            return Ok(None);
        };
        Ok(Some(toolchain.to_string()))
    }

    fn rustup_toolchain_manifest(
        &self,
        tv: &ToolVersion,
        runtime: &RustRuntime,
    ) -> Result<Option<InstalledRustupManifest>> {
        let Some(toolchain) = self.rustup_active_toolchain(tv, runtime)? else {
            return Ok(None);
        };
        let manifest = runtime
            .homes
            .rustup
            .join("toolchains")
            .join(&toolchain)
            .join("lib")
            .join("rustlib")
            .join("multirust-channel-manifest.toml");
        if !manifest.is_file() {
            debug!(
                "rustup manifest missing for {} at {}",
                tv.style(),
                manifest.display()
            );
            return Ok(Some(InstalledRustupManifest {
                toolchain,
                contents: None,
            }));
        }
        let contents = match file::read_to_string(&manifest) {
            Ok(contents) => Some(contents),
            Err(err) => {
                debug!(
                    "failed to read rustup manifest for {} at {}: {err:#}",
                    tv.style(),
                    manifest.display()
                );
                None
            }
        };
        Ok(Some(InstalledRustupManifest {
            toolchain,
            contents,
        }))
    }

    async fn rustup_complete_profile_components(
        &self,
        tv: &ToolVersion,
        runtime: &RustRuntime,
    ) -> Result<Option<RustupProfileComponents>> {
        let Some(installed) = self.rustup_toolchain_manifest(tv, runtime)? else {
            if self
                .rustup_installed_items(tv, "component", runtime)?
                .is_some()
            {
                bail!(
                    "cannot reconcile the rustup complete profile for {} because its active toolchain could not be determined",
                    tv.style()
                );
            }
            return Ok(None);
        };
        if let Some(manifest) = installed.contents {
            match parse_rustup_profile_components(&manifest, &installed.toolchain, "complete") {
                Ok(components) => return Ok(Some(components)),
                Err(err) => debug!(
                    "failed to resolve the rustup complete profile for {} from its installed manifest: {err:#}",
                    tv.style()
                ),
            }
        }
        let url = rustup_channel_manifest_url(
            &tv.version,
            rustup_dist_var(tv, "RUSTUP_DIST_SERVER"),
            rustup_dist_var(tv, "RUSTUP_DIST_ROOT"),
        );
        let manifest = read_rustup_channel_manifest(&url).await.wrap_err_with(|| {
            format!(
                "cannot reconcile the rustup complete profile for {} because its installed manifest is unusable and {url} could not be fetched",
                tv.style()
            )
        })?;
        parse_rustup_profile_components(&manifest, &installed.toolchain, "complete")
            .map(Some)
            .wrap_err_with(|| {
                format!(
                    "cannot reconcile the rustup complete profile for {} from {url}",
                    tv.style()
                )
            })
    }

    fn missing_components(
        &self,
        requested: &[String],
        installed: &BTreeSet<String>,
        host: Option<&str>,
    ) -> Vec<String> {
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
        let active_toolchain = if effective_profile == "complete" {
            None
        } else {
            self.rustup_active_toolchain(tv, &runtime)?
        };
        let host = active_toolchain.as_deref().and_then(rustup_toolchain_host);

        // Query components even when none were explicitly requested. This
        // verifies that rustup still has the toolchain represented by mise's
        // install symlink after restoring only mise's data directory.
        let Some(installed_components) = self.rustup_installed_items(tv, "component", &runtime)?
        else {
            return Ok(false);
        };

        let mut required_components = components.unwrap_or_default();
        let manifest_host = if effective_profile == "complete" {
            let Some(profile_components) = self
                .rustup_complete_profile_components(tv, &runtime)
                .await?
            else {
                return Ok(false);
            };
            required_components.extend(profile_components.components);
            Some(profile_components.host)
        } else {
            required_components.extend(fallback_rustup_profile_components(effective_profile, host));
            None
        };
        let host = manifest_host.as_deref().or(host);
        required_components.sort();
        required_components.dedup();

        if !required_components.is_empty() {
            let missing =
                self.missing_components(&required_components, &installed_components, host);
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
        let mut versions: Vec<VersionInfo> = github::list_releases("rust-lang/rust")
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
            .collect();
        if let Ok(current_nightly) = current_nightly_version().await {
            versions.push(VersionInfo {
                version: current_nightly,
                ..Default::default()
            });
        }
        versions.extend([
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
        ]);
        Ok(versions)
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
        _config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        if !self.is_rolling_channel(version) {
            return Ok(None);
        }
        current_nightly_version().await.map(Some)
    }

    fn requires_concrete_channel_version(&self, version: &str) -> bool {
        self.is_rolling_channel(version)
    }

    fn is_exact_version(&self, version: &str) -> bool {
        is_dated_nightly(version)
    }

    async fn resolve_exact_version(
        &self,
        _config: &Arc<Config>,
        version: &str,
    ) -> Result<Option<String>> {
        Ok(is_dated_nightly(version).then(|| version.to_string()))
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
        let active_toolchain = if effective_profile == "complete" {
            None
        } else {
            self.rustup_active_toolchain(&tv, &runtime)?
        };
        let host = active_toolchain.as_deref().and_then(rustup_toolchain_host);
        let mut components = components.unwrap_or_default();
        if effective_profile == "complete" {
            if let Some(profile_components) = self
                .rustup_complete_profile_components(&tv, &runtime)
                .await?
            {
                components.extend(profile_components.components);
            }
        } else {
            components.extend(fallback_rustup_profile_components(effective_profile, host));
        }
        components.sort();
        components.dedup();

        let mut cmd = CmdLineRunner::new(runtime.bin_dir.join(RUSTUP_BIN))
            .with_pr(ctx.pr.as_ref())
            .arg("toolchain")
            .arg("install")
            .arg(&tv.version)
            .opt_args("--component", Some(components))
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
        let requested = tv.request.version();
        if requested == "nightly" {
            if Settings::get().offline() || opts.offline {
                let oi = OutdatedInfo::new(config, tv.clone(), tv.version.clone())?;
                return Ok((oi.current.as_ref() != Some(&tv.version)).then_some(oi));
            }
            let latest = current_nightly_version().await?;
            let oi = OutdatedInfo::new(config, tv.clone(), latest.clone())?;
            return Ok((oi.current.as_ref() != Some(&latest)).then_some(oi));
        }
        if is_dated_nightly(&requested) {
            let latest = if bump && !Settings::get().offline() && !opts.offline {
                current_nightly_version().await?
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
            }
            return Ok((oi.current.as_ref() != Some(&latest)).then_some(oi));
        }
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

fn rustup_dist_var(tv: &ToolVersion, key: &str) -> Option<String> {
    match tv.install_env().shift_remove(key) {
        Some(value) => value.into_string(),
        None => env::var(key).ok(),
    }
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn rustup_channel_manifest_url(
    version: &str,
    dist_server: Option<String>,
    legacy_dist_root: Option<String>,
) -> String {
    let dist_root = if let Some(server) = dist_server {
        format!("{}/dist", server.trim_end_matches('/'))
    } else if let Some(root) = legacy_dist_root {
        let root = root.trim_end_matches('/');
        format!("{}/dist", root.strip_suffix("/dist").unwrap_or(root))
    } else {
        RUST_DIST_ROOT.to_string()
    };
    if let Some(date) = version
        .strip_prefix("nightly-")
        .filter(|date| date.parse::<jiff::civil::Date>().is_ok())
    {
        format!("{dist_root}/{date}/channel-rust-nightly.toml")
    } else {
        format!("{dist_root}/channel-rust-{version}.toml")
    }
}

async fn read_rustup_channel_manifest(url: &str) -> Result<String> {
    if let Ok(url) = url::Url::parse(url)
        && url.scheme() == "file"
    {
        let path = url
            .to_file_path()
            .map_err(|_| eyre::eyre!("invalid rustup file URL: {url}"))?;
        return file::read_to_string(&path).wrap_err_with(|| {
            format!("failed to read Rust channel manifest at {}", path.display())
        });
    }
    HTTP_FETCH.get_text_cached(url).await
}

fn fallback_rustup_profile_components(profile: &str, host: Option<&str>) -> Vec<String> {
    let mut components = match profile {
        "minimal" | "default" => RUST_MINIMAL_PROFILE_COMPONENTS
            .iter()
            .map(|component| (*component).to_string())
            .collect::<Vec<_>>(),
        "complete" => return Vec::new(),
        _ => unreachable!("profile was normalized"),
    };
    if profile == "default" {
        components.extend(
            RUST_DEFAULT_PROFILE_COMPONENTS
                .iter()
                .map(|component| (*component).to_string()),
        );
    }
    if profile != "complete" && host.is_some_and(|host| host.ends_with("-pc-windows-gnu")) {
        components.push("rust-mingw".to_string());
    }
    components
}

fn rustup_toolchain_host(toolchain: &str) -> Option<&str> {
    if rustup_component_suffix_is_host_triple(toolchain) {
        return Some(toolchain);
    }
    toolchain.match_indices('-').find_map(|(index, _)| {
        let suffix = &toolchain[index + 1..];
        rustup_component_suffix_is_host_triple(suffix).then_some(suffix)
    })
}

fn parse_rustup_profile_components(
    manifest: &str,
    toolchain: &str,
    profile: &str,
) -> Result<RustupProfileComponents> {
    let manifest: RustupManifest =
        toml::from_str(manifest).wrap_err("failed to parse the installed rustup manifest")?;
    let host = manifest
        .pkg
        .rust
        .target
        .keys()
        .filter(|target| {
            target.as_str() != "*"
                && (toolchain == target.as_str()
                    || toolchain
                        .strip_suffix(target.as_str())
                        .is_some_and(|prefix| prefix.ends_with('-')))
        })
        .max_by_key(|target| target.len())
        .cloned()
        .ok_or_else(|| eyre::eyre!("unable to determine the host for toolchain {toolchain}"))?;
    let target = manifest
        .pkg
        .rust
        .target
        .get(&host)
        .ok_or_else(|| eyre::eyre!("rustup manifest is missing host target {host}"))?;
    let all_components = || target.components.iter().chain(&target.extensions);
    let selected: Vec<&RustupManifestComponent> = if manifest.profiles.is_empty() {
        target.components.iter().collect()
    } else {
        manifest
            .profiles
            .get(profile)
            .ok_or_else(|| eyre::eyre!("rustup manifest is missing the {profile} profile"))?
            .iter()
            .filter_map(|name| {
                all_components().find(|component| {
                    component.pkg == *name
                        && component
                            .target
                            .as_deref()
                            .is_none_or(|target| target == "*" || target == host)
                })
            })
            .collect()
    };
    let mut components = selected
        .into_iter()
        .map(|component| {
            manifest
                .renames
                .iter()
                .rev()
                .find_map(|(name, rename)| (rename.to == component.pkg).then(|| name.clone()))
                .unwrap_or_else(|| component.pkg.clone())
        })
        .collect::<Vec<_>>();
    components.sort();
    components.dedup();
    Ok(RustupProfileComponents { components, host })
}

fn rustup_component_installed(
    installed: &BTreeSet<String>,
    component: &str,
    host: Option<&str>,
) -> bool {
    installed.iter().any(|item| {
        item == component
            || host.is_some_and(|host| {
                item.strip_prefix(component)
                    .and_then(|suffix| suffix.strip_prefix('-'))
                    == Some(host)
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
    "riscv64a23",
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
    fn rustup_component_matching_requires_the_selected_host() {
        let mut installed = BTreeSet::from([
            "rust-src".to_string(),
            "llvm-tools-x86_64-unknown-linux-gnu".to_string(),
            "rust-std-wasm32-unknown-unknown".to_string(),
            "rustc-x86_64-unknown-linux-gnu".to_string(),
        ]);
        let host = rustup_toolchain_host("1.81.0-x86_64-unknown-linux-gnu");

        assert_eq!(host, Some("x86_64-unknown-linux-gnu"));
        assert!(rustup_component_installed(&installed, "rust-src", host));
        assert!(rustup_component_installed(&installed, "llvm-tools", host));
        assert!(!rustup_component_installed(&installed, "rust-std", host));
        assert!(!rustup_component_installed(&installed, "rustfmt", host));
        installed.insert("rust-std-x86_64-unknown-linux-gnu".to_string());
        assert!(rustup_component_installed(&installed, "rust-std", host));
    }

    #[test]
    fn rustup_required_components_recognize_riscv_hosts() {
        let plugin = RustPlugin::new();
        for host in [
            "riscv64gc-unknown-linux-gnu",
            "riscv64a23-unknown-linux-gnu",
        ] {
            let toolchain = format!("nightly-2026-09-01-{host}");
            let selected_host = rustup_toolchain_host(&toolchain);
            assert_eq!(selected_host, Some(host));
            let required = fallback_rustup_profile_components("minimal", selected_host);
            let installed = required
                .iter()
                .map(|component| format!("{component}-{host}"))
                .collect();
            assert!(
                plugin
                    .missing_components(&required, &installed, selected_host)
                    .is_empty()
            );
        }
    }

    #[test]
    fn required_profile_components_match_rustup() {
        assert_eq!(
            fallback_rustup_profile_components("minimal", Some("x86_64-unknown-linux-gnu")),
            ["cargo", "rust-std", "rustc"]
        );
        assert_eq!(
            fallback_rustup_profile_components("default", Some("x86_64-unknown-linux-gnu")),
            [
                "cargo",
                "rust-std",
                "rustc",
                "clippy",
                "rust-docs",
                "rustfmt"
            ]
        );
        assert_eq!(
            fallback_rustup_profile_components("minimal", Some("x86_64-pc-windows-gnu")),
            ["cargo", "rust-std", "rustc", "rust-mingw"]
        );
        assert!(fallback_rustup_profile_components("complete", None).is_empty());
        assert!(
            fallback_rustup_profile_components("complete", Some("x86_64-pc-windows-gnu"))
                .is_empty()
        );
    }

    #[test]
    fn parses_profile_components_for_the_selected_host() {
        let manifest = r#"
[renames.a-clippy]
to = "clippy-preview"

[renames.clippy]
to = "clippy-preview"

[renames.rust-analyzer]
to = "rust-analyzer-preview"

[profiles]
minimal = ["rustc", "cargo", "rust-std", "rust-mingw"]
default = ["rustc", "cargo", "rust-std", "rust-mingw", "clippy-preview"]
complete = ["rustc", "rust-mingw", "clippy-preview", "rust-analyzer-preview", "rust-src"]

[pkg.rust.target.x86_64-unknown-linux-gnu]

[[pkg.rust.target.x86_64-unknown-linux-gnu.components]]
pkg = "rustc"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.components]]
pkg = "cargo"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.components]]
pkg = "rust-std"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.extensions]]
pkg = "rust-std"
target = "wasm32-unknown-unknown"

[[pkg.rust.target.x86_64-unknown-linux-gnu.extensions]]
pkg = "clippy-preview"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.extensions]]
pkg = "rust-analyzer-preview"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.extensions]]
pkg = "rust-src"
target = "*"
"#;

        assert_eq!(
            parse_rustup_profile_components(
                manifest,
                "1.81.0-x86_64-unknown-linux-gnu",
                "complete"
            )
            .unwrap(),
            RustupProfileComponents {
                components: vec![
                    "clippy".to_string(),
                    "rust-analyzer".to_string(),
                    "rust-src".to_string(),
                    "rustc".to_string()
                ],
                host: "x86_64-unknown-linux-gnu".to_string()
            }
        );
    }

    #[test]
    fn profileless_manifests_use_legacy_components() {
        let manifest = r#"
[pkg.rust.target.x86_64-unknown-linux-gnu]

[[pkg.rust.target.x86_64-unknown-linux-gnu.components]]
pkg = "rustc"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.components]]
pkg = "cargo"
target = "x86_64-unknown-linux-gnu"

[[pkg.rust.target.x86_64-unknown-linux-gnu.extensions]]
pkg = "clippy-preview"
target = "x86_64-unknown-linux-gnu"
"#;

        assert_eq!(
            parse_rustup_profile_components(
                manifest,
                "1.19.0-x86_64-unknown-linux-gnu",
                "complete"
            )
            .unwrap()
            .components,
            vec!["cargo", "rustc"]
        );
    }

    #[test]
    fn builds_rustup_channel_manifest_urls() {
        assert_eq!(
            rustup_channel_manifest_url("1.81.0", None, None),
            "https://static.rust-lang.org/dist/channel-rust-1.81.0.toml"
        );
        assert_eq!(
            rustup_channel_manifest_url("nightly-2026-08-12", None, None),
            "https://static.rust-lang.org/dist/2026-08-12/channel-rust-nightly.toml"
        );
        assert_eq!(
            rustup_channel_manifest_url(
                "1.81.0",
                Some("https://mirror.example.com/".to_string()),
                Some("https://ignored.example.com/dist".to_string())
            ),
            "https://mirror.example.com/dist/channel-rust-1.81.0.toml"
        );
        assert_eq!(
            rustup_channel_manifest_url(
                "1.81.0",
                None,
                Some("https://legacy.example.com/dist".to_string())
            ),
            "https://legacy.example.com/dist/channel-rust-1.81.0.toml"
        );
    }

    #[tokio::test]
    async fn reads_rustup_channel_manifest_from_file_server() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("channel-rust-1.81.0.toml");
        std::fs::write(&path, "manifest-version = '2'").unwrap();
        let url = url::Url::from_file_path(path).unwrap();

        assert_eq!(
            read_rustup_channel_manifest(url.as_str()).await.unwrap(),
            "manifest-version = '2'"
        );
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
