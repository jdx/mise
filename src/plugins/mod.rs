pub use external_plugin::ExternalPlugin;

use once_cell::sync::Lazy;
use regex::Regex;
pub use script_manager::{Script, ScriptManager};
use std::fmt::{Debug, Display};
use std::hash::Hash;

use crate::forge::Forge;

pub mod core;
mod external_plugin;
mod external_plugin_cache;
mod mise_plugin_toml;
mod script_manager;

pub fn unalias_plugin(plugin_name: &str) -> &str {
    match plugin_name {
        "nodejs" => "node",
        "golang" => "go",
        _ => plugin_name,
    }
}

impl Display for dyn Forge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
impl Eq for dyn Forge {}
impl PartialEq for dyn Forge {
    fn eq(&self, other: &Self) -> bool {
        self.get_type() == other.get_type() && self.name() == other.name()
    }
}
impl Hash for dyn Forge {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name().hash(state)
    }
}
impl PartialOrd for dyn Forge {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for dyn Forge {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name().cmp(other.name())
    }
}

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

#[cfg(test)]
mod tests {
    use crate::forge::Forge;
    use pretty_assertions::assert_str_eq;

    use super::*;

    #[test]
    fn test_exact_match() {
        assert_cli!("plugin", "add", "tiny");
        let plugin = ExternalPlugin::newa(String::from("tiny"));
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
