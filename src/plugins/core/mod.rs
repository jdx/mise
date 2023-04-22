mod nodejs;
mod python;

use crate::plugins::{Plugin, PluginName};
use crate::tool::Tool;
pub use nodejs::NodeJSPlugin;
use once_cell::sync::Lazy;
pub use python::PythonPlugin;
use std::collections::BTreeMap;
use std::sync::Arc;

type ToolMap = BTreeMap<PluginName, Arc<Tool>>;

pub static CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| {
    let tools: Vec<Box<dyn Plugin>> = vec![
        Box::new(PythonPlugin::new("python".to_string())),
        Box::new(NodeJSPlugin::new("nodejs".to_string())),
        Box::new(NodeJSPlugin::new("node".to_string())),
    ];
    ToolMap::from_iter(tools.into_iter().map(|plugin| {
        (
            plugin.name().to_string(),
            Arc::new(Tool::new(plugin.name().to_string(), plugin)),
        )
    }))
});
