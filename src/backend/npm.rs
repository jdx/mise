use crate::Result;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::backend::platform_target::PlatformTarget;
#[cfg(windows)]
use crate::backend::runtime_path_for_install_path;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::settings::NpmPackageManager;
use crate::config::{Config, Settings};
use crate::duration::{elapsed_seconds_ceil, process_now};
use crate::install_context::InstallContext;
use crate::semver::{semver_is_at_least, semver_is_older_than, semver_triplet};
use crate::timeout;
use crate::toolset::{ToolRequest, ToolVersion, Toolset};
use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex as TokioMutex;

/// Tolerance applied when converting an absolute `before_date` back to a
/// relative duration for CLI flags. This ensures that a user-supplied
/// `minimum_release_age = "3d"` never gets rounded up to `4d` due to small amounts
/// of elapsed time between when mise resolved the cutoff and when it invoked
/// the package manager.
const BEFORE_DATE_TOLERANCE_SECS: u64 = 60;
const NPM_MIN_RELEASE_AGE_VERSION: &str = "11.10.0";
const AUBE_PROGRAM: &str = if cfg!(windows) { "aube.exe" } else { "aube" };
const BUN_MIN_RELEASE_AGE_VERSION: &str = "1.3.0";
const PNPM_MIN_RELEASE_AGE_VERSION: &str = "10.16.0";

#[derive(Debug)]
pub struct NPMBackend {
    ba: Arc<BackendArg>,
    // use a mutex to prevent deadlocks that occurs due to reentrant cache access
    latest_version_cache: TokioMutex<CacheManager<Option<String>>>,
}

const NPM_PROGRAM: &str = if cfg!(windows) { "npm.cmd" } else { "npm" };

#[async_trait]
impl Backend for NPMBackend {
    fn get_type(&self) -> BackendType {
        BackendType::Npm
    }

    fn ba(&self) -> &Arc<BackendArg> {
        &self.ba
    }

    fn mark_prereleases_from_version_pattern(&self) -> bool {
        true
    }

    fn get_dependencies(&self) -> eyre::Result<Vec<&str>> {
        // npm CLI is always needed for version queries (npm view), plus the configured
        // package manager for installation. We avoid listing all package managers to
        // prevent incorrect dependency edges.
        let settings = Settings::get();
        let package_manager = settings.npm.package_manager;
        let tool_name = self.tool_name();

        // Avoid circular dependency when installing npm itself
        // But we still need the configured package manager for installation
        if tool_name == "npm" {
            return match package_manager {
                NpmPackageManager::Auto => Ok(vec!["node"]),
                NpmPackageManager::Aube => Ok(vec!["node", "aube"]),
                NpmPackageManager::Bun => Ok(vec!["node", "bun"]),
                NpmPackageManager::Pnpm => Ok(vec!["node", "pnpm"]),
                NpmPackageManager::Npm => Ok(vec!["node"]),
            };
        }

        // Avoid circular dependency when installing the configured package manager
        // e.g., npm:bun with bun configured, or npm:pnpm with pnpm configured
        if tool_name == package_manager.to_string() {
            // Still need npm for version queries
            return Ok(vec!["node", "npm"]);
        }

        // For regular packages: need npm (for version queries) + configured package manager
        let mut deps = vec!["node", "npm"];
        match package_manager {
            NpmPackageManager::Auto => {}
            NpmPackageManager::Aube => deps.push("aube"),
            NpmPackageManager::Bun => deps.push("bun"),
            NpmPackageManager::Pnpm => deps.push("pnpm"),
            NpmPackageManager::Npm => {} // npm is already in deps
        }
        Ok(deps)
    }

