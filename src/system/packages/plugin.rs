use std::collections::{BTreeMap, HashSet};
use std::env::{join_paths, split_paths};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use eyre::{WrapErr, bail, eyre};
use heck::ToKebabCase;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use vfox::{
    PackageActionContext, PackageInstalledContext, PackageRequest as VfoxPackageRequest,
    PackageUninstallContext,
};

use super::{InstallOpts, PackageRequest, PackageState, PackageStatus, SystemPackageManager};
use crate::config::Config;
use crate::plugins::mise_plugin_toml::{MisePluginToml, MisePluginTomlPackageManagerConfig};
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::result::Result;
use crate::toolset::{ConfigScope, ToolsetBuilder};

const STATE_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct PackagePluginState {
    schema_version: u8,
    manager: String,
    packages: BTreeMap<String, OwnedPackage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct OwnedPackage {
    version: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PluginPrunePlan {
    pub remove: Vec<PackageRequest>,
    stale: Vec<String>,
}

impl PluginPrunePlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.remove.is_empty()
    }
}

impl PackagePluginState {
    fn prune_requests(&self, configured: &[PackageRequest]) -> Vec<PackageRequest> {
        let keep = configured
            .iter()
            .map(|request| request.name.as_str())
            .collect::<HashSet<_>>();
        self.packages
            .iter()
            .filter(|(name, _)| !keep.contains(name.as_str()))
            .map(|(name, owned)| PackageRequest {
                name: name.clone(),
                version: owned.version.clone(),
                tap_url: None,
            })
            .collect()
    }

