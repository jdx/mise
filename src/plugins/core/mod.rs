use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use once_cell::sync::Lazy;

pub use python::PythonPlugin;

use crate::cache::CacheManager;
use crate::env::RTX_NODE_BUILD;
use crate::plugins::core::bun::BunPlugin;
use crate::plugins::core::deno::DenoPlugin;
use crate::plugins::core::go::GoPlugin;
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
use crate::plugins::core::node_build::NodeBuildPlugin;
use crate::plugins::core::ruby::RubyPlugin;
use crate::plugins::{Plugin, PluginName};
use crate::timeout::run_with_timeout;
use crate::tool::Tool;
use crate::toolset::ToolVersion;
use crate::{dirs, env};

mod bun;
mod deno;
mod go;
mod java;
mod node;
mod node_build;
mod python;
mod ruby;

type ToolMap = BTreeMap<PluginName, Arc<Tool>>;

pub static CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| {
    build_core_plugins(vec![
        Box::new(GoPlugin::new()),
        Box::new(JavaPlugin::new()),
        if *RTX_NODE_BUILD == Some(true) {
            Box::new(NodeBuildPlugin::new())
        } else {
            Box::new(NodePlugin::new())
        },
        Box::new(PythonPlugin::new()),
        Box::new(RubyPlugin::new()),
    ])
});

pub static EXPERIMENTAL_CORE_PLUGINS: Lazy<ToolMap> = Lazy::new(|| {
    build_core_plugins(vec![
        Box::new(BunPlugin::new()),
        Box::new(DenoPlugin::new()),
    ])
});

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
    pub cache_path: PathBuf,
    pub remote_version_cache: CacheManager<Vec<String>>,
}

impl CorePlugin {
    pub fn new(name: &'static str) -> Self {
        let cache_path = dirs::CACHE.join(name);
        Self {
            remote_version_cache: CacheManager::new(cache_path.join("remote_versions.msgpack.z"))
                .with_fresh_duration(*env::RTX_FETCH_REMOTE_VERSIONS_CACHE),
            cache_path,
        }
    }

    pub fn path_env_with_tv_path(tv: &ToolVersion) -> Result<OsString> {
        let mut path = env::split_paths(&env::var_os("PATH").unwrap()).collect::<Vec<_>>();
        path.insert(0, tv.install_path().join("bin"));
        Ok(env::join_paths(path)?)
    }

    pub fn run_fetch_task_with_timeout<F, T>(f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send,
        T: Send,
    {
        run_with_timeout(f, *env::RTX_FETCH_REMOTE_VERSIONS_TIMEOUT)
    }
}