    /// NPM installs packages from npm registry using version specs (e.g., eslint@8.0.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    fn get_optional_dependencies(&self) -> eyre::Result<Vec<&str>> {
        Ok(vec!["aube"])
    }

    fn resolve_lockfile_options(
        &self,
        request: &ToolRequest,
        _target: &PlatformTarget,
    ) -> BTreeMap<String, String> {
        let opts = request.options();
        let mut result = BTreeMap::new();

        for key in install_time_option_keys() {
            if let Some(value) = opts.get(&key) {
                result.insert(key, value.to_string());
            }
        }

        result
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        // Use npm CLI to respect custom registry configurations
        self.ensure_npm_for_version_check(config).await;
        timeout::run_with_timeout_async(
            async || {
                let env = self.dependency_env(config).await?;

                let raw = cmd!(
                    NPM_PROGRAM,
                    "view",
                    self.tool_name(),
                    "versions",
                    "time",
                    "--json"
                )
                .full_env(&env)
                .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
                .read()?;
                let data: Value = serde_json::from_str(&raw)?;
                let versions = data["versions"]
                    .as_array()
                    .ok_or_else(|| eyre::eyre!("invalid versions"))?;
                let time = data["time"]
                    .as_object()
                    .ok_or_else(|| eyre::eyre!("invalid time"))?;
                let version_info = versions
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|version| {
                        let created_at = time
                            .get(version)
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        VersionInfo {
                            version: version.to_string(),
                            created_at,
                            prerelease: is_semver_prerelease(version),
                            ..Default::default()
                        }
                    })
                    .collect();

                Ok(version_info)
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        // TODO: Add bun support for getting latest version without npm
        // See TODO in _list_remote_versions for details
        self.ensure_npm_for_version_check(config).await;

        let cache = self.latest_version_cache.lock().await;
        let this = self;
        timeout::run_with_timeout_async(
            async || {
                cache
                    .get_or_try_init_async(async || {
                        // Always use npm for getting version info since bun info requires package.json
                        // bun is only used for actual package installation
                        let raw =
                            cmd!(NPM_PROGRAM, "view", this.tool_name(), "dist-tags", "--json")
                                .full_env(this.dependency_env(config).await?)
                                .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
                                .read()?;
                        let dist_tags: Value = serde_json::from_str(&raw)?;
                        match dist_tags["latest"] {
                            Value::String(ref s) => Ok(Some(s.clone())),
                            _ => Ok(None),
                        }
                    })
                    .await
            },
            Settings::get().fetch_remote_versions_timeout(),
        )
        .await
        .cloned()
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let package_manager = self
            .package_manager_for_install(&ctx.config, Some(&ctx.ts))
            .await;
        self.check_install_deps(&ctx.config, package_manager, Some(&ctx.ts))
            .await;
        let options = tv.request.options();
        let install_before_args = match ctx.before_date {
            Some(before_date) => {
                self.warn_if_package_manager_may_not_support_release_age(ctx, package_manager)
                    .await;
                self.build_transitive_release_age_args(&ctx.config, package_manager, before_date)
                    .await
            }
            None => Vec::new(),
        };
        match package_manager {
            NpmPackageManager::Auto => unreachable!("auto package manager should be resolved"),
            NpmPackageManager::Aube => {
                let aube_program = self
                    .aube_path_for_install(&ctx.config, Some(&ctx.ts))
                    .await
                    .unwrap_or_else(|| AUBE_PROGRAM.into());
                self.write_aube_npmrc(&tv.install_path(), ctx.before_date)?;
                let mut cmd = CmdLineRunner::new(aube_program)
                    .arg("add")
                    .arg("--global")
                    .arg(format!("{}@{}", self.tool_name(), tv.version))
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
                    .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                    .prepend_path(
                        self.dependency_toolset(&ctx.config)
                            .await?
                            .list_paths(&ctx.config)
                            .await,
                    )?
                    .current_dir(tv.install_path());
                if let Some(args) = options.get("aube_args") {
                    cmd = cmd.args(shell_words::split(args)?);
                }
                cmd.execute()?;
            }
            NpmPackageManager::Bun => {
                let mut cmd = CmdLineRunner::new("bun")
                    .arg("install")
                    .arg(format!("{}@{}", self.tool_name(), tv.version))
                    .arg("--global")
                    .arg("--trust")
                    // Isolated linker does not symlink binaries into BUN_INSTALL_BIN properly.
                    // https://github.com/jdx/mise/discussions/7541
                    .arg("--linker")
                    .arg("hoisted")
                    .args(install_before_args)
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
                    .env("BUN_INSTALL_GLOBAL_DIR", tv.install_path())
                    .env("BUN_INSTALL_BIN", tv.install_path().join("bin"))
                    .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                    .prepend_path(
                        self.dependency_toolset(&ctx.config)
                            .await?
                            .list_paths(&ctx.config)
                            .await,
                    )?
                    .current_dir(tv.install_path());
                if let Some(args) = options.get("bun_args") {
                    cmd = cmd.args(shell_words::split(args)?);
                }
                cmd.execute()?;
            }
            NpmPackageManager::Pnpm => {
                let bin_dir = tv.install_path().join("bin");
                crate::file::create_dir_all(&bin_dir)?;
                let mut cmd = CmdLineRunner::new("pnpm")
                    .arg("add")
                    .arg("--global")
                    .arg(format!("{}@{}", self.tool_name(), tv.version))
                    .arg("--global-dir")
                    .arg(tv.install_path())
                    .arg("--global-bin-dir")
                    .arg(&bin_dir)
                    .args(install_before_args)
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
                    .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                    .prepend_path(
                        self.dependency_toolset(&ctx.config)
                            .await?
                            .list_paths(&ctx.config)
                            .await,
                    )?
                    // required to avoid pnpm error "global bin dir isn't in PATH"
                    // https://github.com/pnpm/pnpm/issues/9333
                    .prepend_path(vec![bin_dir])?;
                if let Some(args) = options.get("pnpm_args") {
                    cmd = cmd.args(shell_words::split(args)?);
                }
                cmd.execute()?;
            }
            _ => {
                let mut cmd = CmdLineRunner::new(NPM_PROGRAM)
                    .arg("install")
                    .arg("-g")
                    .arg(format!("{}@{}", self.tool_name(), tv.version))
                    .arg("--prefix")
                    .arg(tv.install_path())
                    .args(install_before_args)
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path_without_tools(&ctx.config).await?)
                    .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
                    .prepend_path(ctx.ts.list_paths(&ctx.config).await)?
                    .prepend_path(
                        self.dependency_toolset(&ctx.config)
                            .await?
                            .list_paths(&ctx.config)
                            .await,
                    )?;
                if let Some(args) = options.get("npm_args") {
                    cmd = cmd.args(shell_words::split(args)?);
                }
                cmd.execute()?;
            }
        }
        Ok(tv)
    }

    #[cfg(windows)]
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &crate::toolset::ToolVersion,
    ) -> eyre::Result<Vec<std::path::PathBuf>> {
        Ok(Self::windows_bin_paths_for_install_path(&tv.install_path())
            .into_iter()
            .map(|path| runtime_path_for_install_path(tv, path))
            .collect())
    }
}

