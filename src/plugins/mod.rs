use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use color_eyre::eyre::Result;
use indexmap::IndexMap;

pub use external_plugin::ExternalPlugin;
pub use rtx_plugin_toml::RtxPluginToml;
pub use script_manager::{Script, ScriptManager};

use crate::config::{Config, Settings};
use crate::toolset::ToolVersion;
use crate::ui::progress_report::ProgressReport;

mod external_plugin;
mod external_plugin_cache;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub trait Plugin: Debug + Send + Sync {
    fn get_type(&self) -> PluginType;
    fn name(&self) -> &PluginName;
    fn list_remote_versions(&self, settings: &Settings) -> Result<&Vec<String>>;
    fn latest_stable_version(&self, settings: &Settings) -> Result<Option<String>>;
    fn get_remote_url(&self) -> Option<String> {
        None
    }

    fn is_installed(&self) -> bool {
        true
    }
    fn install(&self, _config: &Config, _pr: &mut ProgressReport) -> Result<()> {
        Ok(())
    }
    fn update(&self, _git_ref: Option<String>) -> Result<()> {
        Ok(())
    }
    fn uninstall(&self, _pr: &ProgressReport) -> Result<()> {
        Ok(())
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

    fn install_version(
        &self,
        config: &Config,
        tv: &ToolVersion,
        pr: &mut ProgressReport,
    ) -> Result<()>;
    fn uninstall_version(&self, config: &Config, tv: &ToolVersion) -> Result<()>;
    fn list_bin_paths(&self, config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>>;
    fn exec_env(&self, config: &Config, tv: &ToolVersion) -> Result<HashMap<String, String>>;
}

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
    use crate::tool::Tool;

    use super::*;

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "tiny");
        let settings = Settings::default();
        let plugin = ExternalPlugin::new(&settings, &PluginName::from("tiny"));
        let tool = Tool::new(plugin.name.clone(), Box::new(plugin));
        let version = tool
            .latest_version(&settings, Some("1.0.0".into()))
            .unwrap()
            .unwrap();
        assert_str_eq!(version, "1.0.0");
        let version = tool.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "3.1.0");
    }

    #[test]
    fn test_latest_stable() {
        let settings = Settings::default();
        let plugin = ExternalPlugin::new(&settings, &PluginName::from("dummy"));
        let tool = Tool::new(plugin.name.clone(), Box::new(plugin));
        let version = tool.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
