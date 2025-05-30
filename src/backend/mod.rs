use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as TokioMutex;

use crate::cli::args::{BackendArg, ToolVersionType};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, Settings};
use crate::file::{display_path, remove_all, remove_all_with_warning};
use crate::install_context::InstallContext;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::{PluginType, VERSION_REGEX};
use crate::registry::{REGISTRY, tool_enabled};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::outdated_info::OutdatedInfo;
use crate::toolset::{ToolRequest, ToolVersion, Toolset, install_state, is_outdated_version};
use crate::ui::progress_report::SingleReport;
use crate::{
    cache::{CacheManager, CacheManagerBuilder},
    plugins::PluginEnum,
};
use crate::{dirs, env, file, hash, lock_file, plugins, versions_host};
use async_trait::async_trait;
use backend_type::BackendType;
use console::style;
use eyre::{Result, WrapErr, bail, eyre};
use indexmap::IndexSet;
use itertools::Itertools;
use regex::Regex;
use std::sync::LazyLock as Lazy;

pub mod aqua;
pub mod asdf;
pub mod backend_type;
pub mod cargo;
pub mod dotnet;
mod external_plugin_cache;
pub mod gem;
pub mod go;
pub mod npm;
pub mod pipx;
pub mod spm;
pub mod ubi;
pub mod vfox;

pub type ABackend = Arc<dyn Backend>;
pub type BackendMap = BTreeMap<String, ABackend>;
pub type BackendList = Vec<ABackend>;
pub type VersionCacheManager = CacheManager<Vec<String>>;

static TOOLS: Mutex<Option<Arc<BackendMap>>> = Mutex::new(None);

pub async fn load_tools() -> Result<Arc<BackendMap>> {
    if let Some(memo_tools) = TOOLS.lock().unwrap().clone() {
        return Ok(memo_tools);
    }
    install_state::init().await?;
    time!("load_tools start");
    let core_tools = CORE_PLUGINS.values().cloned().collect::<Vec<ABackend>>();
    let mut tools = core_tools;
    // add tools with idiomatic files so they get parsed even if no versions are installed
    tools.extend(
        REGISTRY
            .values()
            .filter(|rt| !rt.idiomatic_files.is_empty() && rt.is_supported_os())
            .filter_map(|rt| arg_to_backend(rt.short.into())),
    );
    time!("load_tools core");
    tools.extend(
        install_state::list_tools()
            .values()
            .filter(|ist| ist.full.is_some())
            .flat_map(|ist| arg_to_backend(ist.clone().into())),
    );
    time!("load_tools install_state");
    tools.retain(|backend| {
        tool_enabled(
            &Settings::get().enable_tools(),
            &Settings::get().disable_tools(),
            &backend.id().to_string(),
        )
    });
    tools.retain(|backend| {
        !Settings::get()
            .disable_backends
            .contains(&backend.get_type().to_string())
    });

    let tools: BackendMap = tools
        .into_iter()
        .map(|backend| (backend.ba().short.clone(), backend))
        .collect();
    let tools = Arc::new(tools);
    *TOOLS.lock().unwrap() = Some(tools.clone());
    time!("load_tools done");
    Ok(tools)
}

pub fn list() -> BackendList {
    TOOLS
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .values()
        .cloned()
        .collect()
}

pub fn get(ba: &BackendArg) -> Option<ABackend> {
    let mut tools = TOOLS.lock().unwrap();
    let tools_ = tools.as_ref().unwrap();
    if let Some(backend) = tools_.get(&ba.short) {
        Some(backend.clone())
    } else if let Some(backend) = arg_to_backend(ba.clone()) {
        let mut tools_ = tools_.deref().clone();
        tools_.insert(ba.short.clone(), backend.clone());
        *tools = Some(Arc::new(tools_));
        Some(backend)
    } else {
        None
    }
}

pub fn remove(short: &str) {
    let mut tools = TOOLS.lock().unwrap();
    let mut tools_ = tools.as_ref().unwrap().deref().clone();
    tools_.remove(short);
    *tools = Some(Arc::new(tools_));
}