impl NPMBackend {
    pub fn from_arg(ba: BackendArg) -> Self {
        Self {
            latest_version_cache: TokioMutex::new(
                CacheManagerBuilder::new(ba.cache_path.join("latest_version.msgpack.z"))
                    .with_fresh_duration(Settings::get().fetch_remote_versions_cache())
                    .build(),
            ),
            ba: Arc::new(ba),
        }
    }

    async fn build_transitive_release_age_args(
        &self,
        config: &Arc<Config>,
        package_manager: NpmPackageManager,
        before_date: Timestamp,
    ) -> Vec<OsString> {
        let seconds = elapsed_seconds_ceil(before_date, process_now());
        match package_manager {
            NpmPackageManager::Auto => unreachable!("auto package manager should be resolved"),
            NpmPackageManager::Aube => Vec::new(),
            NpmPackageManager::Npm => {
                // Sub-day windows always emit --before because --min-release-age
                // is day-granular — which is also the fallback for older npm.
                // Short-circuiting here lets us skip the `npm --version` probe
                // entirely when the cutoff is <24h.
                let supports_min_release_age =
                    seconds >= 86400 && self.npm_supports_min_release_age_flag(config).await;
                Self::build_npm_release_age_args(before_date, seconds, supports_min_release_age)
            }
            NpmPackageManager::Bun => Self::build_bun_release_age_args(seconds),
            NpmPackageManager::Pnpm => Self::build_pnpm_release_age_args(seconds),
        }
    }

    fn build_npm_release_age_args(
        before_date: Timestamp,
        seconds: u64,
        supports_min_release_age: bool,
    ) -> Vec<OsString> {
        // Both older npm (no --min-release-age) and sub-day windows
        // (--min-release-age is day-granular) fall back to --before.
        if !supports_min_release_age || seconds < 86400 {
            return vec!["--before".into(), before_date.to_string().into()];
        }
        // Apply the drift tolerance only for the day-based conversion;
        // bun/pnpm emit the cutoff in finer units so drift is harmless there.
        let days = seconds
            .saturating_sub(BEFORE_DATE_TOLERANCE_SECS)
            .div_ceil(86400)
            .max(1);
        vec![format!("--min-release-age={days}").into()]
    }

