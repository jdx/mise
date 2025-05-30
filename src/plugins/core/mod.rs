use eyre::Result;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::LazyLock as Lazy;

use crate::backend::{Backend, BackendMap};
use crate::cli::args::BackendArg;
use crate::config::Settings;
use crate::env;
use crate::env::PATH_KEY;
use crate::timeout::run_with_timeout;
use crate::toolset::ToolVersion;

mod bun;
mod deno;
mod elixir;
mod erlang;
mod go;
mod java;
mod node;
pub(crate) mod python;
#[cfg_attr(windows, path = "ruby_windows.rs")]
mod ruby;
mod rust;
mod swift;
mod zig;

pub static CORE_PLUGINS: Lazy<BackendMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(bun::BunPlugin::new()),
        Arc::new(deno::DenoPlugin::new()),
        Arc::new(elixir::ElixirPlugin::new()),
        Arc::new(erlang::ErlangPlugin::new()),
        Arc::new(go::GoPlugin::new()),
        Arc::new(java::JavaPlugin::new()),
        Arc::new(node::NodePlugin::new()),
        Arc::new(python::PythonPlugin::new()),
        Arc::new(ruby::RubyPlugin::new()),
        Arc::new(rust::RustPlugin::new()),
        Arc::new(swift::SwiftPlugin::new()),
        Arc::new(zig::ZigPlugin::new()),
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
    run_with_timeout(f, Settings::get().fetch_remote_versions_timeout())
}

pub fn new_backend_arg(tool_name: &str) -> BackendArg {
    BackendArg::new_raw(
        tool_name.to_string(),
        Some(format!("core:{tool_name}")),
        tool_name.to_string(),
        None,
    )
}