pub fn arg_to_backend(ba: BackendArg) -> Option<ABackend> {
    match ba.backend_type() {
        BackendType::Core => {
            CORE_PLUGINS
                .get(&ba.short)
                .or_else(|| {
                    // this can happen if something like "corenode" is aliased to "core:node"
                    ba.full()
                        .strip_prefix("core:")
                        .and_then(|short| CORE_PLUGINS.get(short))
                })
                .cloned()
        }
        BackendType::Aqua => Some(Arc::new(aqua::AquaBackend::from_arg(ba))),
        BackendType::Asdf => Some(Arc::new(asdf::AsdfBackend::from_arg(ba))),
        BackendType::Cargo => Some(Arc::new(cargo::CargoBackend::from_arg(ba))),
        BackendType::Dotnet => Some(Arc::new(dotnet::DotnetBackend::from_arg(ba))),
        BackendType::Npm => Some(Arc::new(npm::NPMBackend::from_arg(ba))),
        BackendType::Gem => Some(Arc::new(gem::GemBackend::from_arg(ba))),
        BackendType::Go => Some(Arc::new(go::GoBackend::from_arg(ba))),
        BackendType::Pipx => Some(Arc::new(pipx::PIPXBackend::from_arg(ba))),
        BackendType::Spm => Some(Arc::new(spm::SPMBackend::from_arg(ba))),
        BackendType::Ubi => Some(Arc::new(ubi::UbiBackend::from_arg(ba))),
        BackendType::Vfox => Some(Arc::new(vfox::VfoxBackend::from_arg(ba))),
        BackendType::Unknown => None,
    }
}