    fn build_bun_release_age_args(seconds: u64) -> Vec<OsString> {
        vec!["--minimum-release-age".into(), seconds.to_string().into()]
    }

    fn build_pnpm_release_age_args(seconds: u64) -> Vec<OsString> {
        let minutes = seconds.div_ceil(60);
        vec![format!("--config.minimumReleaseAge={minutes}").into()]
    }

    async fn warn_if_package_manager_may_not_support_release_age(
        &self,
        ctx: &InstallContext,
        package_manager: NpmPackageManager,
    ) {
        let Some((tool, required_version, flag)) =
            Self::release_age_package_manager_requirement(package_manager)
        else {
            return;
        };

        let version = match Self::toolset_package_manager_version(&ctx.ts, tool) {
            Some(version) => Some(version),
            None => match self.dependency_toolset(&ctx.config).await {
                Ok(ts) => Self::toolset_package_manager_version(&ts, tool),
                Err(_) => None,
            },
        };

        let Some(version) = version else {
            return;
        };

        if semver_is_older_than(&version, required_version).unwrap_or(false) {
            warn!(
                "minimum_release_age is set for npm:{} but {}@{} is older than the documented minimum {}@{} required for {}. Older versions may fail while processing the forwarded argument. See https://mise.en.dev/dev-tools/backends/npm.html",
                self.tool_name(),
                tool,
                version,
                tool,
                required_version,
                flag
            );
        }
    }

    fn release_age_package_manager_requirement(
        package_manager: NpmPackageManager,
    ) -> Option<(&'static str, &'static str, &'static str)> {
        match package_manager {
            NpmPackageManager::Auto => None,
            NpmPackageManager::Aube => None,
            NpmPackageManager::Npm => None,
            NpmPackageManager::Bun => {
                Some(("bun", BUN_MIN_RELEASE_AGE_VERSION, "--minimum-release-age"))
            }
            NpmPackageManager::Pnpm => Some((
                "pnpm",
                PNPM_MIN_RELEASE_AGE_VERSION,
                "--config.minimumReleaseAge",
            )),
        }
    }

    fn toolset_package_manager_version(ts: &Toolset, tool: &str) -> Option<String> {
        let tvl = ts
            .versions
            .iter()
            .find(|(ba, _)| ba.short == tool)
            .map(|(_, tvl)| tvl)?;

        if let Some(tv) = tvl
            .versions
            .iter()
            .find(|tv| semver_triplet(&tv.version).is_some())
        {
            return Some(tv.version.clone());
        }

        tvl.requests
            .iter()
            .map(|tr| tr.version())
            .find(|version| semver_triplet(version).is_some())
    }

    /// Detect whether the locally installed npm supports --min-release-age.
    /// When npm is explicitly managed by mise, the version is read from the
    /// dependency ToolSet without spawning a subprocess. Otherwise falls back
    /// to `npm --version`. Returns false on any failure so callers
    /// transparently fall back to the older --before flag.
    async fn npm_supports_min_release_age_flag(&self, config: &Arc<Config>) -> bool {
        // When npm is explicitly managed by mise (e.g. `mise use npm@11.10.0`),
        // pull the resolved version from the dependency ToolSet and skip the
        // subprocess entirely.
        if let Ok(ts) = self.dependency_toolset(config).await {
            for (ba, tvl) in &ts.versions {
                if ba.short == "npm"
                    && let Some(tv) = tvl.versions.first()
                {
                    debug!(
                        "npm version detection: found npm {} in ToolSet, skipping subprocess",
                        tv.version
                    );
                    return semver_is_at_least(&tv.version, NPM_MIN_RELEASE_AGE_VERSION)
                        .unwrap_or(false);
                }
            }
        }

        // Fallback for node-bundled npm: run `npm --version`
        let env = match self.dependency_env(config).await {
            Ok(env) => env,
            Err(e) => {
                debug!(
                    "npm version detection: dependency_env failed, using --before fallback: {e:#}"
                );
                return false;
            }
        };
        let output = match cmd!(NPM_PROGRAM, "--version")
            .full_env(env)
            .env("NPM_CONFIG_UPDATE_NOTIFIER", "false")
            .read()
        {
            Ok(s) => s,
            Err(e) => {
                debug!(
                    "npm version detection: `npm --version` failed, using --before fallback: {e:#}"
                );
                return false;
            }
        };
        semver_is_at_least(&output, NPM_MIN_RELEASE_AGE_VERSION).unwrap_or(false)
    }

