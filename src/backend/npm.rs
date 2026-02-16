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
use std::collections::BTreeMap;
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
        // npm CLI is needed for version queries (npm view) unless bun is used.
        // We also need the configured package manager for installation.
        // We avoid listing all package managers to prevent incorrect dependency edges.
        let settings = Settings::get();
        let package_manager = settings.npm.package_manager;
        let tool_name = self.tool_name();

        let mut deps = match package_manager {
            NpmPackageManager::Npm => vec!["node", "npm"],
            NpmPackageManager::Bun => vec!["bun"],
            // `pnpm view` internally calls npm
            NpmPackageManager::Pnpm => vec!["node", "npm", "pnpm"],
        };

        // Avoid circular dependency when installing package managers themselves
        if tool_name == "npm" && package_manager == NpmPackageManager::Npm {
            deps.retain(|&dep| dep != "npm");
        }

        Ok(deps)
    }

    /// NPM installs packages from npm registry using version specs (e.g., eslint@8.0.0).
    /// It doesn't support installing from direct URLs, so lockfile URLs are not applicable.
    fn supports_lockfile_url(&self) -> bool {
        false
    }

    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<VersionInfo>> {
        let settings = Settings::get();
        let package_manager = settings.npm.package_manager;

        match package_manager {
            NpmPackageManager::Npm => {
                // Use npm CLI to respect custom registry configurations
                self.ensure_dependency(config, NpmPackageManager::Npm).await;
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
                    settings.fetch_remote_versions_timeout(),
                )
                .await
            }
            NpmPackageManager::Bun => {
                self.ensure_dependency(config, NpmPackageManager::Bun).await;
                timeout::run_with_timeout_async(
                    async || {
                        let env = self.dependency_env(config).await?;
                        // Bun doesn't support fetching specific fields like npm does, but fetching the package
                        // metadata returns everything we need including versions and time.
                        let output =
                            self.read_bun_view(&env, vec![self.tool_name().to_string()], config)?;

                        let data: Value = serde_json::from_str(&output)?;
                        let versions = data["versions"]
                            .as_object()
                            .ok_or_else(|| eyre::eyre!("invalid versions"))?;
                        let time = data["time"]
                            .as_object()
                            .ok_or_else(|| eyre::eyre!("invalid time"))?;

                        let version_info = versions
                            .keys()
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
                    settings.fetch_remote_versions_timeout(),
                )
                .await
            }
            NpmPackageManager::Pnpm => {
                self.ensure_dependency(config, NpmPackageManager::Pnpm)
                    .await;
                timeout::run_with_timeout_async(
                    async || {
                        let env = self.dependency_env(config).await?;

                        // pnpm view calls npm view internally
                        let raw = cmd!(
                            "pnpm",
                            "view",
                            self.tool_name(),
                            "versions",
                            "time",
                            "--json"
                        )
                        .full_env(&env)
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
                    settings.fetch_remote_versions_timeout(),
                )
                .await
            }
        }
    }

    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        let settings = Settings::get();
        let package_manager = settings.npm.package_manager;

        if package_manager == NpmPackageManager::Bun {
            self.ensure_dependency(config, NpmPackageManager::Bun).await;
            let cache = self.latest_version_cache.lock().await;
            let this = self;
            timeout::run_with_timeout_async(
                async || {
                    cache
                        .get_or_try_init_async(async || {
                            let output = this.read_bun_view(
                                &this.dependency_env(config).await?,
                                vec![this.tool_name().to_string(), "dist-tags".to_string()],
                                config,
                            )?;
                            let dist_tags: Value = serde_json::from_str(&output)?;
                            match dist_tags["latest"] {
                                Value::String(ref s) => Ok(Some(s.clone())),
                                _ => this.latest_version(config, Some("latest".into())).await,
                            }
                        })
                        .await
                },
                settings.fetch_remote_versions_timeout(),
            )
            .await
            .cloned()
        } else if package_manager == NpmPackageManager::Pnpm {
            self.ensure_dependency(config, NpmPackageManager::Pnpm)
                .await;
            let cache = self.latest_version_cache.lock().await;
            let this = self;
            timeout::run_with_timeout_async(
                async || {
                    cache
                        .get_or_try_init_async(async || {
                            // pnpm view calls npm view internally
                            let raw = cmd!("pnpm", "view", this.tool_name(), "dist-tags", "--json")
                                .full_env(this.dependency_env(config).await?)
                                .read()?;
                            let dist_tags: Value = serde_json::from_str(&raw)?;
                            match dist_tags["latest"] {
                                Value::String(ref s) => Ok(Some(s.clone())),
                                _ => this.latest_version(config, Some("latest".into())).await,
                            }
                        })
                        .await
                },
                settings.fetch_remote_versions_timeout(),
            )
            .await
            .cloned()
        } else {
            self.ensure_dependency(config, NpmPackageManager::Npm).await;
            let cache = self.latest_version_cache.lock().await;
            let this = self;
            timeout::run_with_timeout_async(
                async || {
                    cache
                        .get_or_try_init_async(async || {
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
                settings.fetch_remote_versions_timeout(),
            )
            .await
            .cloned()
        }
    }

    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion> {
        let settings = Settings::get();
        let package_manager = settings.npm.package_manager;
        match package_manager {
            NpmPackageManager::Bun => {
                self.ensure_dependency(&ctx.config, NpmPackageManager::Bun)
                    .await
            }
            NpmPackageManager::Pnpm => {
                self.ensure_dependency(&ctx.config, NpmPackageManager::Pnpm)
                    .await
            }
            NpmPackageManager::Npm => {
                self.ensure_dependency(&ctx.config, NpmPackageManager::Npm)
                    .await
            }
        }

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

    fn read_bun_view(
        &self,
        env: &BTreeMap<String, String>,
        args: Vec<String>,
        _config: &Arc<Config>,
    ) -> eyre::Result<String> {
        let temp_dir_root = &*crate::env::MISE_INSTALLS_DIR;
        if !temp_dir_root.exists() {
            crate::file::create_dir_all(temp_dir_root)?;
        }
        let temp_dir = tempfile::Builder::new()
            .prefix("mise-bun-view-")
            .tempdir_in(temp_dir_root)?;
        let package_json = temp_dir.path().join("package.json");
        crate::file::write(&package_json, "{}")?;

        let mut full_args = vec!["pm".to_string(), "view".to_string(), "--json".to_string()];
        full_args.extend(args);

        let mut cmd = crate::cmd::cmd("bun", full_args);

        // We do .env here to ensure we inherit PATH properly if needed, although full_env might overwrite PATH.
        // It's tricky with duct. Usually full_env replaces everything.
        // Let's rely on full_env.
        cmd = cmd.full_env(env);

        // But we MUST run in the temp dir
        cmd = cmd.dir(temp_dir.path());

        Ok(cmd.read()?)
    }

    async fn ensure_dependency(&self, config: &Arc<Config>, pm: NpmPackageManager) {
        let (cmd, msg) = match pm {
            NpmPackageManager::Bun => (
                "bun",
                "To use npm packages with bun, you need to install bun first:\n\
                  mise use bun@latest\n\n\
                Or switch back to npm by setting:\n\
                  mise settings npm.package_manager=npm",
            ),
            NpmPackageManager::Pnpm => (
                "pnpm",
                "To use npm packages with pnpm, you need to install pnpm first:\n\
                  mise use pnpm@latest\n\n\
                Or switch back to npm by setting:\n\
                  mise settings npm.package_manager=npm",
            ),
            NpmPackageManager::Npm => (
                "npm", // Use "npm" for dependency check, which will check npm.cmd on Windows
                "To use npm packages with mise, you need to install Node.js first:\n\
                  mise use node@latest\n\n\
                Alternatively, you can use bun or pnpm instead of npm by setting:\n\
                  mise settings npm.package_manager=bun",
            ),
        };

        self.warn_if_dependency_missing(config, cmd, msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::{BackendArg, BackendResolution};
    use std::sync::Mutex;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

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
        let _guard = TEST_MUTEX.lock().unwrap();
        // Ensure clean env
        crate::env::remove_var("MISE_NPM_PACKAGE_MANAGER");
        Settings::reset(None);

        // When the tool is npm itself (npm:npm) with default settings (npm as package manager),
        // it should only depend on node. With bun/pnpm configured, it would include those too.
        let backend = create_npm_backend("npm");
        let deps = backend.get_dependencies().unwrap();
        assert_eq!(deps, vec!["node"]);
    }

    #[test]
    fn test_get_dependencies_default_package_manager() {
        let _guard = TEST_MUTEX.lock().unwrap();
        crate::env::remove_var("MISE_NPM_PACKAGE_MANAGER");
        Settings::reset(None);

        // With default settings (npm), packages should depend on node + npm
        let backend = create_npm_backend("prettier");
        let deps = backend.get_dependencies().unwrap();
        assert!(deps.contains(&"node"));
        assert!(deps.contains(&"npm"));
        assert!(!deps.contains(&"bun"));
        assert!(!deps.contains(&"pnpm"));
    }

    #[test]
    fn test_get_dependencies_bun_package_manager() {
        let _guard = TEST_MUTEX.lock().unwrap();

        // With bun settings, packages should depend on bun only
        crate::env::set_var("MISE_NPM_PACKAGE_MANAGER", "bun");
        Settings::reset(None);

        // Force refresh of Settings if needed, but in tests Settings::get() reads env vars usually?
        // Actually Settings struct is built from config files and env vars.
        // We might need to ensure settings are re-read or mocked.
        // But Settings::get() might be cached?
        // Let's assume env var works for now, or check Settings implementation.
        // If not, we might need to manually construct Settings or Config.
        // But let's try setting env var.
        let backend = create_npm_backend("prettier");
        let deps = backend.get_dependencies().unwrap();

        // Reset env var
        crate::env::remove_var("MISE_NPM_PACKAGE_MANAGER");
        Settings::reset(None);

        assert!(deps.contains(&"bun"));
        assert!(!deps.contains(&"node"));
        assert!(!deps.contains(&"npm"));
        assert!(!deps.contains(&"pnpm"));
    }

    #[test]
    fn test_get_dependencies_bun_package_manager_for_npm_tool() {
        let _guard = TEST_MUTEX.lock().unwrap();

        // With bun settings, npm tool should depend on bun only
        crate::env::set_var("MISE_NPM_PACKAGE_MANAGER", "bun");
        Settings::reset(None);

        let backend = create_npm_backend("npm");
        let deps = backend.get_dependencies().unwrap();

        // Reset env var
        crate::env::remove_var("MISE_NPM_PACKAGE_MANAGER");
        Settings::reset(None);

        assert!(deps.contains(&"bun"));
        assert!(!deps.contains(&"node"));
    }

    #[test]
    fn test_get_dependencies_bun_package_manager_for_bun_tool() {
        let _guard = TEST_MUTEX.lock().unwrap();

        // With bun settings, bun tool should depend on bun only
        crate::env::set_var("MISE_NPM_PACKAGE_MANAGER", "bun");
        Settings::reset(None);

        let backend = create_npm_backend("bun");
        let deps = backend.get_dependencies().unwrap();

        // Reset env var
        crate::env::remove_var("MISE_NPM_PACKAGE_MANAGER");
        Settings::reset(None);

        assert!(deps.contains(&"bun"));
        assert!(!deps.contains(&"node"));
        assert!(!deps.contains(&"npm"));
    }
}