    fn approved_prune_requests(
        &self,
        approved: &[PackageRequest],
        configured: &[PackageRequest],
    ) -> Vec<PackageRequest> {
        let keep = configured
            .iter()
            .map(|request| request.name.as_str())
            .collect::<HashSet<_>>();
        approved
            .iter()
            .filter(|request| {
                self.packages.contains_key(&request.name) && !keep.contains(request.name.as_str())
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug)]
pub(crate) struct PackagePluginManager {
    name: String,
    plugin: Arc<VfoxPlugin>,
    config: MisePluginTomlPackageManagerConfig,
    hook_env: OnceCell<IndexMap<String, String>>,
}

impl PackagePluginManager {
    pub(crate) fn new(name: String) -> Result<Self> {
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

    fn state_path_for(name: &str) -> PathBuf {
        crate::dirs::STATE
            .join("package-plugins")
            .join(format!("{}.json", crate::hash::hash_sha256_to_str(name)))
    }

    fn state_path(&self) -> PathBuf {
        Self::state_path_for(&self.name)
    }

    fn operation_lock(&self) -> Result<Option<fslock::LockFile>> {
        crate::lock_file::get(&self.state_path(), false)
    }

    fn load_state_at(path: &std::path::Path, manager: &str) -> Result<PackagePluginState> {
        if !path.exists() {
            return Ok(PackagePluginState {
                schema_version: STATE_SCHEMA_VERSION,
                manager: manager.to_string(),
                packages: BTreeMap::new(),
            });
        }
        let state: PackagePluginState = serde_json::from_str(&crate::file::read_to_string(path)?)
            .wrap_err_with(|| {
            format!(
                "failed to read package plugin ownership state {}",
                crate::file::display_path(path)
            )
        })?;
        if state.schema_version != STATE_SCHEMA_VERSION {
            bail!(
                "unsupported package plugin ownership state version {} in {}",
                state.schema_version,
                crate::file::display_path(path)
            );
        }
        if state.manager != manager {
            bail!(
                "package plugin ownership state {} belongs to manager '{}', not '{manager}'",
                crate::file::display_path(path),
                state.manager
            );
        }
        Ok(state)
    }

    fn load_state(&self) -> Result<PackagePluginState> {
        Self::load_state_at(&self.state_path(), &self.name)
    }

    fn save_state_at(path: &std::path::Path, state: &PackagePluginState) -> Result<()> {
        crate::file::create_dir_all(path.parent().expect("package plugin state parent"))?;
        let mut contents = serde_json::to_vec_pretty(state)?;
        contents.push(b'\n');
        crate::file::write_atomic(path, contents)
    }

    fn save_state(&self, state: &PackagePluginState) -> Result<()> {
        Self::save_state_at(&self.state_path(), state)
    }

    pub(crate) fn supports_uninstall(&self) -> bool {
        self.plugin
            .plugin_path
            .join("hooks/package_uninstall.lua")
            .exists()
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
                let mut env: IndexMap<String, String> = crate::env::vars_safe().collect();
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

    async fn uninstall_action(&self, pkgs: &[PackageRequest]) -> Result<()> {
        let env = self.checked_hook_env().await?;
        let ctx = PackageUninstallContext {
            packages: Self::requests(pkgs),
        };
        self.vfox(env)?.package_uninstall(&self.name, ctx).await?;
        Ok(())
    }

    fn status_version(status: &PackageStatus) -> Option<String> {
        match &status.state {
            PackageState::Installed { version }
            | PackageState::NeedsRepair { installed: version }
            | PackageState::VersionMismatch { installed: version } => Some(version.clone()),
            #[cfg(unix)]
            PackageState::InstalledAutoUpdates { version } => Some(version.clone()),
            PackageState::Missing => None,
            #[cfg(unix)]
            PackageState::Unavailable { .. } => None,
        }
    }

    fn reconcile_installed_ownership(
        &self,
        state: &mut PackagePluginState,
        before: &[PackageStatus],
        after: &[PackageStatus],
    ) -> Result<()> {
        let missing = before
            .iter()
            .filter(|status| matches!(status.state, PackageState::Missing))
            .map(|status| status.request.name.as_str())
            .collect::<HashSet<_>>();
        for status in after {
            if (missing.contains(status.request.name.as_str())
                || state.packages.contains_key(&status.request.name))
                && let Some(version) = Self::status_version(status)
            {
                state.packages.insert(
                    status.request.name.clone(),
                    OwnedPackage {
                        version: Some(version),
                    },
                );
            }
        }
        self.save_state(state)
    }

    fn reconcile_owned_versions(state: &mut PackagePluginState, after: &[PackageStatus]) -> bool {
        let mut changed = false;
        for status in after {
            if let Some(version) = Self::status_version(status)
                && let Some(owned) = state.packages.get_mut(&status.request.name)
            {
                let version = Some(version);
                changed |= owned.version != version;
                owned.version = version;
            }
        }
        changed
    }

    fn reconcile_missing_ownership(
        state: &mut PackagePluginState,
        statuses: &[PackageStatus],
    ) -> bool {
        let mut changed = false;
        for status in statuses {
            if matches!(status.state, PackageState::Missing) {
                changed |= state.packages.remove(&status.request.name).is_some();
            }
        }
        changed
    }

    pub(crate) async fn prune_plan(
        &self,
        configured: &[PackageRequest],
    ) -> Result<PluginPrunePlan> {
        let _lock = self.operation_lock()?;
        let state = self.load_state()?;
        let requests = state.prune_requests(configured);
        if requests.is_empty() {
            return Ok(PluginPrunePlan {
                remove: vec![],
                stale: vec![],
            });
        }
        let statuses = self.installed(&requests).await?;
        let mut remove = vec![];
        let mut stale = vec![];
        for status in statuses {
            match Self::status_version(&status) {
                Some(version) => remove.push(PackageRequest {
                    name: status.request.name,
                    version: Some(version),
                    tap_url: None,
                }),
                None if matches!(status.state, PackageState::Missing) => {
                    stale.push(status.request.name);
                }
                None => {}
            }
        }
        Ok(PluginPrunePlan { remove, stale })
    }

    pub(crate) async fn apply_prune_plan(&self, plan: &PluginPrunePlan) -> Result<usize> {
        let _lock = self.operation_lock()?;
        let config = Config::reset().await?;
        let configured =
            crate::system::package_requests_for_manager_from_config_and_tracked_config_files(
                &config, &self.name,
            )
            .await?;
        let mut state = self.load_state()?;
        let mut state_changed = false;
        let stale_requests = plan
            .stale
            .iter()
            .filter_map(|name| {
                state.packages.get(name).map(|owned| PackageRequest {
                    name: name.clone(),
                    version: owned.version.clone(),
                    tap_url: None,
                })
            })
            .collect::<Vec<_>>();
        if !stale_requests.is_empty() {
            let stale_statuses = self.installed(&stale_requests).await?;
            state_changed |= Self::reconcile_missing_ownership(&mut state, &stale_statuses);
        }
        // The confirmed plan is an upper bound. Configuration may have changed
        // while the confirmation prompt was open, so never remove an approved
        // package that is now part of the desired set.
        let requests = state.approved_prune_requests(&plan.remove, &configured);
        if requests.is_empty() {
            if state_changed {
                self.save_state(&state)?;
            }
            return Ok(0);
        }
        let before = self.installed(&requests).await?;
        let present = before
            .iter()
            .filter_map(|status| {
                Self::status_version(status).map(|version| PackageRequest {
                    name: status.request.name.clone(),
                    version: Some(version),
                    tap_url: None,
                })
            })
            .collect::<Vec<_>>();
        Self::reconcile_missing_ownership(&mut state, &before);
        self.save_state(&state)?;
        if present.is_empty() {
            return Ok(0);
        }
        if !self.supports_uninstall() {
            bail!(
                "package plugin '{}' does not support uninstall; add hooks/package_uninstall.lua",
                self.name
            );
        }

        let action_result = self.uninstall_action(&present).await;
        let after = self.installed(&present).await;
        let after = match after {
            Ok(after) => after,
            Err(status_err) => {
                if let Err(action_err) = action_result {
                    warn!(
                        "{}: failed to verify uninstall after error: {status_err:#}",
                        self.name
                    );
                    return Err(action_err);
                }
                return Err(status_err);
            }
        };
        let mut removed = 0;
        let mut remaining = vec![];
        for status in after {
            if matches!(status.state, PackageState::Missing) {
                if state.packages.remove(&status.request.name).is_some() {
                    removed += 1;
                }
            } else {
                remaining.push(status.request.name);
            }
        }
        self.save_state(&state)?;
        action_result?;
        if !remaining.is_empty() {
            return Err(eyre!(
                "package plugin '{}' did not uninstall: {}",
                self.name,
                remaining.join(", ")
            ));
        }
        Ok(removed)
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

    async fn unavailable_reason_async(&self) -> Option<String> {
        if !self.platform_available() {
            return Some(format!(
                "not available on {}",
                crate::config::Settings::get().os()
            ));
        }
        let env = match self.hook_env().await {
            Ok(env) => env,
            Err(err) => return Some(format!("failed to resolve host tool PATH: {err:#}")),
        };
        let paths = env
            .get("PATH")
            .map(|path| split_paths(path).collect::<Vec<_>>())
            .unwrap_or_default();
        self.missing_from_path(&paths).map(|binary| {
            format!(
                "required binary '{binary}' not found; add it to [tools] or install it manually"
            )
        })
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
        if opts.dry_run {
            return self.action(pkgs, opts, false).await;
        }
        let _lock = self.operation_lock()?;
        let mut state = self.load_state()?;
        let before = self.installed(pkgs).await?;
        let action_result = self.action(pkgs, opts, false).await;
        let after = self.installed(pkgs).await;
        match after {
            Ok(after) => {
                if let Err(state_err) =
                    self.reconcile_installed_ownership(&mut state, &before, &after)
                {
                    if let Err(action_err) = action_result {
                        warn!(
                            "{}: failed to save ownership after install error: {state_err:#}",
                            self.name
                        );
                        return Err(action_err);
                    }
                    return Err(state_err);
                }
            }
            Err(status_err) => {
                if let Err(action_err) = action_result {
                    warn!(
                        "{}: failed to verify ownership after install error: {status_err:#}",
                        self.name
                    );
                    return Err(action_err);
                }
                return Err(status_err);
            }
        }
        action_result
    }

    async fn upgrade(&self, pkgs: &[PackageRequest], opts: &InstallOpts) -> Result<()> {
        if opts.dry_run {
            return self.action(pkgs, opts, true).await;
        }
        let _lock = self.operation_lock()?;
        let mut state = self.load_state()?;
        let action_result = self.action(pkgs, opts, true).await;
        let after = self.installed(pkgs).await;
        match after {
            Ok(after) => {
                if Self::reconcile_owned_versions(&mut state, &after)
                    && let Err(state_err) = self.save_state(&state)
                {
                    if let Err(action_err) = action_result {
                        warn!(
                            "{}: failed to save ownership after upgrade error: {state_err:#}",
                            self.name
                        );
                        return Err(action_err);
                    }
                    return Err(state_err);
                }
            }
            Err(status_err) => {
                if let Err(action_err) = action_result {
                    warn!(
                        "{}: failed to verify ownership after upgrade error: {status_err:#}",
                        self.name
                    );
                    return Err(action_err);
                }
                return Err(status_err);
            }
        }
        action_result
    }

    fn supports_version_pins(&self) -> bool {
        self.config.supports_version_pins
    }

    fn is_plugin(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(manager: &str) -> PackagePluginState {
        PackagePluginState {
            schema_version: STATE_SCHEMA_VERSION,
            manager: manager.to_string(),
            packages: BTreeMap::from([
                (
                    "keep".to_string(),
                    OwnedPackage {
                        version: Some("nightly-2026.07".to_string()),
                    },
                ),
                (
                    "remove".to_string(),
                    OwnedPackage {
                        version: Some("release:edge".to_string()),
                    },
                ),
            ]),
        }
    }

    #[test]
    fn ownership_state_round_trips_atomically() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("state/manager.json");
        let expected = state("fake");
        PackagePluginManager::save_state_at(&path, &expected)?;
        assert_eq!(
            PackagePluginManager::load_state_at(&path, "fake")?,
            expected
        );
        Ok(())
    }

    #[test]
    fn ownership_state_rejects_invalid_json_and_schema() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("manager.json");
        crate::file::write(&path, b"not json")?;
        assert!(PackagePluginManager::load_state_at(&path, "fake").is_err());

        let mut invalid = state("fake");
        invalid.schema_version += 1;
        PackagePluginManager::save_state_at(&path, &invalid)?;
        assert!(PackagePluginManager::load_state_at(&path, "fake").is_err());
        Ok(())
    }

    #[test]
    fn prune_candidates_match_names_and_preserve_opaque_versions() {
        let configured = vec![PackageRequest {
            name: "keep".to_string(),
            version: Some("a-different-pin".to_string()),
            tap_url: None,
        }];
        assert_eq!(
            state("fake").prune_requests(&configured),
            vec![PackageRequest {
                name: "remove".to_string(),
                version: Some("release:edge".to_string()),
                tap_url: None,
            }]
        );
    }

    #[test]
    fn approved_prune_candidates_can_only_shrink_after_config_revalidation() {
        let state = state("fake");
        let approved = vec![
            PackageRequest {
                name: "remove".to_string(),
                version: Some("release:edge".to_string()),
                tap_url: None,
            },
            PackageRequest {
                name: "not-owned".to_string(),
                version: None,
                tap_url: None,
            },
        ];
        let configured = vec![PackageRequest {
            name: "remove".to_string(),
            version: Some("newly-declared".to_string()),
            tap_url: None,
        }];

        assert_eq!(
            state.approved_prune_requests(&approved, &[]),
            vec![approved[0].clone()]
        );
        assert!(
            state
                .approved_prune_requests(&approved, &configured)
                .is_empty()
        );
    }

    #[test]
    fn upgrade_reconciliation_updates_only_owned_versions() {
        let mut state = state("fake");
        let statuses = vec![
            PackageStatus {
                request: PackageRequest {
                    name: "keep".to_string(),
                    version: Some("nightly-2026.08".to_string()),
                    tap_url: None,
                },
                state: PackageState::Installed {
                    version: "nightly-2026.08".to_string(),
                },
            },
            PackageStatus {
                request: PackageRequest {
                    name: "manual".to_string(),
                    version: None,
                    tap_url: None,
                },
                state: PackageState::Installed {
                    version: "release:edge".to_string(),
                },
            },
        ];

        assert!(PackagePluginManager::reconcile_owned_versions(
            &mut state, &statuses
        ));
        assert_eq!(
            state.packages["keep"].version.as_deref(),
            Some("nightly-2026.08")
        );
        assert!(!state.packages.contains_key("manual"));
    }

    #[test]
    fn stale_reconciliation_rechecks_status_before_dropping_ownership() {
        let mut state = state("fake");
        let statuses = vec![
            PackageStatus {
                request: PackageRequest {
                    name: "keep".to_string(),
                    version: Some("nightly-2026.07".to_string()),
                    tap_url: None,
                },
                state: PackageState::Installed {
                    version: "nightly-2026.07".to_string(),
                },
            },
            PackageStatus {
                request: PackageRequest {
                    name: "remove".to_string(),
                    version: Some("release:edge".to_string()),
                    tap_url: None,
                },
                state: PackageState::Missing,
            },
        ];

        assert!(PackagePluginManager::reconcile_missing_ownership(
            &mut state, &statuses
        ));
        assert!(state.packages.contains_key("keep"));
        assert!(!state.packages.contains_key("remove"));
    }
}
