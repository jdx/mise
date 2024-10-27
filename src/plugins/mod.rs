use crate::backend::{ABackend, BackendList, BackendType};
use crate::cli::args::BackendArg;
use crate::errors::Error::PluginNotInstalled;
use crate::plugins::asdf_plugin::{AsdfPlugin, ASDF_PLUGIN_NAMES};
use crate::plugins::core::CorePlugin;
use crate::plugins::vfox_plugin::{VfoxPlugin, VFOX_PLUGIN_NAMES};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::{backend, dirs, file};
use clap::Command;
use itertools::Itertools;
use once_cell::sync::Lazy;
use regex::Regex;
pub use script_manager::{Script, ScriptManager};
use std::collections::BTreeMap;
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
    Core,
    Asdf,
    Vfox,
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|([abc])[0-9]+|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

pub fn get(name: &str) -> ABackend {
    BackendArg::new(name, name).into()
}

pub static PLUGIN_NAMES_TO_TYPE: Lazy<BTreeMap<String, PluginType>> = Lazy::new(|| {
    let vfox = VFOX_PLUGIN_NAMES
        .iter()
        .map(|name| (name.clone(), PluginType::Vfox))
        .collect_vec();
    let asdf = ASDF_PLUGIN_NAMES
        .iter()
        .map(|name| (name.clone(), PluginType::Asdf))
        .collect_vec();
    asdf.into_iter().chain(vfox).collect()
});

pub static INSTALLED_PLUGINS: Lazy<Vec<(PathBuf, PluginType)>> =
    Lazy::new(|| match file::dir_subdirs(&dirs::PLUGINS) {
        Ok(dirs) => dirs
            .into_iter()
            .map(|d| {
                let path = dirs::PLUGINS.join(&d);
                let plugin_type = if path.join("metadata.lua").exists() {
                    PluginType::Vfox
                } else {
                    PluginType::Asdf
                };
                (path, plugin_type)
            })
            .collect(),
        Err(e) => {
            warn!("error reading plugin dirs: {e}");
            vec![]
        }
    });

pub fn list() -> BackendList {
    // TODO: replace with list2
    backend::list()
        .into_iter()
        .filter(|f| matches!(f.get_type(), BackendType::Asdf | BackendType::Vfox))
        .collect()
}

pub fn list2() -> eyre::Result<PluginMap> {
    let core = CorePlugin::list()
        .into_iter()
        .map(|p| (p.name().to_string(), p));
    let asdf = AsdfPlugin::list()?
        .into_iter()
        .map(|p| (p.name().to_string(), p));
    let vfox = VfoxPlugin::list()?
        .into_iter()
        .map(|p| (p.name().to_string(), p));
    Ok(core.chain(asdf).chain(vfox).collect())
}

pub fn list_external() -> BackendList {
    list()
        .into_iter()
        .filter(|tool| matches!(tool.get_plugin_type(), PluginType::Asdf | PluginType::Vfox))
        .collect()
}

pub type APlugin = Box<dyn Plugin>;
pub type PluginMap = BTreeMap<String, APlugin>;
pub type PluginList = Vec<APlugin>;

pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn path(&self) -> PathBuf;
    fn get_plugin_type(&self) -> PluginType;
    fn get_remote_url(&self) -> eyre::Result<Option<String>>;
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
    use crate::test::reset;

    #[test]
    fn test_exact_match() {
        reset();
        assert_cli!("plugin", "add", "tiny");
        let plugin = AsdfBackend::from_arg("tiny".into());
        let version = plugin
            .latest_version(Some("1.0.0".into()))
            .unwrap()
            .unwrap();
        assert_str_eq!(version, "1.0.0");
        let version = plugin.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "3.1.0");
    }

    #[test]
    fn test_latest_stable() {
        reset();
        let plugin = AsdfBackend::from_arg("dummy".into());
        let version = plugin.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
