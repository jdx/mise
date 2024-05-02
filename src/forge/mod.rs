use std::collections::BTreeMap;
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use clap::Command;
use console::style;
use eyre::WrapErr;
use itertools::Itertools;
use rayon::prelude::*;
use regex::Regex;
use strum::IntoEnumIterator;
use versions::Versioning;

use crate::cli::args::ForgeArg;
use crate::config::{Config, Settings};
use crate::file::{display_path, remove_all, remove_all_with_warning};
use crate::forge::cargo::CargoForge;
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::plugins::core::CORE_PLUGINS;
use crate::plugins::{ExternalPlugin, PluginType, VERSION_REGEX};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{dirs, file};

use self::forge_meta::ForgeMeta;

mod cargo;
pub mod forge_meta;
mod go;
mod npm;
mod pipx;
mod ubi;

pub type AForge = Arc<dyn Forge>;
pub type ForgeMap = BTreeMap<ForgeArg, AForge>;
pub type ForgeList = Vec<AForge>;

#[derive(
    Debug, PartialEq, Eq, Hash, Clone, Copy, EnumString, EnumIter, AsRefStr, Ord, PartialOrd,
)]
#[strum(serialize_all = "snake_case")]
pub enum ForgeType {
    Asdf,
    Cargo,
    Go,
    Npm,
    Pipx,
    Ubi,
}

impl Display for ForgeType {
    fn fmt(&self, formatter: &mut Formatter) -> std::fmt::Result {
        write!(formatter, "{}", format!("{:?}", self).to_lowercase())
    }
}

static FORGES: Mutex<Option<ForgeMap>> = Mutex::new(None);

fn load_forges() -> ForgeMap {
    let mut forges = FORGES.lock().unwrap();
    if let Some(forges) = &*forges {
        return forges.clone();
    }
    let mut plugins = CORE_PLUGINS.clone();
    plugins.extend(ExternalPlugin::list().expect("failed to list plugins"));
    plugins.extend(list_installed_forges().expect("failed to list forges"));
    let settings = Settings::get();
    plugins.retain(|plugin| !settings.disable_tools.contains(plugin.id()));
    let plugins: ForgeMap = plugins
        .into_iter()
        .map(|plugin| (plugin.fa().clone(), plugin))
        .collect();
    *forges = Some(plugins.clone());
    plugins
}

fn list_installed_forges() -> eyre::Result<ForgeList> {
    Ok(file::dir_subdirs(&dirs::INSTALLS)?
        .into_par_iter()
        .map(|dir| {
            let id = ForgeMeta::read(&dir).id;
            let fa = ForgeArg::from_str(&id).unwrap();
            match fa.forge_type {
                ForgeType::Asdf => Arc::new(ExternalPlugin::new(fa.name)) as AForge,
                ForgeType::Cargo => Arc::new(CargoForge::new(fa.name)) as AForge,
                ForgeType::Npm => Arc::new(npm::NPMForge::new(fa.name)) as AForge,
                ForgeType::Go => Arc::new(go::GoForge::new(fa.name)) as AForge,
                ForgeType::Pipx => Arc::new(pipx::PIPXForge::new(fa.name)) as AForge,
                ForgeType::Ubi => Arc::new(ubi::UbiForge::new(fa.name)) as AForge,
            }
        })
        .filter(|f| f.fa().forge_type != ForgeType::Asdf)
        .collect())
}

pub fn list() -> ForgeList {
    load_forges().values().cloned().collect()
}

pub fn list_forge_types() -> Vec<ForgeType> {
    ForgeType::iter().collect()
}

pub fn get(fa: &ForgeArg) -> AForge {
    if let Some(forge) = load_forges().get(fa) {
        forge.clone()
    } else {
        let mut m = FORGES.lock().unwrap();
        let forges = m.as_mut().unwrap();
        let name = fa.name.to_string();
        forges
            .entry(fa.clone())
            .or_insert_with(|| match fa.forge_type {
                ForgeType::Asdf => Arc::new(ExternalPlugin::new(name)),
                ForgeType::Cargo => Arc::new(CargoForge::new(name)),
                ForgeType::Npm => Arc::new(npm::NPMForge::new(name)),
                ForgeType::Go => Arc::new(go::GoForge::new(name)),
                ForgeType::Pipx => Arc::new(pipx::PIPXForge::new(name)),
                ForgeType::Ubi => Arc::new(ubi::UbiForge::new(name)),
            })
            .clone()
    }
}