#[async_trait]
pub trait Backend: Debug + Send + Sync {
    fn id(&self) -> &str {
        &self.ba().short
    }
    fn tool_name(&self) -> String {
        self.ba().tool_name()
    }
    fn get_type(&self) -> BackendType {
        BackendType::Core
    }
    fn ba(&self) -> &Arc<BackendArg>;
    async fn description(&self) -> Option<String> {
        None
    }
    fn get_plugin_type(&self) -> Option<PluginType> {
        None
    }
    /// If any of these tools are installing in parallel, we should wait for them to finish
    /// before installing this tool.
    fn get_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }
    /// dependencies which wait for install but do not warn, like cargo-binstall
    fn get_optional_dependencies(&self) -> Result<Vec<&str>> {
        Ok(vec![])
    }
    fn get_all_dependencies(&self, optional: bool) -> Result<IndexSet<BackendArg>> {
        let all_fulls = self.ba().all_fulls();
        if all_fulls.is_empty() {
            // this can happen on windows where we won't be able to install this os/arch so
            // the fact there might be dependencies is meaningless
            return Ok(Default::default());
        }
        let mut deps: Vec<&str> = self.get_dependencies()?;
        if optional {
            deps.extend(self.get_optional_dependencies()?);
        }
        let mut deps: IndexSet<_> = deps.into_iter().map(BackendArg::from).collect();
        if let Some(rt) = REGISTRY.get(self.ba().short.as_str()) {
            // add dependencies from registry.toml
            deps.extend(rt.depends.iter().map(BackendArg::from));
        }
        deps.retain(|ba| &**self.ba() != ba);
        deps.retain(|ba| !all_fulls.contains(&ba.full()));
        for ba in deps.clone() {
            if let Ok(backend) = ba.backend() {
                deps.extend(backend.get_all_dependencies(optional)?);
            }
        }
        Ok(deps)
    }

    async fn list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>> {
        let remote_versions = self.get_remote_version_cache();
        let remote_versions = remote_versions.lock().await;
        let ba = self.ba().clone();
        let id = self.id();
        let versions = remote_versions
            .get_or_try_init_async(|| async {
                trace!("Listing remote versions for {}", ba.to_string());
                match versions_host::list_versions(&ba).await {
                    Ok(Some(versions)) => return Ok(versions),
                    Ok(None) => {}
                    Err(e) => {
                        debug!("Error getting versions from versions host: {:#}", e);
                    }
                };
                trace!(
                    "Calling backend to list remote versions for {}",
                    ba.to_string()
                );
                let versions = self
                    ._list_remote_versions(config)
                    .await?
                    .into_iter()
                    .filter(|v| match v.parse::<ToolVersionType>() {
                        Ok(ToolVersionType::Version(_)) => true,
                        _ => {
                            warn!("Invalid version: {id}@{v}");
                            false
                        }
                    })
                    .collect_vec();
                if versions.is_empty() {
                    warn!("No versions found for {id}");
                }
                Ok(versions)
            })
            .await?;
        Ok(versions.clone())
    }
    async fn _list_remote_versions(&self, config: &Arc<Config>) -> eyre::Result<Vec<String>>;
    async fn latest_stable_version(&self, config: &Arc<Config>) -> eyre::Result<Option<String>> {
        self.latest_version(config, Some("latest".into())).await
    }
    fn list_installed_versions(&self) -> Vec<String> {
        install_state::list_versions(&self.ba().short)
    }
    fn is_version_installed(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        check_symlink: bool,
    ) -> bool {
        let check_path = |install_path: &Path, check_symlink: bool| {
            let is_installed = install_path.exists();
            let is_not_incomplete = !self.incomplete_file_path(tv).exists();
            let is_valid_symlink = !check_symlink || !is_runtime_symlink(install_path);

            let installed = is_installed && is_not_incomplete && is_valid_symlink;
            if log::log_enabled!(log::Level::Trace) && !installed {
                let mut msg = format!(
                    "{} is not installed, path: {}",
                    self.ba(),
                    display_path(install_path)
                );
                if !is_installed {
                    msg += " (not installed)";
                }
                if !is_not_incomplete {
                    msg += " (incomplete)";
                }
                if !is_valid_symlink {
                    msg += " (runtime symlink)";
                }
                trace!("{}", msg);
            }
            installed
        };
        match tv.request {
            ToolRequest::System { .. } => true,
            _ => {
                if let Some(install_path) = tv.request.install_path(config) {
                    if check_path(&install_path, true) {
                        return true;
                    }
                }
                check_path(&tv.install_path(), check_symlink)
            }
        }
    }
    async fn is_version_outdated(&self, config: &Arc<Config>, tv: &ToolVersion) -> bool {
        let latest = match tv.latest_version(config).await {
            Ok(latest) => latest,
            Err(e) => {
                warn!(
                    "Error getting latest version for {}: {:#}",
                    self.ba().to_string(),
                    e
                );
                return false;
            }
        };
        !self.is_version_installed(config, tv, true) || is_outdated_version(&tv.version, &latest)
    }
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        match tv.install_path() {
            path if path.is_symlink() && !is_runtime_symlink(&path) => Some(path),
            _ => None,
        }
    }
    fn create_symlink(&self, version: &str, target: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
        let link = self.ba().installs_path.join(version);
        if link.exists() {
            return Ok(None);
        }
        file::create_dir_all(link.parent().unwrap())?;
        let link = file::make_symlink(target, &link)?;
        Ok(Some(link))
    }
    fn list_installed_versions_matching(&self, query: &str) -> Vec<String> {
        let versions = self.list_installed_versions();
        self.fuzzy_match_filter(versions, query)
    }
    async fn list_versions_matching(
        &self,
        config: &Arc<Config>,
        query: &str,
    ) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions(config).await?;
        Ok(self.fuzzy_match_filter(versions, query))
    }
    async fn latest_version(
        &self,
        config: &Arc<Config>,
        query: Option<String>,
    ) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let mut matches = self.list_versions_matching(config, &query).await?;
                if matches.is_empty() && query == "latest" {
                    matches = self.list_remote_versions(config).await?;
                }
                Ok(find_match_in_list(&matches, &query))
            }
            None => self.latest_stable_version(config).await,
        }
    }
    fn latest_installed_version(&self, query: Option<String>) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query);
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.ba().installs_path.join("latest");
                if installed_symlink.exists() {
                    let Some(target) = file::resolve_symlink(&installed_symlink)? else {
                        return Ok(Some("latest".to_string()));
                    };
                    let version = target
                        .file_name()
                        .ok_or_else(|| eyre!("Invalid symlink target"))?
                        .to_string_lossy()
                        .to_string();
                    Ok(Some(version))
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn warn_if_dependencies_missing(&self, config: &Arc<Config>) -> eyre::Result<()> {
        let deps = self
            .get_all_dependencies(false)?
            .into_iter()
            .filter(|ba| &**self.ba() != ba)
            .map(|ba| ba.short)
            .collect::<HashSet<_>>();
        if !deps.is_empty() {
            trace!("Ensuring dependencies installed for {}", self.id());
            let ts = config.get_tool_request_set().await?.filter_by_tool(deps);
            let missing = ts.missing_tools(config).await;
            if !missing.is_empty() {
                warn_once!(
                    "missing dependency: {}",
                    missing.iter().map(|d| d.to_string()).join(", "),
                );
            }
        }
        Ok(())
    }
    fn purge(&self, pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
        rmdir(&self.ba().installs_path, pr)?;
        rmdir(&self.ba().cache_path, pr)?;
        rmdir(&self.ba().downloads_path, pr)?;
        Ok(())
    }
    fn get_aliases(&self) -> eyre::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }
    fn idiomatic_filenames(&self) -> Result<Vec<String>> {
        Ok(REGISTRY
            .get(self.id())
            .map(|rt| rt.idiomatic_files.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default())
    }
    fn parse_idiomatic_file(&self, path: &Path) -> eyre::Result<String> {
        let contents = file::read_to_string(path)?;
        Ok(contents.trim().to_string())
    }
    fn plugin(&self) -> Option<&PluginEnum> {
        None
    }

    async fn install_version(
        &self,
        ctx: InstallContext,
        tv: ToolVersion,
    ) -> eyre::Result<ToolVersion> {
        if let Some(plugin) = self.plugin() {
            plugin.is_installed_err()?;
        }
        if self.is_version_installed(&ctx.config, &tv, true) {
            if ctx.force {
                self.uninstall_version(&ctx.config, &tv, &ctx.pr, false)
                    .await?;
            } else {
                return Ok(tv);
            }
        }
        ctx.pr.set_message("install".into());
        let _lock = lock_file::get(&tv.install_path(), ctx.force)?;
        self.create_install_dirs(&tv)?;

        let old_tv = tv.clone();
        let tv = match self.install_version_(&ctx, tv).await {
            Ok(tv) => tv,
            Err(e) => {
                self.cleanup_install_dirs_on_error(&old_tv);
                return Err(e);
            }
        };

        if tv.install_path().starts_with(*dirs::INSTALLS) {
            // this will be false only for `install-into`
            install_state::write_backend_meta(self.ba())?;
        }

        self.cleanup_install_dirs(&tv);
        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::DATA.to_path_buf()];
        touch_dirs.extend(ctx.config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                trace!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = file::remove_file(self.incomplete_file_path(&tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
        if let Some(script) = tv.request.options().get("postinstall") {
            ctx.pr
                .finish_with_message("running custom postinstall hook".to_string());
            self.run_postinstall_hook(&ctx, &tv, script).await?;
        }
        ctx.pr.finish_with_message("installed".to_string());

        Ok(tv)
    }

    async fn run_postinstall_hook(
        &self,
        ctx: &InstallContext,
        tv: &ToolVersion,
        script: &str,
    ) -> eyre::Result<()> {
        CmdLineRunner::new(&*env::SHELL)
            .env(&*env::PATH_KEY, plugins::core::path_env_with_tv_path(tv)?)
            .with_pr(&ctx.pr)
            .arg("-c")
            .arg(script)
            .envs(self.exec_env(&ctx.config, &ctx.ts, tv).await?)
            .execute()?;
        Ok(())
    }
    async fn install_version_(&self, ctx: &InstallContext, tv: ToolVersion) -> Result<ToolVersion>;
    async fn uninstall_version(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        pr: &Box<dyn SingleReport>,
        dryrun: bool,
    ) -> eyre::Result<()> {
        pr.set_message("uninstall".into());

        if !dryrun {
            self.uninstall_version_impl(config, pr, tv).await?;
        }
        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("remove {}", display_path(dir)));
            if dryrun {
                return Ok(());
            }
            remove_all_with_warning(dir)
        };
        rmdir(&tv.install_path())?;
        if !Settings::get().always_keep_download {
            rmdir(&tv.download_path())?;
        }
        rmdir(&tv.cache_path())?;
        Ok(())
    }
    async fn uninstall_version_impl(
        &self,
        _config: &Arc<Config>,
        _pr: &Box<dyn SingleReport>,
        _tv: &ToolVersion,
    ) -> Result<()> {
        Ok(())
    }
    async fn list_bin_paths(
        &self,
        _config: &Arc<Config>,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        match tv.request {
            ToolRequest::System { .. } => Ok(vec![]),
            _ => Ok(vec![tv.install_path().join("bin")]),
        }
    }

    async fn exec_env(
        &self,
        _config: &Arc<Config>,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    async fn which(
        &self,
        config: &Arc<Config>,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> eyre::Result<Option<PathBuf>> {
        let bin_paths = self
            .list_bin_paths(config, tv)
            .await?
            .into_iter()
            .filter(|p| p.parent().is_some());
        for bin_path in bin_paths {
            let paths_with_ext = if cfg!(windows) {
                vec![
                    bin_path.clone(),
                    bin_path.join(bin_name).with_extension("exe"),
                    bin_path.join(bin_name).with_extension("cmd"),
                    bin_path.join(bin_name).with_extension("bat"),
                    bin_path.join(bin_name).with_extension("ps1"),
                ]
            } else {
                vec![bin_path.join(bin_name)]
            };
            for bin_path in paths_with_ext {
                if bin_path.exists() && file::is_executable(&bin_path) {
                    return Ok(Some(bin_path));
                }
            }
        }
        Ok(None)
    }

    fn create_install_dirs(&self, tv: &ToolVersion) -> eyre::Result<()> {
        let _ = remove_all_with_warning(tv.install_path());
        if !Settings::get().always_keep_download {
            let _ = remove_all_with_warning(tv.download_path());
        }
        let _ = remove_all_with_warning(tv.cache_path());
        let _ = file::remove_file(tv.install_path()); // removes if it is a symlink
        file::create_dir_all(tv.install_path())?;
        file::create_dir_all(tv.download_path())?;
        file::create_dir_all(tv.cache_path())?;
        File::create(self.incomplete_file_path(tv))?;
        Ok(())
    }
    fn cleanup_install_dirs_on_error(&self, tv: &ToolVersion) {
        if !Settings::get().always_keep_install {
            let _ = remove_all_with_warning(tv.install_path());
            self.cleanup_install_dirs(tv);
        }
    }
    fn cleanup_install_dirs(&self, tv: &ToolVersion) {
        if !Settings::get().always_keep_download {
            let _ = remove_all_with_warning(tv.download_path());
        }
    }
    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        install_state::incomplete_file_path(&tv.ba().short, &tv.tv_pathname())
    }

    async fn path_env_for_cmd(&self, config: &Arc<Config>, tv: &ToolVersion) -> Result<OsString> {
        let path = self
            .list_bin_paths(config, tv)
            .await?
            .into_iter()
            .chain(
                self.dependency_toolset(config)
                    .await?
                    .list_paths(config)
                    .await,
            )
            .chain(env::PATH.clone());
        Ok(env::join_paths(path)?)
    }

    async fn dependency_toolset(&self, config: &Arc<Config>) -> eyre::Result<Toolset> {
        let dependencies = self
            .get_all_dependencies(true)?
            .into_iter()
            .map(|ba| ba.short)
            .collect();
        let mut ts: Toolset = config
            .get_tool_request_set()
            .await?
            .filter_by_tool(dependencies)
            .into();
        ts.resolve(config).await?;
        Ok(ts)
    }

    async fn dependency_which(&self, config: &Arc<Config>, bin: &str) -> Option<PathBuf> {
        if let Some(bin) = file::which_non_pristine(bin) {
            return Some(bin);
        }
        let Ok(ts) = self.dependency_toolset(config).await else {
            return None;
        };
        let (b, tv) = ts.which(config, bin).await?;
        b.which(config, &tv, bin).await.ok().flatten()
    }

    async fn dependency_env(&self, config: &Arc<Config>) -> eyre::Result<BTreeMap<String, String>> {
        self.dependency_toolset(config)
            .await?
            .full_env(config)
            .await
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "v?[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{query}([-.].+)?$")).unwrap();
        versions
            .into_iter()
            .filter(|v| {
                if query == v {
                    return true;
                }
                if VERSION_REGEX.is_match(v) {
                    return false;
                }
                query_regex.is_match(v)
            })
            .collect()
    }

    fn get_remote_version_cache(&self) -> Arc<TokioMutex<VersionCacheManager>> {
        // use a mutex to prevent deadlocks that occurs due to reentrant cache access
        static REMOTE_VERSION_CACHE: Lazy<
            Mutex<HashMap<String, Arc<TokioMutex<VersionCacheManager>>>>,
        > = Lazy::new(Default::default);

        REMOTE_VERSION_CACHE
            .lock()
            .unwrap()
            .entry(self.ba().full())
            .or_insert_with(|| {
                let mut cm = CacheManagerBuilder::new(
                    self.ba().cache_path.join("remote_versions.msgpack.z"),
                )
                .with_fresh_duration(Settings::get().fetch_remote_versions_cache());
                if let Some(plugin_path) = self.plugin().map(|p| p.path()) {
                    cm = cm
                        .with_fresh_file(plugin_path.clone())
                        .with_fresh_file(plugin_path.join("bin/list-all"))
                }

                TokioMutex::new(cm.build()).into()
            })
            .clone()
    }

    fn verify_checksum(
        &self,
        ctx: &InstallContext,
        tv: &mut ToolVersion,
        file: &Path,
    ) -> Result<()> {
        let filename = file.file_name().unwrap().to_string_lossy().to_string();
        if let Some(checksum) = &tv.checksums.get(&filename) {
            ctx.pr.set_message(format!("checksum {filename}"));
            if let Some((algo, check)) = checksum.split_once(':') {
                hash::ensure_checksum(file, check, Some(&ctx.pr), algo)?;
            } else {
                bail!("Invalid checksum: {checksum}");
            }
        } else if Settings::get().lockfile && Settings::get().experimental {
            ctx.pr.set_message(format!("generate checksum {filename}"));
            let hash = hash::file_hash_sha256(file, Some(&ctx.pr))?;
            tv.checksums.insert(filename, format!("sha256:{hash}"));
        }
        Ok(())
    }

    async fn outdated_info(
        &self,
        _config: &Arc<Config>,
        _tv: &ToolVersion,
        _bump: bool,
    ) -> Result<Option<OutdatedInfo>> {
        Ok(None)
    }
}

