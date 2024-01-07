use std::collections::BTreeMap;
use std::ffi::OsString;
use std::iter::Iterator;
use std::path::PathBuf;
use std::sync::Arc;

use itertools::Itertools;
use miette::{IntoDiagnostic, Result};
use once_cell::sync::Lazy;

pub use python::PythonPlugin;

use crate::cache::CacheManager;
use crate::http::HTTP_FETCH;
use crate::plugins::core::bun::BunPlugin;
use crate::plugins::core::deno::DenoPlugin;
use crate::plugins::core::erlang::ErlangPlugin;
use crate::plugins::core::go::GoPlugin;
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
use crate::plugins::core::ruby::RubyPlugin;
use crate::plugins::Plugin;
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;
use crate::{dirs, env};

mod bun;
mod deno;
mod erlang;
mod go;
mod java;
mod node;
mod python;
mod ruby;

pub type PluginMap = BTreeMap<String, Arc<dyn Plugin>>;

pub static CORE_PLUGINS: Lazy<PluginMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Plugin>> = vec![
        Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
    ];
    plugins
        .into_iter()
        .map(|plugin| (plugin.name().to_string(), plugin))
        .collect()
});

pub static EXPERIMENTAL_CORE_PLUGINS: Lazy<PluginMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Plugin>> = vec![Arc::new(ErlangPlugin::new())];
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
                .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            cache_path,
        }
    }

    pub fn path_env_with_tv_path(tv: &ToolVersion) -> Result<OsString> {
        let mut path = env::split_paths(&env::var_os("PATH").unwrap()).collect::<Vec<_>>();
        path.insert(0, tv.install_path().join("bin"));
        env::join_paths(path).into_diagnostic()
    }

    pub fn run_fetch_task_with_timeout<F, T>(f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T> + Send,
        T: Send,
    {
        run_with_timeout(f, *env::MISE_FETCH_REMOTE_VERSIONS_TIMEOUT)
    }

    pub fn fetch_remote_versions_from_mise(&self) -> Result<Option<Vec<String>>> {
        if !*env::MISE_USE_VERSIONS_HOST {
            return Ok(None);
        }
        let raw = HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", &self.name))?;
        let versions = raw
            .lines()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect_vec();
        Ok(Some(versions))
    }
}
