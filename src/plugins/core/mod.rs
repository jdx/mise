use std::collections::BTreeMap;
use std::sync::Arc;

use once_cell::sync::Lazy;

pub use python::PythonPlugin;

use crate::plugins::core::node::NodePlugin;
use crate::plugins::{Plugin, PluginName};
use crate::tool::Tool;

mod node;
mod python;

type ToolMap = BTreeMap<PluginName, Arc<Tool>>;

pub static CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| {
    build_core_plugins(vec![
        Box::new(NodePlugin::new("node".to_string()).with_legacy_file_support()),
        Box::new(PythonPlugin::new("python".to_string())),
    ])
});

pub static EXPERIMENTAL_CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| build_core_plugins(vec![]));

fn build_core_plugins(tools: Vec<Box<dyn Plugin>>) -> ToolMap {
    ToolMap::from_iter(tools.into_iter().map(|plugin| {
        (
            plugin.name().to_string(),
            Arc::new(Tool::new(plugin.name().to_string(), plugin)),
        )
    }))
}