pub trait Forge: Debug + Send + Sync {
    fn id(&self) -> &str {
        &self.fa().id
    }
    fn name(&self) -> &str {
        &self.fa().name
    }
    fn get_type(&self) -> ForgeType {
        ForgeType::Asdf
    }
    fn fa(&self) -> &ForgeArg;
    fn get_plugin_type(&self) -> PluginType {
        PluginType::Core
    }
    /// If any of these tools are installing in parallel, we should wait for them to finish
    /// before installing this tool.
    fn get_dependencies(&self, _tv: &ToolVersion) -> eyre::Result<Vec<String>> {
        Ok(vec![])
    }
    fn list_remote_versions(&self) -> eyre::Result<Vec<String>> {
        self.ensure_dependencies_installed()?;
        self._list_remote_versions()
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
    fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        match tv.request {
            ToolVersionRequest::System(_) => true,
            _ => {
                tv.install_path().exists()
                    && !self.incomplete_file_path(tv).exists()
                    && !is_runtime_symlink(&tv.install_path())
            }
        }
    }
    fn is_version_outdated(&self, tv: &ToolVersion, p: &dyn Forge) -> bool {
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
        !self.is_version_installed(tv) || tv.version != latest
    }
    fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        match tv.install_path() {
            path if path.is_symlink() => Some(path),
            _ => None,
        }
    }
    fn create_symlink(&self, version: &str, target: &Path) -> eyre::Result<()> {
        let link = self.fa().installs_path.join(version);
        file::create_dir_all(link.parent().unwrap())?;
        file::make_symlink(target, &link)
    }
    fn list_installed_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_installed_versions()?;
        fuzzy_match_filter(versions, query)
    }
    fn list_versions_matching(&self, query: &str) -> eyre::Result<Vec<String>> {
        let versions = self.list_remote_versions()?;
        fuzzy_match_filter(versions, query)
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
    #[requires(self.is_installed())]
    fn latest_installed_version(&self, query: Option<String>) -> eyre::Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.fa().installs_path.join("latest");
                if installed_symlink.exists() {
                    if !installed_symlink.is_symlink() {
                        return Ok(Some("latest".to_string()));
                    }
                    let target = installed_symlink.read_link()?;
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
    fn current_sha_short(&self) -> eyre::Result<String> {
        Ok(String::from(""))
    }
    fn current_abbrev_ref(&self) -> eyre::Result<String> {
        Ok(String::from(""))
    }
    fn is_installed(&self) -> bool {
        true
    }
    fn ensure_installed(&self, _mpr: &MultiProgressReport, _force: bool) -> eyre::Result<()> {
        Ok(())
    }
    fn ensure_dependencies_installed(&self) -> eyre::Result<()> {
        Ok(())
    }
    fn update(&self, _pr: &dyn SingleReport, _git_ref: Option<String>) -> eyre::Result<()> {
        Ok(())
    }
    fn uninstall(&self, _pr: &dyn SingleReport) -> eyre::Result<()> {
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
    fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        Ok(vec![])
    }
    fn execute_external_command(&self, _command: &str, _args: Vec<String>) -> eyre::Result<()> {
        unimplemented!()
    }

    #[requires(ctx.tv.forge.forge_type == self.get_type())]
    fn install_version(&self, ctx: InstallContext) -> eyre::Result<()> {
        ensure!(self.is_installed(), "{} is not installed", self.id());
        let config = Config::get();
        let settings = Settings::try_get()?;
        if self.is_version_installed(&ctx.tv) {
            if ctx.force {
                self.uninstall_version(&ctx.tv, ctx.pr.as_ref(), false)?;
                ctx.pr.set_message("installing".into());
            } else {
                return Ok(());
            }
        }
        let _lock = self.get_lock(&ctx.tv.install_path(), ctx.force)?;
        self.create_install_dirs(&ctx.tv)?;

        if let Err(e) = self.install_version_impl(&ctx) {
            self.cleanup_install_dirs_on_error(&settings, &ctx.tv);
            return Err(e);
        }

        ForgeMeta::write(&ctx.tv.forge)?;

        self.cleanup_install_dirs(&settings, &ctx.tv);
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
        ctx.pr.finish_with_message("installed".to_string());

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
            ToolVersionRequest::System(_) => Ok(vec![]),
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

    fn get_lock(&self, path: &Path, force: bool) -> eyre::Result<Option<fslock::LockFile>> {
        let lock = if force {
            None
        } else {
            let lock = LockFile::new(path)
                .with_callback(|l| {
                    debug!("waiting for lock on {}", display_path(l));
                })
                .lock()?;
            Some(lock)
        };
        Ok(lock)
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
}

fn fuzzy_match_filter(versions: Vec<String>, query: &str) -> eyre::Result<Vec<String>> {
    let mut query = query;
    if query == "latest" {
        query = "[0-9].*";
    }
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

fn find_match_in_list(list: &[String], query: &str) -> Option<String> {
    let v = match list.contains(&query.to_string()) {
        true => Some(query.to_string()),
        false => list.last().map(|s| s.to_string()),
    };
    v
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

pub fn unalias_forge(forge: &str) -> &str {
    match forge {
        "nodejs" => "node",
        "golang" => "go",
        _ => forge,
    }
}

impl Display for dyn Forge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id())
    }
}

impl Eq for dyn Forge {}

impl PartialEq for dyn Forge {
    fn eq(&self, other: &Self) -> bool {
        self.get_plugin_type() == other.get_plugin_type() && self.id() == other.id()
    }
}

impl Hash for dyn Forge {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id().hash(state)
    }
}

impl PartialOrd for dyn Forge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for dyn Forge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(other.id())
    }
}

#[cfg(test)]
pub fn reset() {
    *FORGES.lock().unwrap() = None;
}
