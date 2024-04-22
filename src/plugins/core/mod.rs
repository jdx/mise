use std::ffi::OsString;

use std::sync::Arc;

use eyre::Result;
use itertools::Itertools;
use once_cell::sync::Lazy;

pub use python::PythonPlugin;

use crate::cache::CacheManager;
use crate::cli::args::ForgeArg;
use crate::config::Settings;
use crate::env;
use crate::forge::{Forge, ForgeList, ForgeType};
use crate::http::HTTP_FETCH;
use crate::plugins::core::bun::BunPlugin;
use crate::plugins::core::deno::DenoPlugin;
use crate::plugins::core::erlang::ErlangPlugin;
use crate::plugins::core::go::GoPlugin;
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
use crate::plugins::core::ruby::RubyPlugin;
use crate::plugins::core::zig::ZigPlugin;
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;

mod bun;
mod deno;
mod erlang;
mod go;
mod java;
mod node;
mod python;
mod ruby;
mod zig;

pub static CORE_PLUGINS: Lazy<ForgeList> = Lazy::new(|| {
    let mut plugins: Vec<Arc<dyn Forge>> = vec![
        Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
    ];
    let settings = Settings::get();
    if settings.experimental {
        plugins.push(Arc::new(ErlangPlugin::new()));
        plugins.push(Arc::new(ZigPlugin::new()));
    }
    plugins
});

#[derive(Debug)]
pub struct CorePlugin {
    pub fa: ForgeArg,
    pub name: &'static str,
    pub remote_version_cache: CacheManager<Vec<String>>,
}

impl CorePlugin {
    pub fn new(name: &'static str) -> Self {
        let fa = ForgeArg::new(ForgeType::Asdf, name);
        Self {
            name,
            remote_version_cache: CacheManager::new(
                fa.cache_path.join("remote_versions.msgpack.z"),
            )
            .with_fresh_duration(*env::MISE_FETCH_REMOTE_VERSIONS_CACHE),
            fa,
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
