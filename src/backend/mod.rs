use std::collections::{BTreeMap, HashSet};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use console::style;
use contracts::requires;
use eyre::{bail, eyre, WrapErr};
use itertools::Itertools;
use rayon::prelude::*;
use regex::Regex;
use strum::IntoEnumIterator;
use versions::Versioning;

use self::backend_meta::BackendMeta;
use crate::cli::args::{BackendArg, ToolVersionType};
use crate::cmd::CmdLineRunner;
use crate::config::SETTINGS;
use crate::config::{Config, Settings, CONFIG};
use crate::file::{display_path, remove_all, remove_all_with_warning};
use crate::install_context::InstallContext;
use crate::plugins::core::{CorePlugin, CORE_PLUGINS};
use crate::plugins::{Plugin, PluginType, VERSION_REGEX};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{is_outdated_version, ToolRequest, ToolVersion, Toolset};
use crate::ui::progress_report::SingleReport;
use crate::{dirs, env, file, lock_file};

pub mod asdf;
pub mod backend_meta;
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

#[derive(
    Debug,
    PartialEq,
    Eq,
    Hash,
    Clone,
    Copy,
    strum::EnumString,
    strum::EnumIter,
    strum::AsRefStr,
    Ord,
    PartialOrd,
)]
#[strum(serialize_all = "snake_case")]
pub enum BackendType {
    Asdf,
    Cargo,
    Core,
    Go,
    Npm,
    Pipx,
    Spm,
    Ubi,
    Vfox,
}

impl Display for BackendType {
    fn fmt(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "{}", format!("{:?}", self).to_lowercase())
    }
}

static TOOLS: Mutex<Option<BackendMap>> = Mutex::new(None);

fn load_tools() -> BackendMap {
    if let Some(backends) = TOOLS.lock().unwrap().as_ref() {
        return backends.clone();
    }
    let mut tools = CORE_PLUGINS
        .iter()
        .map(|(_, p)| p.clone())
        .collect::<Vec<ABackend>>();
    let settings = Settings::get();
    if !cfg!(windows) || settings.asdf {
        tools.extend(asdf::AsdfBackend::list().expect("failed to list asdf plugins"));
    }
    if cfg!(windows) || settings.vfox {
        tools.extend(vfox::VfoxBackend::list().expect("failed to list vfox plugins"));
    }
    tools.extend(list_installed_backends().expect("failed to list backends"));
    tools.retain(|plugin| !settings.disable_tools.contains(plugin.id()));
    let tools: BackendMap = tools
        .into_iter()
        .map(|plugin| (plugin.id().to_string(), plugin))
        .collect();
    *TOOLS.lock().unwrap() = Some(tools.clone());
    tools
}

fn list_installed_backends() -> eyre::Result<BackendList> {
    Ok(file::dir_subdirs(&dirs::INSTALLS)?
        .into_par_iter()
        .map(|dir| arg_to_backend(BackendMeta::read(&dir).into()))
        .filter(|f| f.fa().backend_type != BackendType::Asdf)
        .collect())
}

pub fn list() -> BackendList {
    load_tools().values().cloned().collect()
}

pub fn list_backend_types() -> Vec<BackendType> {
    BackendType::iter().collect()
}

pub fn get(fa: &BackendArg) -> ABackend {
    if let Some(backend) = load_tools().get(&fa.short) {
        backend.clone()
    } else {
        let mut m = TOOLS.lock().unwrap();
        let backends = m.as_mut().unwrap();
        let fa = fa.clone();
        backends
            .entry(fa.short.clone())
            .or_insert_with(|| arg_to_backend(fa))
            .clone()
    }
}

pub fn arg_to_backend(ba: BackendArg) -> ABackend {
    match ba.backend_type {
        BackendType::Asdf => Arc::new(asdf::AsdfBackend::from_arg(ba)),
        BackendType::Cargo => Arc::new(cargo::CargoBackend::from_arg(ba)),
        BackendType::Core => Arc::new(asdf::AsdfBackend::from_arg(ba)),
        BackendType::Npm => Arc::new(npm::NPMBackend::from_arg(ba)),
        BackendType::Go => Arc::new(go::GoBackend::from_arg(ba)),
        BackendType::Pipx => Arc::new(pipx::PIPXBackend::from_arg(ba)),
        BackendType::Spm => Arc::new(spm::SPMBackend::from_arg(ba)),
        BackendType::Ubi => Arc::new(ubi::UbiBackend::from_arg(ba)),
        BackendType::Vfox => Arc::new(vfox::VfoxBackend::from_arg(ba)),
    }
}

impl From<BackendArg> for ABackend {
    fn from(fa: BackendArg) -> Self {
        get(&fa)
    }
}

impl From<&BackendArg> for ABackend {
    fn from(fa: &BackendArg) -> Self {
        get(fa)
    }
}

