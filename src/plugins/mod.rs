use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Display};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Command;
use color_eyre::eyre::Result;
use console::style;
use eyre::WrapErr;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
use versions::Versioning;

pub use external_plugin::ExternalPlugin;
pub use script_manager::{Script, ScriptManager};

use crate::config::{Config, Settings};
use crate::env::RTX_FETCH_REMOTE_VERSIONS_TIMEOUT;
use crate::file::{display_path, remove_all, remove_all_with_warning};
use crate::install_context::InstallContext;
use crate::lock_file::LockFile;
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolVersion, ToolVersionRequest, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};
use crate::{dirs, file, http};

pub mod core;
mod external_plugin;
mod external_plugin_cache;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub static HTTP: Lazy<http::Client> =
    Lazy::new(|| http::Client::new_with_timeout(*RTX_FETCH_REMOTE_VERSIONS_TIMEOUT).unwrap());

pub trait Plugin: Debug + Send + Sync {
    fn name(&self) -> &str;
    fn get_type(&self) -> PluginType {
        PluginType::Core
    }
    fn installs_path(&self) -> PathBuf {
        dirs::INSTALLS.join(self.name())
    }
    fn cache_path(&self) -> PathBuf {
        dirs::CACHE.join(self.name())
    }
    fn downloads_path(&self) -> PathBuf {
        dirs::DOWNLOADS.join(self.name())
    }
    fn list_remote_versions(&self, settings: &Settings) -> Result<Vec<String>>;
    fn latest_stable_version(&self, settings: &Settings) -> Result<Option<String>> {
        self.latest_version(settings, Some("latest".into()))
    }
    fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path().exists() {
            true => file::dir_subdirs(&self.installs_path())?
                .iter()
                .filter(|v| !is_runtime_symlink(&self.installs_path().join(v)))
                .filter(|v| !self.installs_path().join(v).join("incomplete").exists())
                .cloned()
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
    fn is_version_outdated(&self, config: &Config, tv: &ToolVersion, p: Arc<dyn Plugin>) -> bool {
        let latest = match tv.latest_version(config, p) {
            Ok(latest) => latest,
            Err(e) => {
                debug!("Error getting latest version for {}: {:#}", self.name(), e);
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
    fn create_symlink(&self, version: &str, target: &Path) -> Result<()> {
        let link = self.installs_path().join(version);
        file::create_dir_all(link.parent().unwrap())?;
        file::make_symlink(target, &link)
    }
    fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
        let versions = self.list_installed_versions()?;
        fuzzy_match_filter(versions, query)
    }
    fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        let versions = self.list_remote_versions(settings)?;
        fuzzy_match_filter(versions, query)
    }
    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_versions_matching(settings, &query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => self.latest_stable_version(settings),
        }
    }
    fn latest_installed_version(&self, query: Option<String>) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.installs_path().join("latest");
                if installed_symlink.exists() {
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
    fn current_sha_short(&self) -> Result<String> {
        Ok(String::from(""))
    }
    fn current_abbrev_ref(&self) -> Result<String> {
        Ok(String::from(""))
    }
    fn is_installed(&self) -> bool {
        true
    }
    fn ensure_installed(
        &self,
        _config: &mut Config,
        _mpr: Option<&MultiProgressReport>,
        _force: bool,
    ) -> Result<()> {
        Ok(())
    }
    fn update(&self, _git_ref: Option<String>) -> Result<()> {
        Ok(())
    }
    fn uninstall(&self, _pr: &ProgressReport) -> Result<()> {
        Ok(())
    }
    fn purge(&self, pr: &ProgressReport) -> Result<()> {
        rmdir(&self.installs_path(), pr)?;
        rmdir(&self.cache_path(), pr)?;
        rmdir(&self.downloads_path(), pr)?;
        Ok(())
    }
    fn get_aliases(&self, _settings: &Settings) -> Result<BTreeMap<String, String>> {
        Ok(BTreeMap::new())
    }
    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![])
    }
    fn parse_legacy_file(&self, path: &Path, _settings: &Settings) -> Result<String> {
        let contents = file::read_to_string(path)?;
        Ok(contents.trim().to_string())
    }
    fn external_commands(&self) -> Result<Vec<Command>> {
        Ok(vec![])
    }
    fn execute_external_command(
        &self,
        _config: &Config,
        _command: &str,
        _args: Vec<String>,
    ) -> Result<()> {
        unimplemented!()
    }
    fn install_version(&self, mut ctx: InstallContext) -> Result<()> {
        if self.is_version_installed(&ctx.tv) {
            if ctx.force {
                self.uninstall_version(ctx.config, &ctx.tv, &ctx.pr, false)?;
            } else {
                return Ok(());
            }
        }
        self.decorate_progress_bar(&mut ctx.pr, Some(&ctx.tv));
        let _lock = self.get_lock(&ctx.tv.install_path(), ctx.force)?;
        self.create_install_dirs(&ctx.tv)?;

        if let Err(e) = self.install_version_impl(&ctx) {
            self.cleanup_install_dirs_on_error(&ctx.config.settings, &ctx.tv);
            return Err(e);
        }
        self.cleanup_install_dirs(&ctx.config.settings, &ctx.tv);
        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::DATA.to_path_buf()];
        touch_dirs.extend(ctx.config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = file::remove_file(self.incomplete_file_path(&ctx.tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
        ctx.pr.set_message("");
        ctx.pr.finish();

        Ok(())
    }
    fn install_version_impl(&self, ctx: &InstallContext) -> Result<()>;
    fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()> {
        pr.set_message(format!("uninstall {tv}"));

        if !dryrun {
            self.uninstall_version_impl(config, tv)?;
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
    fn uninstall_version_impl(&self, _config: &Config, _tv: &ToolVersion) -> Result<()> {
        Ok(())
    }
    fn list_bin_paths(
        &self,
        _config: &Config,
        _ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
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
    ) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }
    fn which(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> Result<Option<PathBuf>> {
        let bin_paths = self.list_bin_paths(config, ts, tv)?;
        for bin_path in bin_paths {
            let bin_path = bin_path.join(bin_name);
            if bin_path.exists() {
                return Ok(Some(bin_path));
            }
        }
        Ok(None)
    }

    fn get_lock(&self, path: &Path, force: bool) -> Result<Option<fslock::LockFile>> {
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
    fn create_install_dirs(&self, tv: &ToolVersion) -> Result<()> {
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
    fn decorate_progress_bar(&self, pr: &mut ProgressReport, tv: Option<&ToolVersion>) {
        pr.set_style(PROG_TEMPLATE.clone());
        let tool = match tv {
            Some(tv) => tv.to_string(),
            None => self.name().to_string(),
        };
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(tool).cyan().for_stderr(),
        ));
        pr.enable_steady_tick();
    }

    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.install_path().join("incomplete")
    }
}

pub fn unalias_plugin(plugin_name: &str) -> &str {
    match plugin_name {
        "nodejs" => "node",
        "golang" => "go",
        _ => plugin_name,
    }
}

fn fuzzy_match_filter(versions: Vec<String>, query: &str) -> Result<Vec<String>> {
    let mut query = query;
    if query == "latest" {
        query = "[0-9].*";
    }
    let query_regex = Regex::new(&format!("^{}([-.].+)?$", query))?;
    let version_regex = regex!(
        r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|SNAPSHOT|master)"
    );
    let versions = versions
        .into_iter()
        .filter(|v| {
            if query == v {
                return true;
            }
            if version_regex.is_match(v) {
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
fn rmdir(dir: &Path, pr: &ProgressReport) -> Result<()> {
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

impl Display for dyn Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
impl Eq for dyn Plugin {}
impl PartialEq for dyn Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.get_type() == other.get_type() && self.name() == other.name()
    }
}
impl PartialOrd for dyn Plugin {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for dyn Plugin {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginType {
    #[allow(dead_code)]
    Core,
    External,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;

    use crate::assert_cli;
    use crate::config::Settings;

    use super::*;

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "tiny");
        let settings = Settings::default();
        let plugin = ExternalPlugin::newa(PluginName::from("tiny"));
        let version = plugin
            .latest_version(&settings, Some("1.0.0".into()))
            .unwrap()
            .unwrap();
        assert_str_eq!(version, "1.0.0");
        let version = plugin.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "3.1.0");
    }

    #[test]
    fn test_latest_stable() {
        let settings = Settings::default();
        let plugin = ExternalPlugin::new(PluginName::from("dummy"));
        let version = plugin.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
