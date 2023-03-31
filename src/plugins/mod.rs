use std::fmt::Debug;
use std::path::Path;

use color_eyre::eyre::Result;
use indexmap::IndexMap;

pub use external_plugin::ExternalPlugin;
pub use script_manager::{Script, ScriptManager};

use crate::config::{Config, Settings};
use crate::ui::progress_report::ProgressReport;

mod external_plugin;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub trait Plugin: Debug + Send + Sync {
    fn name(&self) -> &PluginName;
    fn is_installed(&self) -> bool;
    fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()>;
    fn update(&self, gitref: Option<String>) -> Result<()>;
    fn uninstall(&self, pr: &ProgressReport) -> Result<()>;
    fn latest_installed_version(&self) -> Result<Option<String>>;
    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>>;
    fn list_installed_versions_matching(&self, query: &str) -> Result<Vec<String>>;
    fn list_versions_matching(&self, settings: &Settings, query: &str) -> Result<Vec<String>>;
    fn list_installed_versions(&self) -> Result<Vec<String>>;
    fn clear_remote_version_cache(&self) -> Result<()>;
    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>>;
    fn get_aliases(&self, settings: &Settings) -> Result<IndexMap<String, String>>;
    fn legacy_filenames(&self, settings: &Settings) -> Result<Vec<String>>;
    fn parse_legacy_file(&self, path: &Path, settings: &Settings) -> Result<String>;
    fn external_commands(&self) -> Result<Vec<Vec<String>>>;
    fn execute_external_command(&self, command: &str, args: Vec<String>) -> Result<()>;
    fn decorate_progress_bar(&self, pr: &mut ProgressReport);
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

    fn is_installed(&self) -> bool {
        match self {
            Plugins::External(p) => p.is_installed(),
        }
    }
    fn install(&self, config: &Config, pr: &mut ProgressReport, force: bool) -> Result<()> {
        match self {
            Plugins::External(p) => p.install(config, pr, force),
        }
    }

    fn update(&self, gitref: Option<String>) -> Result<()> {
        match self {
            Plugins::External(p) => p.update(gitref),
        }
    }

    fn uninstall(&self, pr: &ProgressReport) -> Result<()> {
        match self {
            Plugins::External(p) => p.uninstall(pr),
        }
    }
    fn latest_installed_version(&self) -> Result<Option<String>> {
        match self {
            Plugins::External(p) => p.latest_installed_version(),
        }
    }
    fn latest_version(&self, settings: &Settings, query: Option<String>) -> Result<Option<String>> {
        match self {
            Plugins::External(p) => p.latest_version(settings, query),
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
    fn list_installed_versions(&self) -> Result<Vec<String>> {
        match self {
            Plugins::External(p) => p.list_installed_versions(),
        }
    }
    fn clear_remote_version_cache(&self) -> Result<()> {
        match self {
            Plugins::External(p) => p.clear_remote_version_cache(),
        }
    }
    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>> {
        match self {
            Plugins::External(p) => p.list_remote_versions(settings),
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
