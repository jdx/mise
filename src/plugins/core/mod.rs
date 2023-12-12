use std::collections::BTreeMap;
use std::ffi::OsString;
use std::iter::Iterator;
use std::path::PathBuf;
use std::sync::Arc;

use color_eyre::eyre::Result;
use itertools::Itertools;
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
use crate::plugins::{Plugin, HTTP};
use crate::timeout::run_with_timeout;
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

pub type PluginMap = BTreeMap<String, Arc<dyn Plugin>>;

pub static CORE_PLUGINS: Lazy<PluginMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        if *RTX_NODE_BUILD == Some(true) {
            Arc::new(NodeBuildPlugin::new())
        } else {
            Arc::new(NodePlugin::new())
        },
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
    ];
    plugins
        .into_iter()
        .map(|plugin| (plugin.name().to_string(), plugin))
        .collect()
});

pub static EXPERIMENTAL_CORE_PLUGINS: Lazy<PluginMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Plugin>> =
        vec![Arc::new(BunPlugin::new()), Arc::new(DenoPlugin::new())];
    plugins
        .into_iter()
        .map(|plugin| (plugin.name().to_string(), plugin))
        .collect()
});

#[derive(Debug)]
pub struct CorePlugin {
    pub name: &'static str,
    pub cache_path: PathBuf,
    pub remote_version_cache: CacheManager<Vec<String>>,
}

impl CorePlugin {
    pub fn new(name: &'static str) -> Self {
        let cache_path = dirs::CACHE.join(name);
        Self {
            name,
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

    pub fn fetch_remote_versions_from_rtx(&self) -> Result<Option<Vec<String>>> {
        if !*env::RTX_USE_VERSIONS_HOST {
            return Ok(None);
        }
        let versions = HTTP
            .get_text(format!("http://rtx-versions.jdx.dev/{}", &self.name))?
            .lines()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect_vec();
        Ok(Some(versions))
    }
}
