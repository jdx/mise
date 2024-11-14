use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::cache::{CacheManager, CacheManagerBuilder};
use crate::cli::args::{BackendArg, ToolVersionType};
use crate::cmd::CmdLineRunner;
use crate::config::{Config, CONFIG, SETTINGS};
use crate::file::{display_path, remove_all, remove_all_with_warning};
use crate::install_context::InstallContext;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::{Plugin, PluginType, VERSION_REGEX};
use crate::registry::REGISTRY_BACKEND_MAP;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{
    install_state, is_outdated_version, ToolRequest, ToolSource, ToolVersion, Toolset,
};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, lock_file, plugins, versions_host};
use backend_type::BackendType;
use console::style;
use eyre::{bail, eyre, WrapErr};
use indexmap::IndexSet;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;

pub mod aqua;
pub mod asdf;
pub mod backend_type;
pub mod cargo;
mod external_plugin_cache;
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

static TOOLS: Mutex<Option<BackendMap>> = Mutex::new(None);

pub fn load_tools() {
    let mut memo_tools = TOOLS.lock().unwrap();
    if memo_tools.is_some() {
        return;
    }
    time!("load_tools start");
    let core_tools = CORE_PLUGINS.values().cloned().collect::<Vec<ABackend>>();
    let mut tools = core_tools;
    time!("load_tools core");
    let plugins = install_state::list_plugins()
        .map_err(|err| {
            warn!("{err:#}");
        })
        .unwrap_or_default();
    if !SETTINGS.disable_backends.contains(&"asdf".to_string()) {
        tools.extend(
            plugins
                .iter()
                .filter(|(_, p)| **p == PluginType::Asdf)
                .map(|(p, _)| {
                    arg_to_backend(BackendArg::new(p.to_string(), Some(format!("asdf:{p}"))))
                        .unwrap()
                }),
        );
    }
    time!("load_tools asdf");
    if !SETTINGS.disable_backends.contains(&"vfox".to_string()) {
        tools.extend(
            plugins
                .iter()
                .filter(|(_, p)| **p == PluginType::Vfox)
                .map(|(p, _)| {
                    arg_to_backend(BackendArg::new(p.to_string(), Some(format!("vfox:{p}"))))
                        .unwrap()
                }),
        );
    }
    time!("load_tools vfox");
    tools.extend(
        install_state::list_tools()
            .map_err(|err| {
                warn!("{err:#}");
            })
            .unwrap_or_default()
            .into_values()
            .map(|ist| arg_to_backend(BackendArg::new(ist.short, Some(ist.full))).unwrap()),
    );
    time!("load_tools install_state");
    tools.retain(|backend| !SETTINGS.disable_tools.contains(backend.id()));

    let tools: BackendMap = tools
        .into_iter()
        .map(|backend| (backend.ba().short.clone(), backend))
        .collect();
    *memo_tools = Some(tools.clone());
    time!("load_tools done");
}

pub fn list() -> BackendList {
    load_tools();
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
    load_tools();
    let mut m = TOOLS.lock().unwrap();
    let backends = m.as_mut().unwrap();
    if let Some(backend) = backends.get(&ba.short) {
        Some(backend.clone())
    } else if let Some(backend) = arg_to_backend(ba.clone()) {
        backends.insert(ba.short.clone(), backend.clone());
        Some(backend)
    } else {
        None
    }
}

pub fn remove(short: &str) {
    let mut m = TOOLS.lock().unwrap();
    let backends = m.as_mut().unwrap();
    backends.remove(short);
}

pub fn arg_to_backend(ba: BackendArg) -> Option<ABackend> {
    match ba.backend_type() {
        BackendType::Core => CORE_PLUGINS.get(&ba.short).cloned(),
        BackendType::Aqua => Some(Arc::new(aqua::AquaBackend::from_arg(ba))),
        BackendType::Asdf => Some(Arc::new(asdf::AsdfBackend::from_arg(ba))),
        BackendType::Cargo => Some(Arc::new(cargo::CargoBackend::from_arg(ba))),
        BackendType::Npm => Some(Arc::new(npm::NPMBackend::from_arg(ba))),
        BackendType::Go => Some(Arc::new(go::GoBackend::from_arg(ba))),
        BackendType::Pipx => Some(Arc::new(pipx::PIPXBackend::from_arg(ba))),
        BackendType::Spm => Some(Arc::new(spm::SPMBackend::from_arg(ba))),
        BackendType::Ubi => Some(Arc::new(ubi::UbiBackend::from_arg(ba))),
        BackendType::Vfox => Some(Arc::new(vfox::VfoxBackend::from_arg(ba))),
        BackendType::Unknown => None,
    }
}

