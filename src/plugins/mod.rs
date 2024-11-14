use crate::errors::Error::PluginNotInstalled;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::vfox_plugin::VfoxPlugin;
use crate::toolset::install_state;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use clap::Command;
use eyre::{eyre, Result};
use once_cell::sync::Lazy;
use regex::Regex;
pub use script_manager::{Script, ScriptManager};
use std::fmt::{Debug, Display};
use std::path::PathBuf;
use std::vec;

pub mod asdf_plugin;
pub mod core;
pub mod mise_plugin_toml;
pub mod script_manager;
pub mod vfox_plugin;

#[derive(Debug, Clone, Copy, PartialEq, strum::EnumString, strum::Display)]
pub enum PluginType {
    Asdf,
    Vfox,
}

impl PluginType {
    pub fn from_full(full: &str) -> eyre::Result<Self> {
        match full.split(':').next() {
            Some("asdf") => Ok(Self::Asdf),
            Some("vfox") => Ok(Self::Vfox),
            _ => Err(eyre!("unknown plugin type: {full}")),
        }
    }

    pub fn plugin(&self, short: String) -> APlugin {
        match self {
            PluginType::Asdf => Box::new(AsdfPlugin::new(short)),
            PluginType::Vfox => Box::new(VfoxPlugin::new(short)),
        }
    }
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|([abc])[0-9]+|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

pub fn get(short: &str) -> Result<APlugin> {
    let (name, full) = short.split_once(':').unwrap_or((short, short));
    let plugin_type = if let Some(plugin_type) = install_state::list_plugins()?.remove(short) {
        plugin_type
    } else {
        PluginType::from_full(full)?
    };
    Ok(plugin_type.plugin(name.to_string()))
}

pub type APlugin = Box<dyn Plugin>;

#[allow(unused_variables)]
pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn path(&self) -> PathBuf;
    fn get_plugin_type(&self) -> PluginType;
    fn get_remote_url(&self) -> eyre::Result<Option<String>>;
    fn set_remote_url(&mut self, url: String) {}
    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>>;
    fn current_sha_short(&self) -> eyre::Result<Option<String>>;
    fn is_installed(&self) -> bool {
        true
    }
    fn is_installed_err(&self) -> eyre::Result<()> {
        if !self.is_installed() {
            return Err(PluginNotInstalled(self.name().to_string()).into());
        }
        Ok(())
    }

    fn ensure_installed(&self, _mpr: &MultiProgressReport, _force: bool) -> eyre::Result<()> {
        Ok(())
    }
    fn update(&self, _pr: &dyn SingleReport, _gitref: Option<String>) -> eyre::Result<()> {
        Ok(())
    }
    fn uninstall(&self, _pr: &dyn SingleReport) -> eyre::Result<()> {
        Ok(())
    }
    fn install(&self, _pr: &dyn SingleReport) -> eyre::Result<()> {
        Ok(())
    }
    fn external_commands(&self) -> eyre::Result<Vec<Command>> {
        Ok(vec![])
    }
    fn execute_external_command(&self, _command: &str, _args: Vec<String>) -> eyre::Result<()> {
        unimplemented!(
            "execute_external_command not implemented for {}",
            self.name()
        )
    }
}

impl Ord for APlugin {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

impl PartialOrd for APlugin {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for APlugin {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for APlugin {}

impl Display for APlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_str_eq;
    use test_log::test;

    use crate::backend::asdf::AsdfBackend;
    use crate::backend::Backend;
    use crate::cli::args::BackendArg;
    use crate::test::reset;

    #[test]
    fn test_exact_match() {
        reset();
        assert_cli!("plugin", "add", "tiny");
        let ba = BackendArg::from("tiny");
        assert_str_eq!(ba.short, "tiny");
        assert_str_eq!(ba.tool_name, "mise-plugins/mise-tiny");
        assert_str_eq!(ba.full(), "asdf:mise-plugins/mise-tiny");
        let backend = AsdfBackend::from_arg("tiny".into());
        let version = backend
            .latest_version(Some("1.0.0".into()))
            .unwrap()
            .unwrap();
        assert_str_eq!(version, "1.0.0");
        let version = backend.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "3.1.0");
    }

    #[test]
    fn test_latest_stable() {
        reset();
        let backend = AsdfBackend::from_arg("dummy".into());
        let version = backend.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
