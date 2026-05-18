use color_eyre::eyre::Context;
use eyre::Result;
use std::ffi::OsString;
use std::future::Future;
use std::sync::Arc;
use std::sync::LazyLock as Lazy;

use crate::backend::{Backend, BackendMap};
use crate::cli::args::{BackendArg, BackendResolution};
use crate::config::Settings;
use crate::env;
use crate::path_env::PathEnv;
use crate::timeout::{TimeoutError, run_with_timeout, run_with_timeout_async};
use crate::toolset::ToolVersion;

mod bun;
mod deno;
mod dotnet;
mod elixir;
mod erlang;
mod go;
mod java;
mod node;
pub(crate) mod python;
#[cfg_attr(windows, path = "ruby_windows.rs")]
mod ruby;
mod ruby_common;
mod rust;
mod swift;
mod zig;

pub static CORE_PLUGINS: Lazy<BackendMap> = Lazy::new(|| {
    let plugins: Vec<Arc<dyn Backend>> = vec![
        Arc::new(bun::BunPlugin::new()),
        Arc::new(deno::DenoPlugin::new()),
        Arc::new(dotnet::DotnetPlugin::new()),
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
    let mut path_env = PathEnv::from_iter(env::PATH.clone());
    path_env.add(tv.install_path().join("bin"));
    Ok(path_env.join())
}

pub fn run_fetch_task_with_timeout<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send,
    T: Send,
{
    let timeout = Settings::get().fetch_remote_versions_timeout();
    match run_with_timeout(f, timeout) {
        Ok(v) => Ok(v),
        Err(err) => {
            // Only add a hint when the error was actually caused by a timeout
            if err.downcast_ref::<TimeoutError>().is_some() {
                Err(err).context(
                    "change with `fetch_remote_versions_timeout` or env `MISE_FETCH_REMOTE_VERSIONS_TIMEOUT`",
                )
            } else {
                Err(err)
            }
        }
    }
}

pub async fn run_fetch_task_with_timeout_async<F, Fut, T>(f: F) -> Result<T>
where
    Fut: Future<Output = Result<T>> + Send,
    T: Send,
    F: FnOnce() -> Fut,
{
    let timeout = Settings::get().fetch_remote_versions_timeout();
    match run_with_timeout_async(f, timeout).await {
        Ok(v) => Ok(v),
        Err(err) => {
            if err.downcast_ref::<TimeoutError>().is_some() {
                Err(err).context(
                    "change with `fetch_remote_versions_timeout` or env `MISE_FETCH_REMOTE_VERSIONS_TIMEOUT`",
                )
            } else {
                Err(err)
            }
        }
    }
}

pub fn new_backend_arg(tool_name: &str) -> BackendArg {
    BackendArg::new_raw(
        tool_name.to_string(),
        Some(format!("core:{tool_name}")),
        tool_name.to_string(),
        None,
        BackendResolution::new(true),
    )
}

pub fn backend_from_arg(canonical_short: &str, ba: BackendArg) -> Option<Arc<dyn Backend>> {
    match canonical_short {
        "bun" => Some(Arc::new(bun::BunPlugin::from_arg(ba))),
        "deno" => Some(Arc::new(deno::DenoPlugin::from_arg(ba))),
        "dotnet" => Some(Arc::new(dotnet::DotnetPlugin::from_arg(ba))),
        "elixir" => Some(Arc::new(elixir::ElixirPlugin::from_arg(ba))),
        "erlang" => Some(Arc::new(erlang::ErlangPlugin::from_arg(ba))),
        "go" => Some(Arc::new(go::GoPlugin::from_arg(ba))),
        "java" => Some(Arc::new(java::JavaPlugin::from_arg(ba))),
        "node" => Some(Arc::new(node::NodePlugin::from_arg(ba))),
        "python" => Some(Arc::new(python::PythonPlugin::from_arg(ba))),
        "ruby" => Some(Arc::new(ruby::RubyPlugin::from_arg(ba))),
        "rust" => Some(Arc::new(rust::RustPlugin::from_arg(ba))),
        "swift" => Some(Arc::new(swift::SwiftPlugin::from_arg(ba))),
        "zig" => Some(Arc::new(zig::ZigPlugin::from_arg(ba))),
        _ => None,
    }
}
