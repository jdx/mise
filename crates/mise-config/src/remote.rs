use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::refs;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct RemoteTomlConfig {
    /// local project directory sent to inventory hosts
    pub source: Option<PathBuf>,
    /// default ordered config environments loaded by remote bootstrap
    pub mise_env: Option<Vec<String>>,
    /// install the provisioned mise on inventory hosts instead of only staging it
    // `default` keeps the field optional: schemars cannot see the `Option` through `schema_with`.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_remote_install_mise")]
    pub install_mise: Option<InstallMiseTomlConfig>,
    /// dereference every symbolic link in the source archive
    #[serde(default)]
    pub copy_links: Option<bool>,
    /// source-relative symbolic links to dereference
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    /// additional source archive exclusion patterns
    #[serde(default)]
    pub exclude: Vec<String>,
    /// remote bootstrap inventory keyed by target name
    #[serde(default)]
    pub hosts: IndexMap<String, RemoteHostTomlConfig>,
}

/// SSH target that `mise bootstrap remote` can bootstrap, selected by name or tag
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RemoteHostTomlConfig {
    /// SSH host name or address
    pub host: String,
    /// SSH user
    pub user: Option<String>,
    /// SSH port
    #[schemars(range(min = 1))]
    pub port: Option<u16>,
    /// SSH identity file
    pub identity_file: Option<PathBuf>,
    /// host-specific local project directory to send
    pub source: Option<PathBuf>,
    /// ordered config environments loaded by remote bootstrap
    pub mise_env: Option<Vec<String>>,
    /// dereference every symbolic link in this host's source archive
    pub copy_links: Option<bool>,
    /// source-relative symbolic links to dereference for this host
    #[serde(default)]
    pub copy_link: Vec<PathBuf>,
    /// additional source archive exclusion patterns for this host
    #[serde(default)]
    pub exclude: Vec<String>,
    /// OpenSSH options passed with `-o`
    #[serde(default)]
    pub ssh_options: Vec<String>,
    /// inventory selection tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// install the provisioned mise on this host instead of only staging it
    // `default` keeps the field optional: schemars cannot see the `Option` through `schema_with`.
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_remote_install_mise")]
    pub install_mise: Option<InstallMiseTomlConfig>,
    /// local mise executable to upload
    pub mise_bin: Option<PathBuf>,
    /// existing mise executable name or path on the host
    pub remote_mise: Option<String>,
    /// remote shell command that installs mise
    pub bootstrap_command: Option<String>,
}

// `install_mise = true` or `install_mise = "~/bin/mise"`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum InstallMiseTomlConfig {
    /// install the provisioned mise to ~/.local/bin/mise, or do not install it
    Enabled(bool),
    /// absolute or ~/-relative remote path to install the provisioned mise to
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
