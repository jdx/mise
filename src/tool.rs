use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Command;
use color_eyre::eyre::{eyre, Result};
use console::style;
use itertools::Itertools;
use regex::Regex;
use versions::Versioning;

use crate::config::{Config, Settings};
use crate::file::{display_path, remove_all_with_warning};
use crate::plugins::{ExternalPlugin, Plugin};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};
use crate::{dirs, file};

pub struct Tool {
    pub name: String,
    pub plugin: Box<dyn Plugin>,
    pub installs_path: PathBuf,
    pub plugin_path: PathBuf,
}

impl Tool {
    pub fn new(name: String, plugin: Box<dyn Plugin>) -> Self {
        Self {
            installs_path: dirs::INSTALLS.join(&name),
            plugin_path: dirs::PLUGINS.join(&name),
            name,
            plugin,
        }
    }

    pub fn list() -> Result<Vec<Self>> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .iter()
            .map(|name| {
                let plugin = ExternalPlugin::new(name);
                Self::new(name.to_string(), Box::new(plugin))
            })
            .collect())
    }

    pub fn is_installed(&self) -> bool {
        self.plugin.is_installed()
    }

    pub fn get_remote_url(&self) -> Option<String> {
        self.plugin.get_remote_url()
    }

    pub fn current_sha_short(&self) -> Result<String> {
        self.plugin.current_sha_short()
    }

    pub fn current_abbrev_ref(&self) -> Result<String> {
        self.plugin.current_abbrev_ref()
    }

    pub fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path.exists() {
            true => file::dir_subdirs(&self.installs_path)?
                .iter()
                .filter(|v| !is_runtime_symlink(&self.installs_path.join(v)))
                // TODO: share logic with incomplete_file_path
                .filter(|v| {
                    !dirs::CACHE
                        .join(&self.name)
                        .join(v)
                        .join("incomplete")
                        .exists()
                })
                .map(|v| Versioning::new(v).unwrap_or_default())
                .sorted()
                .map(|v| v.to_string())
                .collect(),
            false => vec![],
        })
    }

    pub fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
        let versions = self.list_installed_versions()?;
        Ok(self.fuzzy_match_filter(versions, query))
    }

    pub fn list_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        self.plugin.list_remote_versions(settings)
    }

    pub fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        let versions = self.list_remote_versions(settings)?;
        Ok(self.fuzzy_match_filter(versions, query))
    }

    pub fn latest_version(
        &self,
        settings: &Settings,
        query: Option<String>,
    ) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_versions_matching(settings, &query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => self.latest_stable_version(settings),
        }
    }

    pub fn latest_installed_version(&self, query: Option<String>) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_installed_versions_matching(&query)?;
                Ok(find_match_in_list(&matches, &query))
            }
            None => {
                let installed_symlink = self.installs_path.join("latest");
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

    pub fn get_aliases(&self, settings: &Settings) -> Result<BTreeMap<String, String>> {
        self.plugin.get_aliases(settings)
    }

    pub fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        self.plugin.legacy_filenames(settings)
    }

    fn latest_stable_version(&self, settings: &Settings) -> Result<Option<String>> {
        if let Some(latest) = self.plugin.latest_stable_version(settings)? {
            Ok(Some(latest))
        } else {
            self.latest_version(settings, Some("latest".into()))
        }
    }

    pub fn decorate_progress_bar(&self, pr: &mut ProgressReport, tv: Option<&ToolVersion>) {
        pr.set_style(PROG_TEMPLATE.clone());
        let tool = match tv {
            Some(tv) => tv.to_string(),
            None => self.name.to_string(),
        };
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(tool).cyan().for_stderr(),
        ));
        pr.enable_steady_tick();
    }

    pub fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        match tv.request {
            ToolVersionRequest::System(_) => true,
            _ => {
                tv.install_path().exists()
                    && !self.incomplete_file_path(tv).exists()
                    && !is_runtime_symlink(&tv.install_path())
            }
        }
    }

    pub fn is_version_outdated(&self, config: &Config, tv: &ToolVersion) -> bool {
        let latest = match tv.latest_version(config, self) {
            Ok(latest) => latest,
            Err(e) => {
                debug!("Error getting latest version for {}: {:#}", self.name, e);
                return false;
            }
        };
        !self.is_version_installed(tv) || tv.version != latest
    }

    pub fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        match tv.install_path() {
            path if path.is_symlink() => Some(path),
            _ => None,
        }
    }

    pub fn create_symlink(&self, version: &str, target: &Path) -> Result<()> {
        let link = self.installs_path.join(version);
        file::create_dir_all(link.parent().unwrap())?;
        file::make_symlink(target, &link)
    }

    pub fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
        force: bool,
    ) -> Result<()> {
        if self.is_version_installed(tv) {
            if force {
                self.uninstall_version(config, tv, pr, false)?;
            } else {
                return Ok(());
            }
        }
        self.decorate_progress_bar(pr, Some(tv));
        let _lock = self.get_lock(&tv.install_path(), force)?;
        self.create_install_dirs(tv)?;

        if let Err(e) = self.plugin.install_version(config, tv, pr) {
            self.cleanup_install_dirs_on_error(&config.settings, tv);
            return Err(e);
        }
        self.cleanup_install_dirs(&config.settings, tv);
        // attempt to touch all the .tool-version files to trigger updates in hook-env
        let mut touch_dirs = vec![dirs::ROOT.to_path_buf()];
        touch_dirs.extend(config.config_files.keys().cloned());
        for path in touch_dirs {
            let err = file::touch_dir(&path);
            if let Err(err) = err {
                debug!("error touching config file: {:?} {:?}", path, err);
            }
        }
        if let Err(err) = file::remove_file(self.incomplete_file_path(tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
        pr.set_message("");
        pr.finish();

        Ok(())
    }

    pub fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()> {
        pr.set_message(format!("uninstall {tv}"));

        if !dryrun {
            self.plugin.uninstall_version(config, tv)?;
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

    pub fn ensure_installed(
        &self,
        config: &mut Config,
        mpr: Option<&MultiProgressReport>,
        force: bool,
    ) -> Result<()> {
        self.plugin.ensure_installed(config, mpr, force)
    }
    pub fn update(&self, git_ref: Option<String>) -> Result<()> {
        self.plugin.update(git_ref)
    }
    pub fn uninstall(&self, pr: &ProgressReport) -> Result<()> {
        self.plugin.uninstall(pr)
    }

    pub fn external_commands(&self) -> Result<Vec<Command>> {
        self.plugin.external_commands()
    }
    pub fn execute_external_command(
        &self,
        config: &Config,
        command: &str,
        args: Vec<String>,
    ) -> Result<()> {
        self.plugin.execute_external_command(config, command, args)
    }
    pub fn parse_legacy_file(&self, path: &Path, settings: &Settings) -> Result<String> {
        self.plugin.parse_legacy_file(path, settings)
    }
    pub fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        match tv.request {
            ToolVersionRequest::System(_) => Ok(vec![]),
            _ => self.plugin.list_bin_paths(config, tv),
        }
    }
    pub fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        match tv.request {
            ToolVersionRequest::System(_) => Ok(HashMap::new()),
            _ => self.plugin.exec_env(config, tv),
        }
    }

    pub fn which(
        &self,
        config: &Config,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> Result<Option<PathBuf>> {
        let bin_paths = self.plugin.list_bin_paths(config, tv)?;
        for bin_path in bin_paths {
            let bin_path = bin_path.join(bin_name);
            if bin_path.exists() {
                return Ok(Some(bin_path));
            }
        }
        Ok(None)
    }

    fn incomplete_file_path(&self, tv: &ToolVersion) -> PathBuf {
        tv.cache_path().join("incomplete")
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

    fn get_lock(&self, path: &Path, force: bool) -> Result<Option<fslock::LockFile>> {
        self.plugin.get_lock(path, force)
    }

    fn fuzzy_match_filter(&self, versions: Vec<String>, query: &str) -> Vec<String> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9].*";
        }
        let query_regex =
            Regex::new(&format!("^{}([-.].+)?$", query)).expect("error parsing regex");
        let version_regex = regex!(
            r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|master)"
        );
        versions
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
            .collect()
    }
}

impl PartialEq for Tool {
    fn eq(&self, other: &Self) -> bool {
        self.plugin_path == other.plugin_path
    }
}

impl Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .field("installs_path", &self.installs_path)
            .field("plugin", &self.plugin)
            .finish()
    }
}

fn find_match_in_list(list: &[String], query: &str) -> Option<String> {
    let v = match list.contains(&query.to_string()) {
        true => Some(query.to_string()),
        false => list.last().map(|s| s.to_string()),
    };
    v
}

impl PartialOrd for Tool {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.name.cmp(&other.name))
    }
}

impl Ord for Tool {
    fn cmp(&self, other: &Self) -> Ordering {
        self.name.cmp(&other.name)
    }
}

impl Eq for Tool {}

#[cfg(test)]
mod tests {
    use crate::plugins::PluginName;

    use super::*;

    #[test]
    fn test_debug() {
        let plugin = ExternalPlugin::new(&PluginName::from("dummy"));
        let tool = Tool::new("dummy".to_string(), Box::new(plugin));
        let debug = format!("{:?}", tool);
        assert!(debug.contains("Tool"));
        assert!(debug.contains("name"));
        assert!(debug.contains("installs_path"));
        assert!(debug.contains("plugin"));
    }
}
