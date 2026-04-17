use crate::Result;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::settings::NpmPackageManager;
use crate::config::{Config, Settings};
use crate::duration::{elapsed_seconds_ceil, process_now};
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;
use std::ffi::OsString;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex as TokioMutex;

/// Tolerance applied when converting an absolute `before_date` back to a
/// relative duration for CLI flags. This ensures that a user-supplied
/// `install_before = "3d"` never gets rounded up to `4d` due to small amounts
/// of elapsed time between when mise resolved the cutoff and when it invoked
/// the package manager.
const BEFORE_DATE_TOLERANCE_SECS: u64 = 60;

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

        // dist-tags returns the absolute latest; bypass it when install_before
        // is set (per-tool or global) so that callers like `mise latest` / `mise edit`
        // that reach this method without a before_date context respect the cutoff.
        // (See jdx/mise#9136)
        let before_str = config
            .get_tool_opts(self.ba())
            .await
            .ok()
            .flatten()
            .and_then(|opts| opts.get("install_before").map(|s| s.to_string()))
            .or_else(|| Settings::get().install_before.clone());
        if let Some(before) = before_str
            .as_deref()
            .map(crate::duration::parse_into_timestamp)
            .transpose()?
        {
            return self
                .latest_version_with_opts(config, None, Some(before))
                .await;
        }

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
                            _ => this.latest_version(config, Some("latest".into())).await,
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
        self.check_install_deps(&ctx.config).await;
        let package_manager = Settings::get().npm.package_manager;
        let install_before_args = match ctx.before_date {
            Some(before_date) => {
                self.build_transitive_release_age_args(&ctx.config, package_manager, before_date)
                    .await
            }
            None => Vec::new(),
        };
        match package_manager {
            NpmPackageManager::Bun => {
                CmdLineRunner::new("bun")
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
                    .current_dir(tv.install_path())
                    .execute()?;
            }
            NpmPackageManager::Pnpm => {
                let bin_dir = tv.install_path().join("bin");
                crate::file::create_dir_all(&bin_dir)?;
                CmdLineRunner::new("pnpm")
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
                    .prepend_path(vec![bin_dir])?
                    .execute()?;
            }
            _ => {
                CmdLineRunner::new(NPM_PROGRAM)
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
                    )?
                    .execute()?;
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
        if Settings::get().npm.package_manager == NpmPackageManager::Npm {
            Ok(vec![tv.install_path()])
        } else {
            Ok(vec![tv.install_path().join("bin")])
        }
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
        // Either branch emits the same `--before` fallback, so merge them:
        //   * older npm without --min-release-age
        //   * sub-day windows (--min-release-age is day-granular)
        if !supports_min_release_age || seconds < 86400 {
            return vec!["--before".into(), before_date.to_string().into()];
        }
        // Tolerate a small amount of elapsed time between resolving
        // `install_before` and converting it back to a day count so that e.g.
        // "3d" doesn't get rounded up to "4d" just because a few seconds have
        // passed (see #9156). The tolerance only applies here because bun/pnpm
        // emit the cutoff in units (seconds / minutes) finer than typical drift.
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

    /// Returns true if the npm major.minor.patch version is >= 11.10.0,
    /// which is when the --min-release-age flag was added (npm/cli#8965).
    fn npm_version_supports_min_release_age(version: &str) -> bool {
        let trimmed = version.trim().trim_start_matches('v');
        let mut parts = trimmed.split(['.', '-', '+']);
        let major: u64 = match parts.next().and_then(|p| p.parse().ok()) {
            Some(v) => v,
            None => return false,
        };
        let minor: u64 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        // 11.10.0+ — only major+minor matter for the gate
        match major.cmp(&11) {
            std::cmp::Ordering::Greater => true,
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => minor >= 10,
        }
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
                    return Self::npm_version_supports_min_release_age(&tv.version);
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
        Self::npm_version_supports_min_release_age(&output)
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
            Note: npm is required for querying package information, even when using bun for installation.",
        )
        .await
    }

    /// Check dependencies for package installation (npm or bun based on settings)
    async fn check_install_deps(&self, config: &Arc<Config>) {
        match Settings::get().npm.package_manager {
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
            _ => {
                self.warn_if_dependency_missing(
                    config,
                    "npm",
                    &["node", "npm"],
                    "To use npm packages with mise, you need to install Node.js first:\n\
                      mise use node@latest\n\n\
                    Alternatively, you can use bun or pnpm instead of npm by setting:\n\
                      mise settings npm.package_manager=bun",
                )
                .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{BackendArg, BackendResolution};
    use pretty_assertions::assert_eq;

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
        // Older npm without --min-release-age always falls back to --before,
        // regardless of the cutoff window.
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
        // Sub-day windows fall back to --before even when npm supports
        // --min-release-age, because that flag is day-granular.
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
        // Exactly 3 full days → --min-release-age=3
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=3")]);
    }

    #[test]
    fn test_build_npm_release_age_args_tolerates_drift() {
        // Regression test for #9156: `install_before = "3d"` re-converted after
        // ~30s of drift must not round up to 4 days.
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3 + 30, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=3")]);
    }

    #[test]
    fn test_build_npm_release_age_args_past_tolerance_rounds_up() {
        // Drift larger than the tolerance (60s) should still round up to the
        // next day so cutoffs remain at least as strict as requested.
        let before_date: Timestamp = "2024-01-01T00:00:00Z".parse().unwrap();
        let args = NPMBackend::build_npm_release_age_args(before_date, 86400 * 3 + 120, true);
        assert_eq!(args, vec![OsString::from("--min-release-age=4")]);
    }

    #[test]
    fn test_build_npm_release_age_args_one_day_boundary() {
        // `install_before = "1d"` with a few seconds of drift should still
        // emit --min-release-age=1 rather than falling back to --before.
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
        // pnpm's --config.minimumReleaseAge is minute-granular; a 1s delta
        // should be rounded up to 1 minute.
        let args = NPMBackend::build_pnpm_release_age_args(1);
        assert_eq!(args, vec![OsString::from("--config.minimumReleaseAge=1")]);
    }

    #[test]
    fn test_npm_version_supports_min_release_age() {
        // 11.10.0 is the cutoff where --min-release-age was added
        assert!(NPMBackend::npm_version_supports_min_release_age("11.10.0"));
        assert!(NPMBackend::npm_version_supports_min_release_age("11.10.1"));
        assert!(NPMBackend::npm_version_supports_min_release_age("11.11.0"));
        assert!(NPMBackend::npm_version_supports_min_release_age("12.0.0"));
        // Tolerate `v` prefix and trailing whitespace from `npm --version`
        assert!(NPMBackend::npm_version_supports_min_release_age("v11.10.0"));
        assert!(NPMBackend::npm_version_supports_min_release_age(
            "11.10.0\n"
        ));
        // Pre-release still satisfies the gate (no known 11.10.0 pre-releases exist)
        assert!(NPMBackend::npm_version_supports_min_release_age(
            "11.10.0-pre.1"
        ));

        assert!(!NPMBackend::npm_version_supports_min_release_age("11.9.9"));
        assert!(!NPMBackend::npm_version_supports_min_release_age("11.0.0"));
        assert!(!NPMBackend::npm_version_supports_min_release_age("10.99.0"));
        assert!(!NPMBackend::npm_version_supports_min_release_age(""));
        assert!(!NPMBackend::npm_version_supports_min_release_age("garbage"));
    }
}