    /// Check dependencies for version checking (always needs npm)
    async fn ensure_npm_for_version_check(&self, config: &Arc<Config>) {
        // We always need npm for querying package versions
        // TODO: Once bun supports querying packages without package.json, this can be updated
        self.warn_if_dependency_missing(
            config,
            "npm", // Use "npm" for dependency check, which will check npm.cmd on Windows
            &["node", "npm"],
            "To use npm packages with mise, you need to install Node.js first:\n\
              mise use node@latest\n\n\
            Note: npm is required for querying package information, even when using aube, bun, or pnpm for installation.",
        )
        .await
    }

    /// Check dependencies for package installation (npm or bun based on settings)
    async fn check_install_deps(
        &self,
        config: &Arc<Config>,
        package_manager: NpmPackageManager,
        ts: Option<&Toolset>,
    ) {
        match package_manager {
            NpmPackageManager::Aube => {
                if let Some(ts) = ts
                    && ts.which_bin(config, AUBE_PROGRAM).await.is_some()
                {
                    return;
                }
                self.warn_if_dependency_missing(
                    config,
                    "aube",
                    &["aube"],
                    "To use npm packages with aube, you need to install aube first:\n\
                          mise use aube@latest\n\n\
                        Or switch back to npm by setting:\n\
                          mise settings npm.package_manager=npm",
                )
                .await
            }
            NpmPackageManager::Bun => {
                self.warn_if_dependency_missing(
                    config,
                    "bun",
                    &["bun"],
                    "To use npm packages with bun, you need to install bun first:\n\
                      mise use bun@latest\n\n\
                    Or switch back to npm by setting:\n\
                      mise settings npm.package_manager=npm",
                )
                .await
            }
            NpmPackageManager::Pnpm => {
                self.warn_if_dependency_missing(
                    config,
                    "pnpm",
                    &["pnpm"],
                    "To use npm packages with pnpm, you need to install pnpm first:\n\
                      mise use pnpm@latest\n\n\
                    Or switch back to npm by setting:\n\
                      mise settings npm.package_manager=npm",
                )
                .await
            }
            NpmPackageManager::Auto => {
                unreachable!("auto package manager should be resolved before dependency checks")
            }
            NpmPackageManager::Npm => {
                self.warn_if_dependency_missing(
                    config,
                    "npm",
                    &["node", "npm"],
                    "To use npm packages with mise, you need to install Node.js first:\n\
                      mise use node@latest\n\n\
                    Alternatively, install aube to use it automatically, or set:\n\
                      mise settings npm.package_manager=aube",
                )
                .await
            }
        }
    }

    async fn package_manager_for_install(
        &self,
        config: &Arc<Config>,
        ts: Option<&Toolset>,
    ) -> NpmPackageManager {
        let settings = Settings::get();
        match settings.npm.package_manager {
            NpmPackageManager::Auto if self.aube_is_installed(config, ts).await => {
                NpmPackageManager::Aube
            }
            NpmPackageManager::Auto => NpmPackageManager::Npm,
            package_manager => package_manager,
        }
    }

    async fn aube_is_installed(&self, config: &Arc<Config>, ts: Option<&Toolset>) -> bool {
        self.aube_path_for_install(config, ts).await.is_some()
    }

    async fn aube_path_for_install(
        &self,
        config: &Arc<Config>,
        ts: Option<&Toolset>,
    ) -> Option<std::path::PathBuf> {
        if let Some(ts) = ts
            && let Some(bin) = ts.which_bin(config, AUBE_PROGRAM).await
        {
            return Some(bin);
        }
        self.dependency_which(config, AUBE_PROGRAM).await
    }

