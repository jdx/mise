//! What the package managers need from the rest of mise, registered once at startup.
//!
//! Provisioning a mise-managed tool and loading package plugins both need the config and toolset
//! machinery, which sits above this module. [`register`] hands those two capabilities in.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use eyre::{Result, eyre};
use futures_util::future::LocalBoxFuture;

use super::SystemPackageManager;

pub struct Host {
    /// Install mise's ruby if it is missing and return the path to its `ruby` executable.
    pub provision_ruby: fn() -> LocalBoxFuture<'static, Result<PathBuf>>,
    /// The path to mise's ruby executable if it is already installed.
    pub installed_ruby: fn() -> LocalBoxFuture<'static, Result<Option<PathBuf>>>,
    /// Find an executable in the config's toolset, falling back to `PATH`.
    pub tool_bin: fn(&'static str) -> LocalBoxFuture<'static, Result<Option<PathBuf>>>,
    /// Names of the installed package plugins.
    pub package_plugin_names: fn() -> Vec<String>,
    /// Load the manager a package plugin provides.
    pub load_package_plugin: fn(&str) -> Result<Arc<dyn SystemPackageManager>>,
}

static HOST: OnceLock<Host> = OnceLock::new();

pub fn register(host: Host) {
    let _ = HOST.set(host);
}

fn host() -> Result<&'static Host> {
    HOST.get()
        .ok_or_else(|| eyre!("system package host was not registered"))
}

#[cfg(unix)]
pub(crate) async fn provision_ruby() -> Result<PathBuf> {
    (host()?.provision_ruby)().await
}

#[cfg(unix)]
pub(crate) async fn installed_ruby() -> Result<Option<PathBuf>> {
    (host()?.installed_ruby)().await
}

pub(crate) async fn tool_bin(name: &'static str) -> Result<Option<PathBuf>> {
    (host()?.tool_bin)(name).await
}

pub(crate) fn package_plugin_names() -> Vec<String> {
    match HOST.get() {
        Some(host) => (host.package_plugin_names)(),
        None => {
            warn!("system package host was not registered; package plugins are not loaded");
            Vec::new()
        }
    }
}

pub(crate) fn load_package_plugin(name: &str) -> Result<Arc<dyn SystemPackageManager>> {
    (host()?.load_package_plugin)(name)
}
