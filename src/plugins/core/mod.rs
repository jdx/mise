use eyre::Result;
use once_cell::sync::Lazy;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

pub use python::PythonPlugin;

use crate::backend::{Backend, BackendMap};
use crate::cli::args::BackendArg;
use crate::config::{Settings, SETTINGS};
use crate::env::PATH_KEY;
#[cfg(unix)]
use crate::plugins::core::bun::BunPlugin;
use crate::plugins::core::deno::DenoPlugin;
#[cfg(unix)]
use crate::plugins::core::erlang::ErlangPlugin;
use crate::plugins::core::go::GoPlugin;
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
use crate::plugins::core::ruby::RubyPlugin;
#[cfg(unix)]
use crate::plugins::core::zig::ZigPlugin;
use crate::plugins::{Plugin, PluginList, PluginType};
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;
use crate::{dirs, env};

#[cfg(unix)]
mod bun;
mod deno;
#[cfg(unix)]
mod erlang;
mod go;
mod java;
mod node;
mod python;
#[cfg_attr(windows, path = "ruby_windows.rs")]
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
    let plugins: Vec<Arc<dyn Backend>> = vec![
        // Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        // Arc::new(ErlangPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
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

// TODO: remove this struct
#[derive(Debug)]
pub struct CorePlugin {
    pub fa: BackendArg,
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
        Self { fa }
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
        run_with_timeout(f, SETTINGS.fetch_remote_versions_timeout())
    }
}

// TODO: remove this since core "plugins" are not plugins, this is legacy from when it was just asdf/core
impl Plugin for CorePlugin {
    fn name(&self) -> &str {
        &self.fa.name
    }

    fn path(&self) -> PathBuf {
        dirs::PLUGINS.join(self.name())
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
