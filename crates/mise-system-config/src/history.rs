use indexmap::IndexMap;
use serde::Deserialize;

/// `[history]` as parsed from a single mise.toml.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct HistoryTomlConfig {
    /// Globs (with `~`) never captured; `!glob` re-includes.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Commands to run after a rollback touches a matching path.
    #[serde(default)]
    pub reload: IndexMap<String, String>,
    /// The setup repository this machine publishes to and fetches from.
    #[serde(default)]
    pub origin: Option<OriginTomlConfig>,
    #[serde(default)]
    pub encryption: Option<FileEncryptionConfig>,
    /// Email on history commits, with an optional `{hostname}` placeholder.
    #[serde(default)]
    pub git_email: Option<String>,
    /// This machine's name in per-machine streams, instead of the one
    /// generated with the store.
    #[serde(default)]
    pub machine: Option<String>,
}

/// Public recipients shared by every encrypted dotfile.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FileEncryptionConfig {
    #[serde(default)]
    pub recipients: Vec<String>,
}

/// `[history.origin]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OriginTomlConfig {
    pub url: String,
    #[serde(default = "default_branch")]
    pub branch: String,
}

impl OriginTomlConfig {
    /// A plaintext connection, as recorded before any declaration.
    pub fn plain(url: String, branch: String) -> Self {
        Self { url, branch }
    }
}

fn default_branch() -> String {
    "main".to_string()
}