pub trait Backend: Debug + Send + Sync {
    fn id(&self) -> &str {
        &self.fa().short
    }
    fn name(&self) -> &str {
        &self.fa().name
    }
    fn get_type(&self) -> BackendType {
        BackendType::Asdf
    }
    fn fa(&self) -> &BackendArg;
    fn get_plugin_type(&self) -> PluginType {
        PluginType::Core
    }
    /// If any of these tools are installing in parallel, we should wait for them to finish
    /// before installing this tool.
    fn get_dependencies(&self, _tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        Ok(vec![])
    }
    fn get_all_dependencies(&self, tvr: &ToolRequest) -> eyre::Result<Vec<BackendArg>> {
        let mut deps = self.get_dependencies(tvr)?;
        let dep_backends = deps.iter().map(|fa| fa.into()).collect::<Vec<ABackend>>();
        for dep in dep_backends {
            // TODO: pass the right tvr
            let tvr = ToolRequest::System(dep.id().into());
            deps.extend(dep.get_all_dependencies(&tvr)?);
        }
        Ok(deps)
    }
    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.ensure_dependencies_installed()?;
        trace!("Listing remote versions for {}", self.fa().to_string());
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
    }
    fn _list_remote_versions(&self) -> eyre::Result<Vec<String>>;
    fn latest_stable_version(&self) -> eyre::Result<Option<String>> {
        self.latest_version(Some("latest".into()))
    }
    fn list_installed_versions(&self) -> eyre::Result<Vec<String>> {
        let installs_path = &self.fa().installs_path;
        Ok(match installs_path.exists() {
            true => file::dir_subdirs(installs_path)?
                .into_iter()
                .filter(|v| !v.starts_with('.'))
                .filter(|v| !is_runtime_symlink(&installs_path.join(v)))
                .filter(|v| !installs_path.join(v).join("incomplete").exists())
                .sorted_by_cached_key(|v| (Versioning::new(v), v.to_string()))
                .collect(),
            false => vec![],
        })
    }
    fn is_version_installed(&self, tv: &ToolVersion, check_symlink: bool) -> bool {
        match tv.request {
            ToolRequest::System(_) => true,
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
    fn is_version_outdated(&self, tv: &ToolVersion, p: &dyn Backend) -> bool {
        let latest = match tv.latest_version(p) {
            Ok(latest) => latest,
            Err(e) => {
                debug!(
                    "Error getting latest version for {}: {:#}",
                    self.fa().to_string(),
                    e
                );
                return false;
            }
        };
        !self.is_version_installed(tv, true) || is_outdated_version(&tv.version, &latest)
    }
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        match tv.install_path() {
            path if path.is_symlink() => Some(path),
            _ => None,
        }
    }
    fn create_symlink(
        &self,
        version: &str,
        target: &Path,
    ) -> eyre::Result<Option<(PathBuf, PathBuf)>> {
        let link = self.fa().installs_path.join(version);
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
                let installed_symlink = self.fa().installs_path.join("latest");
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

    fn get_remote_url(&self) -> Option<String> {
        None
    }
    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        trace!("Ensuring dependencies installed for {}", self.id());
        let deps = self
            .get_all_dependencies(&ToolRequest::System(self.id().into()))?
            .into_iter()
            .collect::<HashSet<_>>();
        let config = Config::get();
        let ts = config.get_tool_request_set()?.filter_by_tool(&deps);
        if !ts.missing_tools().is_empty() {
            bail!(
                "Dependency {} not installed for {}",
                deps.iter().map(|d| d.to_string()).join(", "),
                self.id()
            );
        }
        Ok(())
    }
    fn purge(&self, pr: &dyn SingleReport) -> eyre::Result<()> {
        rmdir(&self.fa().installs_path, pr)?;
        rmdir(&self.fa().cache_path, pr)?;
        rmdir(&self.fa().downloads_path, pr)?;
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

    #[requires(ctx.tv.backend.backend_type == self.get_type())]
    fn install_version(&self, ctx: InstallContext) -> eyre::Result<()> {
        if let Some(plugin) = self.plugin() {
            plugin.is_installed_err()?;
        }
        let config = Config::get();
        if self.is_version_installed(&ctx.tv, true) {
            if ctx.force {
                self.uninstall_version(&ctx.tv, ctx.pr.as_ref(), false)?;
                ctx.pr.set_message("installing".into());
            } else {
                return Ok(());
            }
        }
        let _lock = lock_file::get(&ctx.tv.install_path(), ctx.force)?;
        self.create_install_dirs(&ctx.tv)?;

        if let Err(e) = self.install_version_impl(&ctx) {
            self.cleanup_install_dirs_on_error(&SETTINGS, &ctx.tv);
            return Err(e);
        }

        BackendMeta::write(&ctx.tv.backend)?;

        self.cleanup_install_dirs(&SETTINGS, &ctx.tv);
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
            .env(&*env::PATH_KEY, CorePlugin::path_env_with_tv_path(&ctx.tv)?)
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
            ToolRequest::System(_) => Ok(vec![]),
            _ => Ok(vec![tv.install_short_path().join("bin")]),
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
        let bin_paths = self.list_bin_paths(tv)?;
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
    fn cleanup_install_dirs_on_error(&self, settings: &Settings, tv: &ToolVersion) {
        if !settings.always_keep_install {
            let _ = remove_all_with_warning(tv.install_path());
            self.cleanup_install_dirs(settings, tv);
        }
    }
    fn cleanup_install_dirs(&self, settings: &Settings, tv: &ToolVersion) {
        if !settings.always_keep_download && !settings.always_keep_install {
            let _ = remove_all_with_warning(tv.download_path());
        }
    }
    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.cache_path().join("incomplete")
    }

    fn dependency_toolset(&self) -> eyre::Result<Toolset> {
        let config = Config::get();
        let dependencies = self
            .get_all_dependencies(&ToolRequest::System(self.name().into()))?
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

#[cfg(test)]
pub fn reset() {
    *TOOLS.lock().unwrap() = None;
}
