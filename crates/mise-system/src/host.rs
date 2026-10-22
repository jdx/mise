//! What the system modules need from mise's config and runtime layers, registered once at startup.
//!
//! The system modules interpret the `[bootstrap]`, `[dotfiles]`, and `[history]` declarations, but
//! the config layer loads them, so the two cannot name each other. This module is the seam:
//! [`BootstrapConfig`] and [`ConfigFileView`] describe the slice of `Config` and `ConfigFile` the
//! system modules read, and [`Host`] carries the few free functions and process-wide lookups they
//! call back into.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use eyre::{Result, eyre};
use futures_util::future::LocalBoxFuture;
use indexmap::{IndexMap, IndexSet};
use mise_config::bootstrap::BootstrapTomlConfig;
use mise_config::dotfiles::{DotfileGroupsTomlConfig, DotfilesTomlConfig};
use mise_config::history::HistoryTomlConfig;
use mise_util::env_diff::EnvMap;

/// One loaded config file, as far as the system modules can see it.
pub trait ConfigFileView: Debug + Send + Sync {
    fn get_path(&self) -> &Path;
    fn project_root(&self) -> Option<PathBuf>;
    fn config_root(&self) -> PathBuf;
    fn bootstrap_config(&self) -> Option<BootstrapTomlConfig>;
    fn dotfiles_config(&self) -> Option<DotfilesTomlConfig>;
    fn dotfile_groups_config(&self) -> Option<DotfileGroupsTomlConfig>;
    fn history_config(&self) -> Option<HistoryTomlConfig>;
}

/// The config files of one configuration root, highest precedence first.
pub type ConfigMap = IndexMap<PathBuf, Arc<dyn ConfigFileView>>;

/// The loaded configuration, as far as the system modules can see it.
#[async_trait(?Send)]
pub trait BootstrapConfig {
    /// The config files of the main configuration root.
    fn config_files(&self) -> ConfigMap;
    /// The `[vars]` of the main configuration.
    fn vars(&self) -> IndexMap<String, String>;
    /// Each independent bootstrap configuration root's files.
    fn bootstrap_config_maps(&self) -> Vec<ConfigMap>;
    /// The bootstrap roots that were selected explicitly, with their files.
    fn selected_bootstrap_config_maps(&self) -> Vec<(PathBuf, ConfigMap)>;
    /// Whether the config file at `config_path` belongs to an explicitly selected bootstrap root.
    fn is_in_selected_bootstrap_root(&self, config_path: &Path) -> bool;
    /// The template context for the config file at `config_path`.
    fn bootstrap_tera_ctx(&self, config_path: &Path) -> &tera::Context;
    /// Variables a dry run could not resolve for the file at `config_path`.
    fn bootstrap_dry_run_unresolved_vars(&self, config_path: &Path) -> &IndexSet<String>;
    fn add_redactions_excluding(
        &self,
        redactions: Vec<String>,
        env: &EnvMap,
        exclusions: &BTreeSet<String>,
    );
    fn redact(&self, input: &str) -> String;
    /// The config files tracked by `mise dot`, loaded on demand.
    async fn get_tracked_config_files(&self) -> Result<ConfigMap>;
    /// This configuration without the files declared only for `environments`.
    fn without_environments(&self, environments: &[String]) -> Arc<dyn BootstrapConfig>;
}

/// The system modules say `Config` for the trait object, as they did for mise's own type.
pub type Config = dyn BootstrapConfig;

#[async_trait(?Send)]
impl<T: BootstrapConfig + ?Sized> BootstrapConfig for Arc<T> {
    fn config_files(&self) -> ConfigMap {
        (**self).config_files()
    }
    fn vars(&self) -> IndexMap<String, String> {
        (**self).vars()
    }
    fn bootstrap_config_maps(&self) -> Vec<ConfigMap> {
        (**self).bootstrap_config_maps()
    }
    fn selected_bootstrap_config_maps(&self) -> Vec<(PathBuf, ConfigMap)> {
        (**self).selected_bootstrap_config_maps()
    }
    fn is_in_selected_bootstrap_root(&self, config_path: &Path) -> bool {
        (**self).is_in_selected_bootstrap_root(config_path)
    }
    fn bootstrap_tera_ctx(&self, config_path: &Path) -> &tera::Context {
        (**self).bootstrap_tera_ctx(config_path)
    }
    fn bootstrap_dry_run_unresolved_vars(&self, config_path: &Path) -> &IndexSet<String> {
        (**self).bootstrap_dry_run_unresolved_vars(config_path)
    }
    fn add_redactions_excluding(
        &self,
        redactions: Vec<String>,
        env: &EnvMap,
        exclusions: &BTreeSet<String>,
    ) {
        (**self).add_redactions_excluding(redactions, env, exclusions)
    }
    fn redact(&self, input: &str) -> String {
        (**self).redact(input)
    }
    async fn get_tracked_config_files(&self) -> Result<ConfigMap> {
        (**self).get_tracked_config_files().await
    }
    fn without_environments(&self, environments: &[String]) -> Arc<dyn BootstrapConfig> {
        (**self).without_environments(environments)
    }
}

