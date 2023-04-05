use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use console::style;
use indexmap::IndexMap;
use regex::Regex;

pub use external_plugin::ExternalPlugin;
pub use script_manager::{Script, ScriptManager};

use crate::config::{Config, Settings};
use crate::dirs;
use crate::hash::hash_to_str;
use crate::plugins::rtx_plugin_toml::RtxPluginToml;
use crate::toolset::{ToolVersion, ToolVersionRequest};
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};

mod external_plugin;
mod external_plugin_cache;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub trait Plugin: Debug + Send + Sync + Eq + PartialEq + Hash {
    fn name(&self) -> &PluginName;
    fn toml(&self) -> &RtxPluginToml;
    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>>;
    fn clear_remote_version_cache(&self) -> Result<()>;
    fn list_installed_versions(&self) -> Result<Vec<String>>;
    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>>;
    fn latest_installed_version(&self) -> Result<Option<String>>;

    fn is_installed(&self) -> bool {
        true
    }

    fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
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
    fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
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
    fn get_aliases(&self, _settings: &Settings) -> Result<IndexMap<String, String>> {
        Ok(IndexMap::new())
    }
    fn legacy_filenames(&self, _settings: &Settings) -> Result<Vec<String>> {
        Ok(vec![])
    }
    fn parse_legacy_file(&self, _path: &Path, _settings: &Settings) -> Result<String> {
        unimplemented!()
    }
    fn external_commands(&self) -> Result<Vec<Vec<String>>> {
        Ok(vec![])
    }
    fn execute_external_command(&self, _command: &str, _args: Vec<String>) -> Result<()> {
        unimplemented!()
    }

    fn decorate_progress_bar(&self, pr: &mut ProgressReport, tv: Option<&ToolVersion>) {
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            match tv {
                Some(tv) => tv.to_string(),
                None => self.name().to_string(),
            }
        ));
        pr.enable_steady_tick();
    }

    fn is_version_installed(&self, tv: &ToolVersion) -> bool;
    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
        force: bool,
    ) -> Result<()>;
    fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()>;
    fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>>;
    fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>>;
    fn which(&self, config: &Config, tv: &ToolVersion, bin_name: &str) -> Result<Option<PathBuf>>;
    fn install_path(&self, tv: &ToolVersion) -> PathBuf {
        let pathname = match &tv.request {
            ToolVersionRequest::Path(_, p) => p.to_string_lossy().to_string(),
            _ => self.tv_pathname(tv),
        };
        dirs::INSTALLS.join(self.name()).join(pathname)
    }
    fn cache_path(&self, tv: &ToolVersion) -> PathBuf {
        dirs::CACHE.join(self.name()).join(self.tv_pathname(tv))
    }
    fn download_path(&self, tv: &ToolVersion) -> PathBuf {
        dirs::DOWNLOADS.join(self.name()).join(self.tv_pathname(tv))
    }
    fn tv_pathname(&self, tv: &ToolVersion) -> String {
        match &tv.request {
            ToolVersionRequest::Version(_, _) => tv.version.to_string(),
            ToolVersionRequest::Prefix(_, _) => tv.version.to_string(),
            ToolVersionRequest::Ref(_, r) => format!("ref-{}", r),
            ToolVersionRequest::Path(_, p) => format!("path-{}", hash_to_str(p)),
            ToolVersionRequest::System(_) => "system".to_string(),
        }
    }
}

#[derive(Debug)]
pub enum Plugins {
    External(ExternalPlugin),
}

impl Eq for Plugins {}

impl PartialEq<Self> for Plugins {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Plugins::External(p1), Plugins::External(p2)) => p1 == p2,
        }
    }
}
impl Hash for Plugins {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Plugins::External(p) => p.hash(state),
        }
    }
}

impl Plugin for Plugins {
    fn name(&self) -> &PluginName {
        match self {
            Plugins::External(p) => p.name(),
        }
    }
    fn toml(&self) -> &RtxPluginToml {
        match self {
            Plugins::External(p) => p.toml(),
        }
    }

    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        match self {
            Plugins::External(p) => p.list_remote_versions(settings),
        }
    }

    fn clear_remote_version_cache(&self) -> Result<()> {
        match self {
            Plugins::External(p) => p.clear_remote_version_cache(),
        }
    }
    fn list_installed_versions(&self) -> Result<Vec<String>> {
        match self {
            Plugins::External(p) => p.list_installed_versions(),
        }
    }
    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>> {
        match self {
            Plugins::External(p) => p.latest_version(settings, query),
        }
    }
    fn latest_installed_version(&self) -> Result<Option<String>> {
        match self {
            Plugins::External(p) => p.latest_installed_version(),
        }
    }
    fn is_installed(&self) -> bool {
        match self {
            Plugins::External(p) => p.is_installed(),
        }
    }
    fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>> {
        match self {
            Plugins::External(p) => p.list_installed_versions_matching(query),
        }
    }
    fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>> {
        match self {
            Plugins::External(p) => p.list_versions_matching(settings, query),
        }
    }
    fn get_aliases(&self, settings: &Settings) -> Result<IndexMap<String, String>> {
        match self {
            Plugins::External(p) => p.get_aliases(settings),
        }
    }
    fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>> {
        match self {
            Plugins::External(p) => p.legacy_filenames(settings),
        }
    }
    fn parse_legacy_file(&self, path: &Path, settings: &Settings) -> Result<String> {
        match self {
            Plugins::External(p) => p.parse_legacy_file(path, settings),
        }
    }
    fn external_commands(&self) -> Result<Vec<Vec<String>>> {
        match self {
            Plugins::External(p) => p.external_commands(),
        }
    }
    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()> {
        match self {
            Plugins::External(p) => p.execute_external_command(command, args),
        }
    }
    fn decorate_progress_bar(&self, pr: &mut ProgressReport, tv: Option<&ToolVersion>) {
        match self {
            Plugins::External(p) => p.decorate_progress_bar(pr, tv),
        }
    }
    fn is_version_installed(&self, tv: &ToolVersion) -> bool {
        match self {
            Plugins::External(p) => p.is_version_installed(tv),
        }
    }
    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
        force: bool,
    ) -> Result<()> {
        match self {
            Plugins::External(p) => p.install_version(config, tv, pr, force),
        }
    }
    fn uninstall_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &ProgressReport,
        dryrun: bool,
    ) -> Result<()> {
        match self {
            Plugins::External(p) => p.uninstall_version(config, tv, pr, dryrun),
        }
    }
    fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        match self {
            Plugins::External(p) => p.list_bin_paths(config, tv),
        }
    }
    fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>> {
        match self {
            Plugins::External(p) => p.exec_env(config, tv),
        }
    }
    fn which(&self, config: &Config, tv: &ToolVersion, bin_name: &str) -> Result<Option<PathBuf>> {
        match self {
            Plugins::External(p) => p.which(config, tv, bin_name),
        }
    }
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
        let plugin = ExternalPlugin::new(&settings, &PluginName::from("tiny"));
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
        let plugin = ExternalPlugin::new(&settings, &PluginName::from("dummy"));
        let version = plugin.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
