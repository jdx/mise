use eyre::Result;
use itertools::Itertools;
use once_cell::sync::Lazy;
use std::ffi::OsString;
use std::sync::Arc;

pub use python::PythonPlugin;

use crate::backend::{Backend, BackendMap};
use crate::cache::CacheManager;
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::env;
use crate::env::PATH_KEY;
use crate::http::HTTP_FETCH;
#[cfg(unix)]
use crate::plugins::core::bun::BunPlugin;
#[cfg(unix)]
use crate::plugins::core::deno::DenoPlugin;
#[cfg(unix)]
use crate::plugins::core::erlang::ErlangPlugin;
#[cfg(unix)]
use crate::plugins::core::go::GoPlugin;
#[cfg(unix)]
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
#[cfg(unix)]
use crate::plugins::core::ruby::RubyPlugin;
#[cfg(unix)]
use crate::plugins::core::zig::ZigPlugin;
use crate::plugins::{Plugin, PluginList, PluginType};
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;

#[cfg(unix)]
mod bun;
#[cfg(unix)]
mod deno;
#[cfg(unix)]
mod erlang;
#[cfg(unix)]
mod go;
#[cfg(unix)]
mod java;
mod node;
mod python;
#[cfg(unix)]
mod ruby;
#[cfg(unix)]
mod zig;

pub static CORE_PLUGINS: Lazy<BackendMap> = Lazy::new(|| {
    #[cfg(unix)]
    let mut plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        Arc::new(ErlangPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
    ];
    #[cfg(windows)]
    let mut plugins: Vec<Arc<dyn Backend>> = vec![
        // Arc::new(BunPlugin::new()),
        // Arc::new(DenoPlugin::new()),
        // Arc::new(ErlangPlugin::new()),
        // Arc::new(GoPlugin::new()),
        // Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        // Arc::new(RubyPlugin::new()),
    ];
    #[cfg(unix)]
    {
        let settings = Settings::get();
        if settings.experimental {
            plugins.push(Arc::new(ZigPlugin::new()));
        }
    }
    plugins
        .into_iter()
        .map(|p| (p.id().to_string(), p))
        .collect()
});

#[derive(Debug)]
pub struct CorePlugin {
    pub fa: BackendArg,
    pub remote_version_cache: CacheManager<Vec<String>>,
}

impl CorePlugin {
    pub fn list() -> PluginList {
        let settings = Settings::get();
        CORE_PLUGINS
            .iter()
            .map(|(id, _)| Box::new(CorePlugin::new(id.to_string().into())) as Box<dyn Plugin>)
            .filter(|p| !settings.disable_tools.contains(p.name()))
            .collect()
    }

    pub fn new(fa: BackendArg) -> Self {
        Self {
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions-$KEY.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            fa,
        }
    }

    pub fn path_env_with_tv_path(tv: &ToolVersion) -> Result<OsString> {
        let mut path = env::split_paths(&env::var_os(&*PATH_KEY).unwrap()).collect::<Vec<_>>();
        path.insert(0, tv.install_path().join("bin"));
        Ok(env::join_paths(path)?)
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
        // using http is not a security concern and enabling tls makes mise significantly slower
        let raw = HTTP_FETCH.get_text(format!("http://mise-versions.jdx.dev/{}", &self.fa.name))?;
        let versions = raw
            .lines()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect_vec();
        Ok(Some(versions))
    }
}

impl Plugin for CorePlugin {
    fn name(&self) -> &str {
        &self.fa.name
    }

    fn get_plugin_type(&self) -> PluginType {
        PluginType::Core
    }

    fn get_remote_url(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn current_abbrev_ref(&self) -> Result<Option<String>> {
        Ok(None)
    }

    fn current_sha_short(&self) -> Result<Option<String>> {
        Ok(None)
    }
}