pub struct Host {
    /// The loaded configuration, loading it first if needed.
    pub config: fn() -> LocalBoxFuture<'static, Result<Arc<dyn BootstrapConfig>>>,
    /// Discard the loaded configuration and load it again.
    pub reload_config: fn() -> LocalBoxFuture<'static, Result<Arc<dyn BootstrapConfig>>>,
    /// The loaded configuration, if one has been loaded.
    pub loaded_config: fn() -> Option<Arc<dyn BootstrapConfig>>,
    /// The environments a config file is declared for, from its file name.
    pub environments_for_config_path: fn(&Path) -> Vec<String>,
    /// The directory a config file's relative paths resolve against.
    pub config_root: fn(&Path) -> PathBuf,
    /// Whether the file is part of the global configuration.
    pub is_global_config: fn(&Path) -> bool,
    /// Whether the user has trusted the config file.
    pub is_trusted: fn(&Path) -> bool,
    pub system_config_files: fn() -> IndexSet<PathBuf>,
    pub global_config_files: fn() -> IndexSet<PathBuf>,
    pub global_shared_config_path: fn() -> PathBuf,
    /// The config files in `dir`, plus the `incoming` ones about to land there.
    pub config_files_with_incoming: fn(&Path, &BTreeSet<PathBuf>) -> IndexSet<PathBuf>,
    /// Whether safe mode ignores the bootstrap declarations of the file.
    pub safe_mode_ignores_bootstrap: fn(&Path) -> bool,
    /// `settings.history.describe_command` as written in one settings file.
    pub describe_command_setting: fn(&Path) -> Result<Option<String>>,
    /// Auto platform environments followed by the explicit `MISE_ENV` entries.
    pub mise_env_with_auto: fn() -> &'static [String],
    /// Verify a minisign signature made with mise's release key.
    pub verify_release_signature: fn(&[u8], &str) -> Result<()>,
    /// Parse a config file from disk.
    pub parse_config_file: fn(&Path) -> Result<Arc<dyn ConfigFileView>>,
    /// Decode only the static declarations of a config body, without trusting or evaluating it.
    pub parse_config_body: fn(&str, &Path) -> Result<Arc<dyn ConfigFileView>>,
}

static HOST: OnceLock<Host> = OnceLock::new();

pub fn register(host: Host) {
    let _ = HOST.set(host);
}

fn host() -> &'static Host {
    HOST.get()
        .expect("system host was not registered; mise registers it at startup")
}

pub async fn config() -> Result<Arc<dyn BootstrapConfig>> {
    (host().config)().await
}

pub async fn reload_config() -> Result<Arc<dyn BootstrapConfig>> {
    (host().reload_config)().await
}

pub fn loaded_config() -> Option<Arc<dyn BootstrapConfig>> {
    (host().loaded_config)()
}

pub fn environments_for_config_path(path: &Path) -> Vec<String> {
    (host().environments_for_config_path)(path)
}

pub fn config_root(path: &Path) -> PathBuf {
    (host().config_root)(path)
}

pub fn is_global_config(path: &Path) -> bool {
    (host().is_global_config)(path)
}

pub fn is_trusted(path: &Path) -> bool {
    (host().is_trusted)(path)
}

pub fn system_config_files() -> IndexSet<PathBuf> {
    (host().system_config_files)()
}

pub fn global_config_files() -> IndexSet<PathBuf> {
    (host().global_config_files)()
}

pub fn global_shared_config_path() -> PathBuf {
    (host().global_shared_config_path)()
}

pub fn config_files_with_incoming(dir: &Path, incoming: &BTreeSet<PathBuf>) -> IndexSet<PathBuf> {
    (host().config_files_with_incoming)(dir, incoming)
}

pub fn safe_mode_ignores_bootstrap(path: &Path) -> bool {
    (host().safe_mode_ignores_bootstrap)(path)
}

pub fn describe_command_setting(path: &Path) -> Result<Option<String>> {
    (host().describe_command_setting)(path)
}

pub fn mise_env_with_auto() -> &'static [String] {
    (host().mise_env_with_auto)()
}

pub fn verify_release_signature(data: &[u8], signature: &str) -> Result<()> {
    (host().verify_release_signature)(data, signature)
}

pub fn parse_config_file(path: &Path) -> Result<Arc<dyn ConfigFileView>> {
    (host().parse_config_file)(path)
}

pub fn parse_config_body(body: &str, path: &Path) -> Result<Arc<dyn ConfigFileView>> {
    (host().parse_config_body)(body, path)
}

/// `config.toml`, or `config.local.toml` next to it for machine-only declarations.
pub fn declaration_file(local: bool) -> Result<PathBuf> {
    let global = global_shared_config_path();
    if !local {
        return Ok(global);
    }
    let dir = global.parent().unwrap_or(Path::new("."));
    Ok(dir.join("config.local.toml"))
}

/// The global configuration file as an editable document; a missing file reads as empty.
pub fn read_document(path: &Path) -> Result<toml_edit::DocumentMut> {
    if path.exists() {
        let text = mise_util::file::read_to_string(path)?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|err| eyre!("parsing {}: {err}", mise_util::file::display_path(path)))
    } else {
        Ok(toml_edit::DocumentMut::new())
    }
}
