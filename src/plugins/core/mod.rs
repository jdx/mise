use eyre::Result;
use once_cell::sync::Lazy;
use std::ffi::OsString;
use std::sync::Arc;

pub use python::PythonPlugin;

use crate::backend::{Backend, BackendMap};
use crate::cli::args::BackendArg;
use crate::config::SETTINGS;
use crate::env;
use crate::env::PATH_KEY;
use crate::plugins::core::bun::BunPlugin;
use crate::plugins::core::deno::DenoPlugin;
#[cfg(unix)]
use crate::plugins::core::erlang::ErlangPlugin;
use crate::plugins::core::go::GoPlugin;
use crate::plugins::core::java::JavaPlugin;
use crate::plugins::core::node::NodePlugin;
use crate::plugins::core::ruby::RubyPlugin;
use crate::plugins::core::rust::RustPlugin;
#[cfg(unix)]
use crate::plugins::core::zig::ZigPlugin;
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;

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
mod rust;
#[cfg(unix)]
mod zig;

pub static CORE_PLUGINS: Lazy<BackendMap> = Lazy::new(|| {
    #[cfg(unix)]
    let plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        Arc::new(ErlangPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
        Arc::new(RustPlugin::new()),
        Arc::new(ZigPlugin::new()),
    ];
    #[cfg(windows)]
    let plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(BunPlugin::new()),
        Arc::new(DenoPlugin::new()),
        // Arc::new(ErlangPlugin::new()),
        Arc::new(GoPlugin::new()),
        Arc::new(JavaPlugin::new()),
        Arc::new(NodePlugin::new()),
        Arc::new(PythonPlugin::new()),
        Arc::new(RubyPlugin::new()),
        Arc::new(RustPlugin::new()),
        // Arc::new(ZigPlugin::new()),
    ];
    plugins
        .into_iter()
        .map(|p| (p.id().to_string(), p))
        .collect()
});

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

pub fn new_backend_arg(tool_name: &str) -> BackendArg {
    BackendArg::new_raw(
        tool_name.to_string(),
        Some(format!("core:{}", tool_name)),
        tool_name.to_string(),
        None,
    )
}
