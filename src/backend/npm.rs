use crate::Result;
use crate::backend::Backend;
use crate::backend::VersionInfo;
use crate::backend::backend_type::BackendType;
use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::BackendArg;
use crate::cmd::CmdLineRunner;
use crate::config::settings::NpmPackageManager;
use crate::config::{Config, Settings};
use crate::install_context::InstallContext;
use crate::timeout;
use crate::toolset::ToolVersion;
use async_trait::async_trait;
use serde_json::Value;
use std::{fmt::Debug, sync::Arc};
use tokio::sync::Mutex as TokioMutex;

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
        match Settings::get().npm.package_manager {
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
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path(&ctx.config).await?)
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
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path(&ctx.config).await?)
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
                    .with_pr(ctx.pr.as_ref())
                    .envs(ctx.ts.env_with_path(&ctx.config).await?)
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

    /// Check dependencies for version checking (always needs npm)
    async fn ensure_npm_for_version_check(&self, config: &Arc<Config>) {
        // We always need npm for querying package versions
        // TODO: Once bun supports querying packages without package.json, this can be updated
        self.warn_if_dependency_missing(
            config,
            "npm", // Use "npm" for dependency check, which will check npm.cmd on Windows
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
}
