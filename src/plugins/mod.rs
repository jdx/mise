use console::style;
use eyre::WrapErr;
use std::collections::BTreeMap;
use std::fmt::{Debug, Display};
use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

pub use script_manager::{Script, ScriptManager};

use crate::backend;
use crate::backend::{ABackend, BackendList, BackendType};
use crate::cli::args::BackendArg;
use crate::plugins::asdf_plugin::AsdfPlugin;
use crate::plugins::core::CorePlugin;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;

pub mod asdf_plugin;
pub mod core;
pub mod mise_plugin_toml;
pub mod script_manager;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginType {
    Core,
    Asdf,
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|([abc])[0-9]+|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

pub fn get(name: &str) -> ABackend {
    BackendArg::new(BackendType::Asdf, name).into()
}

pub fn get2(name: &str) -> APlugin {
    Box::new(AsdfPlugin::new(name.to_string())) as APlugin
}

pub fn list() -> BackendList {
    backend::list()
        .into_iter()
        .filter(|f| f.get_type() == BackendType::Asdf)
        .collect()
}

pub fn list2() -> eyre::Result<PluginMap> {
    let core = CorePlugin::list()
        .into_iter()
        .map(|p| (p.name().to_string(), p));
    let asdf = AsdfPlugin::list()?
        .into_iter()
        .map(|p| (p.name().to_string(), p));
    Ok(core.chain(asdf).collect())
}

pub fn list_external() -> BackendList {
    list()
        .into_iter()
        .filter(|tool| tool.get_plugin_type() == PluginType::Asdf)
        .collect()
}

pub type APlugin = Box<dyn Plugin>;
pub type PluginMap = BTreeMap<String, APlugin>;
pub type PluginList = Vec<APlugin>;

pub trait Plugin: Debug + Send {
    fn name(&self) -> &str;
    fn get_plugin_type(&self) -> PluginType;
    fn get_remote_url(&self) -> eyre::Result<Option<String>>;
    fn current_abbrev_ref(&self) -> eyre::Result<Option<String>>;
    fn current_sha_short(&self) -> eyre::Result<Option<String>>;
    fn is_installed(&self) -> bool;

    fn ensure_installed(&self, mpr: &MultiProgressReport, force: bool) -> eyre::Result<()>;
    fn update(&self, pr: &dyn SingleReport, gitref: Option<String>) -> eyre::Result<()>;
    fn uninstall(&self, pr: &dyn SingleReport) -> eyre::Result<()>;
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

    use crate::backend::asdf::Asdf;
    use crate::backend::Backend;
    use crate::test::reset;

    #[test]
    fn test_exact_match() {
        reset();
        assert_cli!("plugin", "add", "tiny");
        let plugin = Asdf::new(String::from("tiny"));
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
        let plugin = Asdf::new(String::from("dummy"));
        let version = plugin.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
