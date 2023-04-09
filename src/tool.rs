use std::collections::{BTreeMap, HashMap};
use std::fs::{remove_file, File};
use std::path::{Path, PathBuf};

use color_eyre::eyre::{eyre, Result};
use console::style;
use itertools::Itertools;
use regex::Regex;
use versions::Versioning;

use crate::config::{Config, Settings};
use crate::file::{create_dir_all, display_path, remove_all_with_warning};
use crate::lock_file::LockFile;
use crate::plugins::{ExternalPlugin, Plugin, PluginType};
use crate::runtime_symlinks::is_runtime_symlink;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};
use crate::{dirs, file};

#[derive(Debug)]
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

    pub fn list(settings: &Settings) -> Result<Vec<Self>> {
        Ok(file::dir_subdirs(&dirs::PLUGINS)?
            .iter()
            .map(|name| {
                let plugin = ExternalPlugin::new(settings, name);
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

    pub fn list_installed_versions(&self) -> Result<Vec<String>> {
        Ok(match self.installs_path.exists() {
            true => file::dir_subdirs(&self.installs_path)?
                .iter()
                .filter(|v| !is_runtime_symlink(&self.installs_path.join(v)))
                .map(|v| Versioning::new(v).unwrap_or_default())
                .sorted()
                .map(|v| v.to_string())
                .collect(),
            false => vec![],
        })
    }

    pub fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9]";
        }
        let query_regex =
            Regex::new((String::from(r"^\s*") + query).as_str()).expect("error parsing regex");
        let versions = self
            .list_installed_versions()?
            .iter()
            .filter(|v| query_regex.is_match(v))
            .cloned()
            .collect();
        Ok(versions)
    }

    pub fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        self.plugin.list_remote_versions(settings)
    }

    pub fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        let mut query = query;
        if query == "latest" {
            query = "[0-9]";
        }
        let version_regex = regex!(
            r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|(a|b|c)[0-9]+|snapshot|master)"
        );
        let query_regex =
            Regex::new((String::from(r"^\s*") + query).as_str()).expect("error parsing regex");
        let versions = self
            .list_remote_versions(settings)?
            .iter()
            .filter(|v| !version_regex.is_match(v))
            .filter(|v| query_regex.is_match(v))
            .cloned()
            .collect();
        Ok(versions)
    }

    pub fn latest_version(
        &self,
        settings: &Settings,
        query: Option<String>,
    ) -> Result<Option<String>> {
        match query {
            Some(query) => {
                let matches = self.list_versions_matching(settings, &query)?;
                let v = match matches.contains(&query) {
                    true => Some(query),
                    false => matches.last().map(|v| v.to_string()),
                };
                Ok(v)
            }
            None => self.latest_stable_version(settings),
        }
    }

    pub fn latest_installed_version(&self) -> Result<Option<String>> {
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
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            match tv {
                Some(tv) => tv.to_string(),
                None => self.name.to_string(),
            }
        ));
        pr.enable_steady_tick();
    }

    pub fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        match tv.request {
            ToolVersionRequest::System(_) => true,
            _ => tv.install_path().exists() && !self.incomplete_file_path(tv).exists(),
        }
    }

    pub fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
        force: bool,
    ) -> Result<()> {
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
        if let Err(err) = remove_file(self.incomplete_file_path(tv)) {
            debug!("error removing incomplete file: {:?}", err);
        }
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
        Ok(())
    }

    pub fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()> {
        if matches!(self.plugin.get_type(), PluginType::Core) {
            return Ok(());
        }
        self.decorate_progress_bar(pr, None);
        let _lock = self.get_lock(&self.plugin_path, force)?;
        self.plugin.install(config, pr)
    }
    pub fn update(&self, git_ref: Option<String>) -> Result<()> {
        self.plugin.update(git_ref)
    }
    pub fn uninstall(&self, pr: &ProgressReport) -> Result<()> {
        self.plugin.uninstall(pr)
    }

    pub fn external_commands(&self) -> Result<Vec<Vec<String>>> {
        self.plugin.external_commands()
    }
    pub fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
        self.plugin.execute_external_command(command, args)
    }
    pub fn parse_legacy_file(&self, path: &Path, settings: &Settings) -> Result<String> {
        self.plugin.parse_legacy_file(path, settings)
    }
    pub fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        self.plugin.list_bin_paths(config, tv)
    }
    pub fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        self.plugin.exec_env(config, tv)
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
        let _ = remove_file(tv.install_path()); // removes if it is a symlink
        create_dir_all(tv.install_path())?;
        create_dir_all(tv.download_path())?;
        create_dir_all(tv.cache_path())?;
        File::create(self.incomplete_file_path(tv))?;
        Ok(())
    }
    fn cleanup_install_dirs_on_error(&self, settings: &Settings, tv: &ToolVersion) {
        let _ = remove_all_with_warning(tv.install_path());
        self.cleanup_install_dirs(settings, tv);
    }
    fn cleanup_install_dirs(&self, settings: &Settings, tv: &ToolVersion) {
        if !settings.always_keep_download {
            let _ = remove_all_with_warning(tv.download_path());
        }
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
}
