use std::fmt::Debug;
use std::sync::Arc;

use once_cell::sync::Lazy;
use regex::Regex;

pub use external_plugin::ExternalPlugin;
pub use script_manager::{Script, ScriptManager};

use crate::cli::args::ForgeArg;
use crate::forge;
use crate::forge::{Forge, ForgeList, ForgeType};

pub mod core;
mod external_plugin;
mod external_plugin_cache;
mod mise_plugin_toml;
mod script_manager;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginType {
    Core,
    External,
}

pub static VERSION_REGEX: Lazy<regex::Regex> = Lazy::new(|| {
    Regex::new(
        r"(^Available versions:|-src|-dev|-latest|-stm|[-\\.]rc|-milestone|-alpha|-beta|[-\\.]pre|-next|([abc])[0-9]+|snapshot|SNAPSHOT|master)"
    )
        .unwrap()
});

pub fn get(name: &str) -> Arc<dyn Forge> {
    let fa = ForgeArg::new(ForgeType::Asdf, name);
    forge::get(&fa)
}

pub fn list() -> ForgeList {
    forge::list()
        .into_iter()
        .filter(|f| f.get_type() == ForgeType::Asdf)
        .collect()
}

pub fn list_external() -> ForgeList {
    list()
        .into_iter()
        .filter(|tool| tool.get_plugin_type() == PluginType::External)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "tiny");
        let plugin = ExternalPlugin::new(String::from("tiny"));
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
        let plugin = ExternalPlugin::new(String::from("dummy"));
        let version = plugin.latest_version(None).unwrap().unwrap();
        assert_str_eq!(version, "2.0.0");
    }
}
