use std::fmt::Debug;
use std::path::Path;

use color_eyre::eyre::Result;
use console::style;
use indexmap::IndexMap;
use regex::Regex;

pub use external_plugin::ExternalPlugin;
pub use script_manager::{Script, ScriptManager};

use crate::config::Settings;
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};

mod external_plugin;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub trait Plugin: Debug + Send + Sync {
    fn name(&self) -> &PluginName;
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

    fn decorate_progress_bar(&self, pr: &mut ProgressReport) {
        pr.set_style(PROG_TEMPLATE.clone());
        pr.set_prefix(format!(
            "{} {} ",
            style("rtx").dim().for_stderr(),
            style(self.name()).cyan().for_stderr()
        ));
        pr.enable_steady_tick();
    }
}

#[derive(Debug)]
pub enum Plugins {
    External(ExternalPlugin),
}

impl Plugin for Plugins {
    fn name(&self) -> &PluginName {
        match self {
            Plugins::External(p) => p.name(),
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
    fn decorate_progress_bar(&self, pr: &mut ProgressReport) {
        match self {
            Plugins::External(p) => p.decorate_progress_bar(pr),
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