pub trait Backend: Debug + Send + Sync {
    fn id(&self) -> &str {
        &self.ba().short
    }
    fn name(&self) -> &str {
        &self.ba().tool_name
    }
    fn get_type(&self) -> BackendType {
        BackendType::Core
    }
    fn ba(&self) -> &BackendArg;
    fn get_plugin_type(&self) -> Option<PluginType> {
        None
    }
    /// If any of these tools are installing in parallel, we should wait for them to finish
    /// before installing this tool.
    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec![])
    }
    fn get_all_dependencies(&self, tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        let mut deps: IndexSet<_> = self
            .get_dependencies(tvr)?
            .into_iter()
            .flat_map(|ba| {
                let short = ba.short.clone();
                [ba].into_iter().chain(
                    REGISTRY_BACKEND_MAP
                        .get(short.as_str())
                        .cloned()
                        .unwrap_or_default(),
                )
            })
            .collect();
        let dep_backends = deps
            .iter()
            .flat_map(|ba| ba.backend())
            .collect::<Vec<ABackend>>();
        for dep in dep_backends {
            // TODO: pass the right tvr
            let tvr = ToolRequest::System(dep.id().into(), ToolSource::Unknown);
            deps.extend(dep.get_all_dependencies(&tvr)?);
        }
        Ok(deps.into_iter().collect())
    }

    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.ensure_dependencies_installed()?;
        self.get_remote_version_cache()
            .get_or_try_init(|| {
                trace!("Listing remote versions for {}", self.ba().to_string());
                match versions_host::list_versions(self.ba()) {
                    Ok(Some(versions)) => return Ok(versions),
                    Ok(None) => {}
                    Err(e) => {
                        debug!("Error getting versions from versions host: {:#}", e);
                    }
                };
                trace!(
                    "Calling backend to list remote versions for {}",
                    self.ba().to_string()
                );
                let versions = self
                    ._list_remote_versions()?
                    .into_iter()
                    .filter(|v| match v.parse::<ToolVersionType>() {
                        Ok(ToolVersionType::Version(_)) => true,
                        _ => {
                            warn!("Invalid version: {}@{v}", self.id());
                            false
                        }
                    })
                    .collect_vec();
                if versions.is_empty() {
                    warn!("No versions found for {}", self.id());
                }
                Ok(versions)
            })
            .cloned()
    }
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>>;
    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version(Some("latest".into()))
    }
    fn list_installed_versions(&self) -> eyre::Result<Vec<String>> {
        install_state::list_versions(&self.ba().short)
    }
    fn is_version_installed(&self, tv: &ToolVersion, check_symlink: bool) -> bool {
        match tv.request {
            ToolRequest::System(..) => true,
            _ => {
                let check_path = |install_path: &Path| {
                    let is_installed = install_path.exists();
                    let is_not_incomplete = !self.incomplete_file_path(tv).exists();
                    let is_valid_symlink = !check_symlink || !is_runtime_symlink(install_path);

                    is_installed && is_not_incomplete && is_valid_symlink
                };
                if let Some(install_path) = tv.request.install_path() {
                    if check_path(&install_path) {
                        return true;
                    }
                }
                check_path(&tv.install_path())
            }
        }
    }
    fn is_version_outdated(&self, tv: &ToolVersion) -> bool {
        let latest = match tv.latest_version() {
            Ok(latest) => latest,
            Err(e) => {
                debug!(
                    "Error getting latest version for {}: {:#}",
                    self.ba().to_string(),
                    e
                );
                return false;
            }
        };
        !self.is_version_installed(tv, true) || is_outdated_version(&tv.version, &latest)
    }
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        match tv.install_path() {
            path if path.is_symlink() && !is_runtime_symlink(&path) => Some(path),
            _ => None,
        }
    }
    fn create_symlink(
        &self,
        version: &str,
        target: &Path,
    ) -> eyre::Result<Option<(PathBuf, PathBuf)>> {
        let link = self.ba().installs_path.join(version);
        if link.exists() {
            return Ok(None);
        }
        file::create_dir_all(link.parent().unwrap())?;
        let link = file::make_symlink(target, &link)?;
        Ok(Some(link))
    }
    fn list_installed_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_installed_versions()?;
        self.fuzzy_match_filter(versions, query)
    }
    fn list_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions()?;
        self.fuzzy_match_filter(versions, query)
    }
    fn latest_version(&self, query: Option<String>) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_versions_matching(&query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => self.latest_stable_version(),
        }
    }
    fn latest_installed_version(&self, query: Option<String>) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.ba().installs_path.join("latest");
                if installed_symlink.exists() {
                    if installed_symlink.is_dir() && !installed_symlink.is_symlink() {
                        return Ok(Some("latest".to_string()));
                    }
                    let target = file::resolve_symlink(&installed_symlink)?;
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

    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        let deps = self
            .get_all_dependencies(&ToolRequest::System(self.id().into(), ToolSource::Unknown))?
            .into_iter()
            .collect::<HashSet<_>>();
        if !deps.is_empty() {
            trace!("Ensuring dependencies installed for {}", self.id());
            let config = Config::get();
            let ts = config.get_tool_request_set()?.filter_by_tool(&deps);
            let missing = ts.missing_tools();
            if !missing.is_empty() {
                bail!(
                    "missing dependency: {}",
                    missing.iter().map(|d| d.to_string()).join(", "),
                );
            }
        }
        Ok(())
    }
    fn purge(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        rmdir(&self.ba().installs_path, pr)?;
        rmdir(&self.ba().cache_path, pr)?;
        rmdir(&self.ba().downloads_path, pr)?;
        Ok(())
    }
    fn get_aliases(&self) -> eyre::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }
    fn legacy_filenames(&self) -> eyre::Result<Vec<String>> {
        Ok(vec![])
    }
    fn parse_legacy_file(&self, path: &Path) -> eyre::Result<String> {
        let contents = file::read_to_string(path)?;
        Ok(contents.trim().to_string())
    }
    fn plugin(&self) -> Option<&dyn Plugin> {
        None
    }

    fn install_version(&self, ctx: InstallContext) -> eyre::Result<()> {
        if let Some(plugin) = self.plugin() {
            plugin.is_installed_err()?;
        }
        let config = Config::get();
        if self.is_version_installed(&ctx.tv, true) {
            if ctx.force {
                self.uninstall_version(&ctx.tv, ctx.pr.as_ref(), false)?;
            } else {
                return Ok(());
            }
        }
        ctx.pr.set_message("installing".into());
        let _lock = lock_file::get(&ctx.tv.install_path(), ctx.force)?;
        self.create_install_dirs(&ctx.tv)?;

        if let Err(e) = self.install_version_impl(&ctx) {
            self.cleanup_install_dirs_on_error(&ctx.tv);
            return Err(e.wrap_err(format!(
                "Failed to install {}@{}",
                self.id(),
                ctx.tv.version
            )));
        }

        self.write_backend_meta()?;

        self.cleanup_install_dirs(&ctx.tv);
        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::DATA.to_path_buf()];
        touch_dirs.extend(config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = file::remove_file(self.incomplete_file_path(&ctx.tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
        if let Some(script) = ctx.tv.request.options().get("postinstall") {
            ctx.pr
                .finish_with_message("running custom postinstall hook".to_string());
            self.run_postinstall_hook(&ctx, script)?;
        }
        ctx.pr.finish_with_message("installed".to_string());

        Ok(())
    }

    fn run_postinstall_hook(&self, ctx: &InstallContext, script: &str) -> eyre::Result<()> {
        CmdLineRunner::new(&*env::SHELL)
            .env(
                &*env::PATH_KEY,
                plugins::core::path_env_with_tv_path(&ctx.tv)?,
            )
            .with_pr(ctx.pr.as_ref())
            .arg("-c")
            .arg(script)
            .envs(self.exec_env(&CONFIG, ctx.ts, &ctx.tv)?)
            .execute()?;
        Ok(())
    }
    fn install_version_impl(&self, ctx: &InstallContext) -> eyre::Result<()>;
    fn uninstall_version(
        &self,
        tv: &ToolVersion,
        pr: &dyn SingleReport,
        dryrun: bool,
    ) -> eyre::Result<()> {
        pr.set_message("uninstall".into());

        if !dryrun {
            self.uninstall_version_impl(pr, tv)?;
        }
        let rmdir = |dir: &Path| {
            if !dir.exists() {
                return Ok(());
            }
            pr.set_message(format!("removing {}", display_path(dir)));
            if dryrun {
                return Ok(());
            }
            remove_all_with_warning(dir)
        };
        rmdir(&tv.install_path())?;
        rmdir(&tv.download_path())?;
        rmdir(&tv.cache_path())?;
        Ok(())
    }
    fn uninstall_version_impl(
        &self,
        _pr: &dyn SingleReport,
        _tv: &ToolVersion,
    ) -> eyre::Result<()> {
        Ok(())
    }
    fn list_bin_paths(&self, tv: &ToolVersion) -> eyre::Result<Vec<PathBuf>> {
        match tv.request {
            ToolRequest::System(..) => Ok(vec![]),
            _ => Ok(vec![tv.install_path().join("bin")]),
        }
    }

    fn exec_env(
        &self,
        _config: &Config,
        _ts: &Toolset,
        _tv: &ToolVersion,
    ) -> eyre::Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }

    fn which(&self, tv: &ToolVersion, bin_name: &str) -> eyre::Result<Option<PathBuf>> {
        let bin_paths = self
            .list_bin_paths(tv)?
            .into_iter()
            .filter(|p| p.parent().is_some());
        for bin_path in bin_paths {
            let bin_path = bin_path.join(bin_name);
            if bin_path.exists() {
                return Ok(Some(bin_path));
            }
        }
        Ok(None)
    }

    fn create_install_dirs(&self, tv: &ToolVersion) -> eyre::Result<()> {
        let _ = remove_all_with_warning(tv.install_path());
        let _ = remove_all_with_warning(tv.download_path());
        let _ = remove_all_with_warning(tv.cache_path());
        let _ = file::remove_file(tv.install_path()); // removes if it is a symlink
        file::create_dir_all(tv.install_path())?;
        file::create_dir_all(tv.download_path())?;
        file::create_dir_all(tv.cache_path())?;
        File::create(self.incomplete_file_path(tv))?;
        Ok(())
    }
    fn cleanup_install_dirs_on_error(&self, tv: &ToolVersion) {
        if !SETTINGS.always_keep_install {
            let _ = remove_all_with_warning(tv.install_path());
            self.cleanup_install_dirs(tv);
        }
    }
    fn cleanup_install_dirs(&self, tv: &ToolVersion) {
        if !SETTINGS.always_keep_download && !SETTINGS.always_keep_install {
            let _ = remove_all_with_warning(tv.download_path());
        }
    }
    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        install_state::incomplete_file_path(&tv.ba().short, &tv.tv_pathname())
    }

    fn dependency_toolset(&self) -> eyre::Result<Toolset> {
        let config = Config::get();
        let dependencies = self
            .get_all_dependencies(&ToolRequest::System(
                self.name().into(),
                ToolSource::Unknown,
            ))?
            .into_iter()
            .collect();
        let mut ts: Toolset = config
            .get_tool_request_set()?
            .filter_by_tool(&dependencies)
            .into();
        ts.resolve()?;
        Ok(ts)
    }

    fn dependency_env(&self) -> eyre::Result<BTreeMap<String, String>> {
        self.dependency_toolset()?.full_env()
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> eyre::Result<Vec<String>> {
        let escaped_query = regex::escape(query);
        let query = if query == "latest" {
            "v?[0-9].*"
        } else {
            &escaped_query
        };
        let query_regex = Regex::new(&format!("^{}([-.].+)?$", query))?;
        let versions = versions
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
            .collect();
        Ok(versions)
    }

    fn get_remote_version_cache(&self) -> Arc<VersionCacheManager> {
        static REMOTE_VERSION_CACHE: Lazy<Mutex<HashMap<String, Arc<VersionCacheManager>>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        REMOTE_VERSION_CACHE
            .lock()
            .unwrap()
            .entry(self.ba().full())
            .or_insert_with(|| {
                let mut cm = CacheManagerBuilder::new(
                    self.ba().cache_path.join("remote_versions.msgpack.z"),
                )
                .with_fresh_duration(SETTINGS.fetch_remote_versions_cache());
                if let Some(plugin_path) = self.plugin().map(|p| p.path()) {
                    cm = cm
                        .with_fresh_file(plugin_path.clone())
                        .with_fresh_file(plugin_path.join("bin/list-all"))
                }

                Arc::new(cm.build())
            })
            .clone()
    }

    fn write_backend_meta(&self) -> eyre::Result<()> {
        file::write(
            self.ba().installs_path.join(".mise.backend"),
            self.ba().full(),
        )
    }
}

fn find_match_in_list(list: &[String], query: &str) -> Option<String> {
    match list.contains(&query.to_string()) {
        true => Some(query.to_string()),
        false => list.last().map(|s| s.to_string()),
    }
}

fn rmdir(dir: &Path, pr: &dyn SingleReport) -> eyre::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    pr.set_message(format!("removing {}", &dir.to_string_lossy()));
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

pub fn reset() {
    *TOOLS.lock().unwrap() = None;
}