    fn write_aube_npmrc(&self, install_path: &Path, before_date: Option<Timestamp>) -> Result<()> {
        let bin_dir = install_path.join("bin");
        crate::file::create_dir_all(install_path)?;
        crate::file::create_dir_all(&bin_dir)?;
        let mut npmrc = format!(
            "globalDir={}\nglobalBinDir={}\n",
            Self::npmrc_path_value(install_path),
            Self::npmrc_path_value(&bin_dir)
        );
        if let Some(before_date) = before_date {
            let minutes = Self::build_aube_minimum_release_age(elapsed_seconds_ceil(
                before_date,
                process_now(),
            ));
            // aube documents minimumReleaseAge in minutes, matching pnpm's setting.
            npmrc.push_str(&format!("minimumReleaseAge={minutes}\n"));
        }
        crate::file::write(install_path.join(".npmrc"), npmrc)?;
        Ok(())
    }

    fn npmrc_path_value(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn build_aube_minimum_release_age(seconds: u64) -> u64 {
        seconds.div_ceil(60)
    }

    #[cfg(any(windows, test))]
    fn windows_bin_paths_for_install_path(install_path: &Path) -> Vec<std::path::PathBuf> {
        let bin_dir = install_path.join("bin");
        if bin_dir.exists() {
            vec![bin_dir]
        } else {
            vec![install_path.to_path_buf()]
        }
    }
}

/// Returns true if `version` is a semver pre-release.
///
/// npm enforces strict semver (rule 9): any hyphen-introduced identifier after
/// the version core is a pre-release (`1.0.0-rc.1`, `0.42.0-nightly...`,
/// `2.0.0-canary.1`, `3.0.0-foo`). Build metadata (`+...`) is stripped first so
/// stable builds like `1.0.0+sha.abc` are not misclassified.
///
/// Stricter than the generic `VERSION_REGEX` channel-tag list — for npm it
/// catches any pre-release tag the maintainer chooses, not just the well-known
/// names mise happens to recognize.
fn is_semver_prerelease(version: &str) -> bool {
    let core_and_pre = version.split_once('+').map_or(version, |(v, _)| v);
    core_and_pre.contains('-')
}

/// Returns install-time-only option keys for NPM backend.
pub fn install_time_option_keys() -> Vec<String> {
    vec![
        "npm_args".into(),
        "pnpm_args".into(),
        "bun_args".into(),
        "aube_args".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{BackendArg, BackendResolution};
    use crate::toolset::{ToolRequest, ToolSource, ToolVersionList, ToolVersionOptions};
    use pretty_assertions::assert_eq;
    use std::sync::Arc;

    fn create_npm_backend(tool: &str) -> NPMBackend {
        let ba = BackendArg::new_raw(
            "npm".to_string(),
            Some(tool.to_string()),
            tool.to_string(),
            None,
            BackendResolution::new(true),
        );
        NPMBackend::from_arg(ba)
    }

    fn create_test_backend_arg(tool: &str) -> Arc<BackendArg> {
        Arc::new(BackendArg::new_raw(
            tool.to_string(),
            None,
            tool.to_string(),
            None,
            BackendResolution::new(true),
        ))
    }

    fn create_test_tool_request(ba: Arc<BackendArg>, version: &str) -> ToolRequest {
        ToolRequest::Version {
            backend: ba,
            version: version.to_string(),
            options: ToolVersionOptions::default(),
            source: ToolSource::Argument,
        }
    }

    #[test]
    fn test_get_dependencies_for_npm_itself() {
        // When the tool is npm itself (npm:npm) with default settings (npm as package manager),
        // it should only depend on node. With bun/pnpm configured, it would include those too.
        let backend = create_npm_backend("npm");
        let deps = backend.get_dependencies().unwrap();
        assert_eq!(deps, vec!["node"]);
    }

    #[test]
    fn test_get_dependencies_default_package_manager() {
        // With default settings (npm), packages should depend on node + npm
        let backend = create_npm_backend("prettier");
        let deps = backend.get_dependencies().unwrap();
        assert!(deps.contains(&"node"));
        assert!(deps.contains(&"npm"));
        assert!(!deps.contains(&"bun"));
        assert!(!deps.contains(&"pnpm"));
    }

    #[test]
    fn test_build_npm_release_age_args_legacy() {
        let before_date: Timestamp = "2024-01-02T03:04:05Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400, false);
        assert_eq!(
            args,
            vec![
                OsString::from("--before"),
                OsString::from("2024-01-02T03:04:05Z")
            ]
        );
    }

    #[test]
    fn test_build_npm_release_age_args_sub_day_uses_before() {
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 1, true);
        assert_eq!(
            args,
            vec![
                OsString::from("--before"),
                OsString::from("2024-01-01T00:00:00Z")
            ]
        );
    }

    #[test]
    fn test_build_npm_release_age_args_full_days() {
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=3")]);
    }

    #[test]
    fn test_build_npm_release_age_args_tolerates_drift() {
        // Regression test for #9156: "3d" re-converted after ~30s of drift
        // must not round up to 4 days.
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3 + 30, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=3")]);
    }

    #[test]
    fn test_build_npm_release_age_args_past_tolerance_rounds_up() {
        // Drift larger than BEFORE_DATE_TOLERANCE_SECS still rounds up so
        // cutoffs remain at least as strict as requested.
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3 + 120, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=4")]);
    }

    #[test]
    fn test_build_npm_release_age_args_one_day_boundary() {
        // Small drift at the 1-day boundary must stay at --min-release-age=1
        // instead of falling through to --before.
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 + 5, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=1")]);
    }

    #[test]
    fn test_build_bun_release_age_args() {
        let args = NPMBackend::build_bun_release_age_args(1);
        assert_eq!(
            args,
            vec![OsString::from("--minimum-release-age"), OsString::from("1")]
        );
    }

    #[test]
    fn test_build_pnpm_release_age_args_rounds_up_to_minutes() {
        let args = NPMBackend::build_pnpm_release_age_args(1);
        assert_eq!(args, vec![OsString::from("--config.minimumReleaseAge=1")]);
    }

    #[test]
    fn test_build_aube_minimum_release_age_rounds_up_to_minutes() {
        assert_eq!(NPMBackend::build_aube_minimum_release_age(1), 1);
        assert_eq!(NPMBackend::build_aube_minimum_release_age(60), 1);
        assert_eq!(NPMBackend::build_aube_minimum_release_age(61), 2);
    }

    #[test]
    fn test_npmrc_path_value_uses_forward_slashes() {
        assert_eq!(
            NPMBackend::npmrc_path_value(Path::new(r"C:\Users\me\mise\npm-cowsay\1.6.0")),
            "C:/Users/me/mise/npm-cowsay/1.6.0"
        );
    }

    #[test]
    fn test_windows_bin_paths_prefers_created_bin_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let install_path = tmp.path().join("npm-cowsay").join("1.6.0");
        std::fs::create_dir_all(install_path.join("bin")).unwrap();

        assert_eq!(
            NPMBackend::windows_bin_paths_for_install_path(&install_path),
            vec![install_path.join("bin")]
        );
    }

