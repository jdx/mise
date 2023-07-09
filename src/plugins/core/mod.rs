use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use once_cell::sync::Lazy;

use crate::cache::CacheManager;
use crate::dirs;
use crate::env::RTX_EXE;
pub use python::PythonPlugin;

use crate::plugins::core::node::NodePlugin;
use crate::plugins::{Plugin, PluginName};
use crate::tool::Tool;

mod node;
mod python;

type ToolMap = BTreeMap<PluginName, Arc<Tool>>;

pub static CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| {
    build_core_plugins(vec![
        Box::new(NodePlugin::new("node".to_string())),
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

#[derive(Debug)]
pub struct CorePlugin {
    pub name: PluginName,
    pub cache_path: PathBuf,
    pub remote_version_cache: CacheManager<Vec<String>>,
}

impl CorePlugin {
    pub fn new(name: PluginName) -> Self {
        let cache_path = dirs::CACHE.join(&name);
        let fresh_duration = Some(Duration::from_secs(60 * 60 * 12)); // 12 hours
        Self {
            remote_version_cache: CacheManager::new(cache_path.join("remote_versions.msgpack.z"))
                .with_fresh_duration(fresh_duration)
                .with_fresh_file(RTX_EXE.clone()),
            name,
            cache_path,
        }
    }
}