fn find_match_in_list(list: &[String], query: &str) -> Option<String> {
    match list.contains(&query.to_string()) {
        true => Some(query.to_string()),
        false => list.last().map(|s| s.to_string()),
    }
}

fn rmdir(dir: &Path, pr: &Box<dyn SingleReport>) -> eyre::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    pr.set_message(format!("remove {}", &dir.to_string_lossy()));
    remove_all(dir).wrap_err_with(|| {
        format!(
            "Failed to remove directory {}",
            style(&dir.to_string_lossy()).cyan().for_stderr()
        )
    })
}

pub fn unalias_backend(backend: &str) -> &str {
    match backend {
        "nodejs" => "node",
        "golang" => "go",
        _ => backend,
    }
}

impl Display for dyn Backend {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

impl Eq for dyn Backend {}

impl PartialEq for dyn Backend {
    fn eq(&self, other: &Self) -> bool {
        self.get_plugin_type() == other.get_plugin_type() && self.id() == other.id()
    }
}

impl Hash for dyn Backend {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state)
    }
}

impl PartialOrd for dyn Backend {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for dyn Backend {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(other.id())
    }
}

pub async fn reset() -> Result<()> {
    install_state::reset();
    *TOOLS.lock().unwrap() = None;
    load_tools().await?;
    Ok(())
}