    #[test]
    fn test_windows_bin_paths_falls_back_to_install_path() {
        let tmp = tempfile::tempdir().unwrap();
        let install_path = tmp.path().join("npm-cowsay").join("1.6.0");

        assert_eq!(
            NPMBackend::windows_bin_paths_for_install_path(&install_path),
            vec![install_path]
        );
    }

    #[test]
    fn test_release_age_package_manager_requirements() {
        assert_eq!(
            NPMBackend::release_age_package_manager_requirement(NpmPackageManager::Auto),
            None
        );
        assert_eq!(
            NPMBackend::release_age_package_manager_requirement(NpmPackageManager::Aube),
            None
        );
        assert_eq!(
            NPMBackend::release_age_package_manager_requirement(NpmPackageManager::Npm),
            None
        );
        assert_eq!(
            NPMBackend::release_age_package_manager_requirement(NpmPackageManager::Bun),
            Some(("bun", BUN_MIN_RELEASE_AGE_VERSION, "--minimum-release-age"))
        );
        assert_eq!(
            NPMBackend::release_age_package_manager_requirement(NpmPackageManager::Pnpm),
            Some((
                "pnpm",
                PNPM_MIN_RELEASE_AGE_VERSION,
                "--config.minimumReleaseAge"
            ))
        );
    }

    #[test]
    fn test_npm_min_release_age_version_requirement() {
        assert_eq!(NPM_MIN_RELEASE_AGE_VERSION, "11.10.0");
        assert_eq!(
            crate::semver::semver_is_at_least("11.10.0", NPM_MIN_RELEASE_AGE_VERSION),
            Some(true)
        );
        assert_eq!(
            crate::semver::semver_is_at_least("11.9.9", NPM_MIN_RELEASE_AGE_VERSION),
            Some(false)
        );
    }

