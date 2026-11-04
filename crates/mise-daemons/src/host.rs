//! What the daemon modules need from mise's config and toolsets, registered once at startup.

use std::collections::HashSet;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use eyre::{Result, eyre};
use futures_util::future::LocalBoxFuture;
use indexmap::IndexMap;

use super::runtime::Runtime;
use super::{Daemon, DaemonSettings, Declaration, GroupDeclaration};

/// One loaded config file, as far as the daemon modules can see it.
pub trait ConfigFileView: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn project_root(&self) -> Option<PathBuf>;
    fn config_root(&self) -> PathBuf;
    fn daemon_declarations(&self) -> IndexMap<String, Declaration>;
    fn daemon_settings(&self) -> Option<DaemonSettings>;
    fn daemon_group_declarations(&self) -> IndexMap<String, GroupDeclaration>;
    fn daemon_providers(&self) -> IndexMap<String, toml::Table>;
}

/// The config files of one configuration root, highest precedence first.
pub type ConfigMap = IndexMap<PathBuf, Arc<dyn ConfigFileView>>;

/// What a provider operation hands back: asynchronous, and borrowing the provider it acts on.
pub type ProviderFuture<'a, T> = LocalBoxFuture<'a, Result<T>>;

pub struct Host {
    /// Whether the file is part of the global configuration.
    pub is_global_config: fn(&Path) -> bool,
    /// Whether ordinary config loading skips the directory / file.
    pub config_dir_is_ignored: fn(&Path, bool) -> bool,
    pub config_path_is_ignored: fn(&Path, bool) -> bool,
    /// The config files in a directory, highest precedence first.
    pub config_paths_in_dir: fn(&Path) -> Vec<PathBuf>,
    /// Whether the user already trusts the file, and the root `mise trust` must be given.
    pub is_path_trusted: fn(&Path) -> bool,
    pub config_trust_root: fn(&Path) -> PathBuf,
    /// The directory a config file's relative paths resolve against.
    pub config_root: fn(&Path) -> PathBuf,
    pub global_config_path: fn() -> PathBuf,
    /// Parse a config file from disk.
    pub parse_config_file: fn(&Path) -> Result<Arc<dyn ConfigFileView>>,
    /// Directories config discovery never walks above.
    pub ceiling_paths: fn() -> &'static HashSet<PathBuf>,
    pub safe_mode: fn() -> bool,
    /// Error in safe mode; `operation` names what was refused.
    pub ensure_not_safe: fn(&str) -> Result<()>,
    /// The user's global configuration as a supervisor runtime, reusing `previous_bin` when set.
    pub global_runtime: fn(Option<PathBuf>) -> LocalBoxFuture<'static, Result<Runtime>>,
    /// Install the tool a provider runs.
    pub install_provider_tools: for<'a> fn(&'a str, &'a Path, &'a Daemon) -> ProviderFuture<'a, ()>,
    /// The bin paths of a provider's tool, after checking the installed version matches.
    pub provider_tool_paths:
        for<'a> fn(&'a str, &'a Path, &'a Daemon) -> ProviderFuture<'a, Vec<PathBuf>>,
}

static HOST: OnceLock<Host> = OnceLock::new();

pub fn register(host: Host) {
    let _ = HOST.set(host);
}

fn host() -> Result<&'static Host> {
    HOST.get()
        .ok_or_else(|| eyre!("daemons host was not registered; mise registers it at startup"))
}

pub async fn global_runtime(previous_bin: Option<PathBuf>) -> Result<Runtime> {
    (host()?.global_runtime)(previous_bin).await
}

pub async fn install_provider_tools(name: &str, source: &Path, daemon: &Daemon) -> Result<()> {
    (host()?.install_provider_tools)(name, source, daemon).await
}

pub async fn provider_tool_paths(
    name: &str,
    source: &Path,
    daemon: &Daemon,
) -> Result<Vec<PathBuf>> {
    (host()?.provider_tool_paths)(name, source, daemon).await
}

pub fn is_global_config(path: &Path) -> bool {
    (host_or_panic().is_global_config)(path)
}

pub fn config_dir_is_ignored(dir: &Path, include_ignored: bool) -> bool {
    (host_or_panic().config_dir_is_ignored)(dir, include_ignored)
}

pub fn config_path_is_ignored(path: &Path, include_ignored: bool) -> bool {
    (host_or_panic().config_path_is_ignored)(path, include_ignored)
}

pub fn config_paths_in_dir(dir: &Path) -> Vec<PathBuf> {
    (host_or_panic().config_paths_in_dir)(dir)
}

pub fn is_path_trusted(path: &Path) -> bool {
    (host_or_panic().is_path_trusted)(path)
}

pub fn config_trust_root(path: &Path) -> PathBuf {
    (host_or_panic().config_trust_root)(path)
}

pub fn config_root(path: &Path) -> PathBuf {
    (host_or_panic().config_root)(path)
}

pub fn global_config_path() -> PathBuf {
    (host_or_panic().global_config_path)()
}

pub fn parse_config_file(path: &Path) -> Result<Arc<dyn ConfigFileView>> {
    (host_or_panic().parse_config_file)(path)
}

pub fn ceiling_paths() -> &'static HashSet<PathBuf> {
    (host_or_panic().ceiling_paths)()
}

pub fn safe_mode() -> bool {
    (host_or_panic().safe_mode)()
}

pub fn ensure_not_safe(operation: &str) -> Result<()> {
    (host_or_panic().ensure_not_safe)(operation)
}

fn host_or_panic() -> &'static Host {
    HOST.get()
        .expect("daemons host was not registered; mise registers it at startup")
}
