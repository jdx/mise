use std::path::PathBuf;

use indexmap::IndexMap;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RemoteTomlConfig {
    pub source: Option<PathBuf>,
    pub mise_env: Option<Vec<String>>,
    pub install_mise: Option<InstallMiseTomlConfig>,
    #[serde(default)]
    pub copy_links: Option<bool>,
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub hosts: IndexMap<String, RemoteHostTomlConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RemoteHostTomlConfig {
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub source: Option<PathBuf>,
    pub mise_env: Option<Vec<String>>,
    pub copy_links: Option<bool>,
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub ssh_options: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub install_mise: Option<InstallMiseTomlConfig>,
    pub mise_bin: Option<PathBuf>,
    pub remote_mise: Option<String>,
    pub bootstrap_command: Option<String>,
}

/// `install_mise = true` or `install_mise = "~/bin/mise"`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum InstallMiseTomlConfig {
    Enabled(bool),
    Path(String),
}

impl InstallMiseTomlConfig {
    pub fn path(self) -> Option<String> {
        match self {
            Self::Enabled(false) => None,
            Self::Enabled(true) => Some(DEFAULT_INSTALL_MISE_PATH.to_string()),
            Self::Path(path) => Some(path),
        }
    }
}

/// Where `install_mise = true` puts the executable. This matches the default
/// used by <https://mise.run> and is already searched by remote mise discovery.
pub const DEFAULT_INSTALL_MISE_PATH: &str = "~/.local/bin/mise";
