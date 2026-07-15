use std::env::{join_paths, split_paths};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{WrapErr, bail};
use heck::ToKebabCase;
use indexmap::IndexMap;
use tokio::sync::OnceCell;
use vfox::{PackageActionContext, PackageInstalledContext, PackageRequest as VfoxPackageRequest};

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::config::Config;
use crate::plugins::mise_plugin_toml::{MisePluginToml, MisePluginTomlPackageManagerConfig};
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::result::Result;
use crate::toolset::{ConfigScope, ToolsetBuilder};

#[derive(Debug)]
pub struct PackagePluginManager {
    name: String,
    plugin: Arc<VfoxPlugin>,
    config: MisePluginTomlPackageManagerConfig,
    hook_env: OnceCell<IndexMap<String, String>>,
}

impl PackagePluginManager {
    pub fn new(name: String) -> Result<Self> {
        let plugin_path = crate::dirs::PLUGINS.join(name.to_kebab_case());
        let config =
            MisePluginToml::from_file(&plugin_path.join("mise.plugin.toml"))?.package_manager;
        let plugin = Arc::new(VfoxPlugin::new(name.clone(), plugin_path));
        Ok(Self {
            name,
            plugin,
            config,
            hook_env: OnceCell::new(),
        })
    }

    fn platform_available(&self) -> bool {
        self.config.os.as_ref().is_none_or(|oses| {
            oses.iter()
                .any(|os| os == crate::config::Settings::get().os())
        })
    }

    fn sync_lookup_path() -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|path| split_paths(&path).collect())
            .unwrap_or_default();
        if !paths.iter().any(|path| path == *crate::dirs::SHIMS) {
            paths.push(crate::dirs::SHIMS.to_path_buf());
        }
        paths
    }

    fn missing_from_path(&self, paths: &[PathBuf]) -> Option<&str> {
        let path = join_paths(paths).ok()?;
        let cwd = std::env::current_dir().ok()?;
        self.config
            .requires
            .iter()
            .find(|binary| which::which_in(binary, Some(&path), &cwd).is_err())
            .map(String::as_str)
    }

    async fn hook_env(&self) -> Result<&IndexMap<String, String>> {
        self.hook_env
            .get_or_try_init(|| async {
                let config = Config::get().await?;
                let toolset = ToolsetBuilder::new()
                    .with_scope(ConfigScope::GlobalOnly)
                    .build(&config)
                    .await?;
                let mut paths = Self::sync_lookup_path();
                paths.extend(toolset.list_paths(&config).await);
                let path =
                    join_paths(&paths).wrap_err("failed to construct package plugin PATH")?;
                let mut env: IndexMap<String, String> = std::env::vars().collect();
                env.insert("PATH".into(), path.to_string_lossy().into_owned());
                Ok(env)
            })
            .await
    }

    async fn checked_hook_env(&self) -> Result<&IndexMap<String, String>> {
        let env = self.hook_env().await?;
        let paths = env
            .get("PATH")
            .map(|path| split_paths(path).collect::<Vec<_>>())
            .unwrap_or_default();
        if let Some(binary) = self.missing_from_path(&paths) {
            bail!(
                "{} is not available: required binary '{binary}' not found; add it to [tools] or install it manually",
                self.name
            );
        }
        Ok(env)
    }

    fn requests(pkgs: &[PackageRequest]) -> Vec<VfoxPackageRequest> {
        pkgs.iter()
            .map(|pkg| VfoxPackageRequest {
                name: pkg.name.clone(),
                version: pkg.version.clone(),
            })
            .collect()
    }

    fn vfox(&self, env: &IndexMap<String, String>) -> Result<vfox::Vfox> {
        let (mut vfox, _) = self.plugin.vfox()?;
        vfox.cmd_env = Some(env.clone());
        Ok(vfox)
    }

    async fn action(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
        upgrade: bool,
    ) -> Result<()> {
        let env = self.checked_hook_env().await?;
        let vfox = self.vfox(env)?;
        let ctx = PackageActionContext {
            packages: Self::requests(pkgs),
            dry_run: opts.dry_run,
            update: opts.update,
        };
        if upgrade
            && self
                .plugin
                .plugin_path
                .join("hooks/package_upgrade.lua")
                .exists()
        {
            vfox.package_upgrade(&self.name, ctx).await?;
        } else {
            vfox.package_install(&self.name, ctx).await?;
        }
        Ok(())
    }
}

#[async_trait(?Send)]
impl SystemPackageManager for PackagePluginManager {
    fn name(&self) -> &str {
        &self.name
    }

    fn is_available(&self) -> bool {
        self.platform_available() && self.missing_from_path(&Self::sync_lookup_path()).is_none()
    }

    fn unavailable_reason(&self) -> String {
        if !self.platform_available() {
            return format!("not available on {}", crate::config::Settings::get().os());
        }
        match self.missing_from_path(&Self::sync_lookup_path()) {
            Some(binary) => format!(
                "required binary '{binary}' not found; add it to [tools] or install it manually"
            ),
            None => "unavailable".to_string(),
        }
    }

    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let env = self.checked_hook_env().await?;
        let response = self
            .vfox(env)?
            .package_installed(
                &self.name,
                PackageInstalledContext {
                    packages: Self::requests(pkgs),
                },
            )
            .await?;
        let by_name: std::collections::HashMap<_, _> = response
            .packages
            .into_iter()
            .map(|pkg| (pkg.name.clone(), pkg))
            .collect();
        pkgs.iter()
            .map(|request| {
                let returned = by_name.get(&request.name);
                let state = match returned {
                    Some(pkg) if pkg.state == "installed" => {
                        let installed = pkg.version.clone().unwrap_or_default();
                        match &request.version {
                            Some(requested) if requested != &installed => {
                                PackageState::VersionMismatch { installed }
                            }
                            _ => PackageState::Installed { version: installed },
                        }
                    }
                    Some(pkg) if pkg.state == "missing" => PackageState::Missing,
                    Some(pkg) => bail!(
                        "{} package hook returned invalid state '{}' for '{}'",
                        self.name,
                        pkg.state,
                        request.name
                    ),
                    None => bail!(
                        "{} package hook did not return state for '{}'",
                        self.name,
                        request.name
                    ),
                };
                Ok(PackageStatus {
                    request: request.clone(),
                    state,
                })
            })
            .collect()
    }

    async fn install(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.action(pkgs, opts, false).await
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        self.action(pkgs, opts, true).await
    }

    fn supports_version_pins(&self) -> bool {
        self.config.supports_version_pins
    }

    fn is_plugin(&self) -> bool {
        true
    }
}
