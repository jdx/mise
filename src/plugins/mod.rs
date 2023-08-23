use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use clap::Command;
use color_eyre::eyre::Result;
use console::style;

pub use external_plugin::ExternalPlugin;
pub use rtx_plugin_toml::RtxPluginToml;
pub use script_manager::{Script, ScriptManager};

use crate::config::{Config, Settings};
use crate::file;
use crate::file::display_path;
use crate::lock_file::LockFile;
use crate::toolset::ToolVersion;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::{ProgressReport, PROG_TEMPLATE};

pub mod core;
mod external_plugin;
mod external_plugin_cache;
mod rtx_plugin_toml;
mod script_manager;

pub type PluginName = String;

pub trait Plugin: Debug + Send + Sync {
    fn name(&self) -> &PluginName;
    fn get_type(&self) -> PluginType {
        PluginType::Core
    }
    fn list_remote_versions(&self, settings: &Settings) -> Result<Vec<String>>;
    fn latest_stable_version(&self, _settings: &Settings) -> Result<Option<String>> {
        Ok(None)
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
    fn install_version(&self, config: &Config, tv: &ToolVersion, pr: &ProgressReport)
        -> Result<()>;
    fn uninstall_version(&self, _config: &Config, _tv: &ToolVersion) -> Result<()> {
        Ok(())
    }
    fn list_bin_paths(&self, _config: &Config, tv: &ToolVersion) -> Result<Vec<PathBuf>> {
        Ok(vec![tv.install_path().join("bin")])
    }
    fn exec_env(&self, _config: &Config, _tv: &ToolVersion) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
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
}

pub fn unalias_plugin(plugin_name: &str) -> &str {
    match plugin_name {
        "nodejs" => "node",
        "golang" => "go",
        _ => plugin_name,
    }
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
        let plugin = ExternalPlugin::new(&PluginName::from("tiny"));
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
        let plugin = ExternalPlugin::new(&PluginName::from("dummy"));
        let tool = Tool::new(plugin.name.clone(), Box::new(plugin));
        let version = tool.latest_version(&settings, None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