    #[test]
    fn test_toolset_package_manager_version_prefers_resolved_version() {
        let ba = create_test_backend_arg("bun");
        let request = create_test_tool_request(ba.clone(), "1.2.0");
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests.push(request.clone());
        tvl.versions
            .push(ToolVersion::new(request, "1.3.0".to_string()));

        let mut ts = Toolset::default();
        ts.versions.insert(ba, tvl);

        assert_eq!(
            NPMBackend::toolset_package_manager_version(&ts, "bun"),
            Some("1.3.0".to_string())
        );
    }

    #[test]
    fn test_toolset_package_manager_version_uses_exact_request() {
        let ba = create_test_backend_arg("pnpm");
        let request = create_test_tool_request(ba.clone(), "10.15.0");
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests.push(request);

        let mut ts = Toolset::default();
        ts.versions.insert(ba, tvl);

        assert_eq!(
            NPMBackend::toolset_package_manager_version(&ts, "pnpm"),
            Some("10.15.0".to_string())
        );
    }

    #[test]
    fn test_toolset_package_manager_version_ignores_unresolved_request() {
        let ba = create_test_backend_arg("pnpm");
        let request = create_test_tool_request(ba.clone(), "10");
        let mut tvl = ToolVersionList::new(ba.clone(), ToolSource::Argument);
        tvl.requests.push(request);

        let mut ts = Toolset::default();
        ts.versions.insert(ba, tvl);

        assert_eq!(
            NPMBackend::toolset_package_manager_version(&ts, "pnpm"),
            None
        );
    }

    #[test]
    fn test_resolve_lockfile_options_includes_install_args_only() {
        let backend = create_npm_backend("react-devtools");
        let mut options = ToolVersionOptions::default();
        options.opts.insert(
            "npm_args".to_string(),
            toml::Value::String("--ignore-scripts=false".into()),
        );
        options.opts.insert(
            "bun_args".to_string(),
            toml::Value::String("--allow-same-version".into()),
        );
        options.opts.insert(
            "aube_args".to_string(),
            toml::Value::String("--loglevel=warn".into()),
        );
        options.install_env.insert(
            "NPM_CONFIG_REGISTRY".to_string(),
            "https://registry.example.com".to_string(),
        );

        let request =
            ToolRequest::new_opts(backend.ba().clone(), "latest", options, ToolSource::Unknown)
                .unwrap();
        let resolved = backend.resolve_lockfile_options(&request, &PlatformTarget::from_current());
        assert_eq!(
            resolved.get("npm_args"),
            Some(&"--ignore-scripts=false".to_string())
        );
        assert_eq!(
            resolved.get("bun_args"),
            Some(&"--allow-same-version".to_string())
        );
        assert_eq!(
            resolved.get("aube_args"),
            Some(&"--loglevel=warn".to_string())
        );
        assert!(!resolved.contains_key("install_env.NPM_CONFIG_REGISTRY"));
    }

    #[test]
    fn test_is_semver_prerelease_flags_hyphen_suffix() {
        // Per semver rule 9, any hyphen-introduced identifier is a pre-release.
        // Covers GitHub discussion #9503 (-nightly slipping past channel-name regex).
        assert!(is_semver_prerelease("0.42.0-nightly.20260429.g6d9911393"));
        assert!(is_semver_prerelease("1.0.0-rc.1"));
        assert!(is_semver_prerelease("2.0.0-canary"));
        assert!(is_semver_prerelease("3.0.0-foo"));
        // Maintainer-invented tag mise's regex doesn't know about — still flagged.
        assert!(is_semver_prerelease("4.0.0-internal-build-7"));
    }

    #[test]
    fn test_is_semver_prerelease_keeps_stable_versions() {
        assert!(!is_semver_prerelease("1.0.0"));
        assert!(!is_semver_prerelease("0.40.1"));
        assert!(!is_semver_prerelease("v22.6.0"));
        // Build metadata alone is not a pre-release.
        assert!(!is_semver_prerelease("1.0.0+sha.abc1234"));
    }

    #[test]
    fn test_is_semver_prerelease_strips_build_metadata_first() {
        // `+build` after a `-pre` tag must still flag as pre-release.
        assert!(is_semver_prerelease("1.0.0-rc.1+build.5"));
        // Hyphen only inside build metadata (not legal semver, but be defensive)
        // — we treat it as stable since the version core has no pre-release.
        assert!(!is_semver_prerelease("1.0.0+build-5"));
    }
}
