use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{Debug, Display};

use std::path::{Path, PathBuf};

use clap::Command;
use color_eyre::eyre::Result;

use crate::config::{Config, Settings};

use crate::dirs;
use crate::install_context::InstallContext;
use crate::plugins::Plugin;
use crate::toolset::{ToolVersion, Toolset};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::ProgressReport;

#[derive(Debug)]
pub struct Tool {
    pub name: String,
    pub plugin: Box<dyn Plugin>,
    pub plugin_path: PathBuf,
    pub installs_path: PathBuf,
    pub cache_path: PathBuf,
    pub downloads_path: PathBuf,
}

impl Tool {
    pub fn new(name: String, plugin: Box<dyn Plugin>) -> Self {
        Self {
            plugin_path: dirs::PLUGINS.join(&name),
            installs_path: dirs::INSTALLS.join(&name),
            cache_path: dirs::CACHE.join(&name),
            downloads_path: dirs::DOWNLOADS.join(&name),
            name,
            plugin,
        }
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
        self.plugin.list_installed_versions()
    }

    pub fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
        self.plugin.list_installed_versions_matching(query)
    }

    pub fn list_remote_versions(&self, settings: &Settings) -> Result<Vec<String>> {
        self.plugin.list_remote_versions(settings)
    }

    pub fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        self.plugin.list_versions_matching(settings, query)
    }

    pub fn latest_version(
        &self,
        settings: &Settings,
        query: Option<String>,
    ) -> Result<Option<String>> {
        self.plugin.latest_version(settings, query)
    }

    pub fn latest_installed_version(&self, query: Option<String>) -> Result<Option<String>> {
        self.plugin.latest_installed_version(query)
    }

    pub fn get_aliases(&self, settings: &Settings) -> Result<BTreeMap<String, String>> {
        self.plugin.get_aliases(settings)
    }

    pub fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        self.plugin.legacy_filenames(settings)
    }

    pub fn decorate_progress_bar(&self, pr: &mut ProgressReport, tv: Option<&ToolVersion>) {
        self.plugin.decorate_progress_bar(pr, tv);
    }

    pub fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        self.plugin.is_version_installed(tv)
    }

    pub fn is_version_outdated(&self, config: &Config, tv: &ToolVersion) -> bool {
        self.plugin.is_version_outdated(self, config, tv)
    }

    pub fn symlink_path(&self, tv: &ToolVersion) -> Option<PathBuf> {
        self.plugin.symlink_path(tv)
    }

    pub fn create_symlink(&self, version: &str, target: &Path) -> Result<()> {
        self.plugin.create_symlink(version, target)
    }

    pub fn install_version(&self, ctx: InstallContext) -> Result<()> {
        self.plugin.install_version(ctx)
    }

    pub fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()> {
        self.plugin.uninstall_version(config, tv, pr, dryrun)
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
    pub fn purge(&self, pr: &ProgressReport) -> Result<()> {
        self.plugin.purge(pr)
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
    pub fn list_bin_paths(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<Vec<PathBuf>> {
        self.plugin.list_bin_paths(config, ts, tv)
    }
    pub fn exec_env(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
    ) -> Result<HashMap<String, String>> {
        self.plugin.exec_env(config, ts, tv)
    }

    pub fn which(
        &self,
        config: &Config,
        ts: &Toolset,
        tv: &ToolVersion,
        bin_name: &str,
    ) -> Result<Option<PathBuf>> {
        self.plugin.which(config, ts, tv, bin_name)
    }
}

impl PartialEq for Tool {
    fn eq(&self, other: &Self) -> bool {
        self.plugin_path == other.plugin_path
    }
}

impl Display for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.name)
    }
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
