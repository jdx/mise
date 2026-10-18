use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// dotfiles history: what is never captured, reload commands, and the setup repository (https://mise.jdx.dev/history.html)
// `[history]` as parsed from a single mise.toml, before the layers are merged.
#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
pub struct HistoryTomlConfig {
    /// encryption for dotfiles tracked with encrypt = true
    #[serde(default)]
    pub encryption: Option<FileEncryptionConfig>,
    /// globs never captured by dotfiles history; a `!glob` entry re-includes a path an earlier glob excluded
    // Each glob may start with `~`.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// email used for history commit authors and committers; {hostname} expands to this machine's hostname
    #[serde(default)]
    pub git_email: Option<String>,
    /// this machine's name in per-machine streams (variants = [{ machine = true }]) instead of the name generated with the history store
    #[serde(default)]
    #[schemars(regex(pattern = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$"))]
    pub machine: Option<String>,
    /// commands run once after a rollback, undo, pull, or dotfiles apply writes a path matching the glob (trusted global or system config only)
    #[serde(default)]
    #[schemars(with = "IndexMap<String, HistoryReloadCommand>")]
    pub reload: IndexMap<String, String>,
    /// the setup repository this machine publishes to and fetches from; machine-local, written by `mise bootstrap dotfiles origin set`
    #[serde(default)]
    pub origin: Option<OriginTomlConfig>,
}

/// command to run once when a rollback, undo, pull, or dotfiles apply writes a path matching this glob
#[derive(JsonSchema)]
pub struct HistoryReloadCommand(pub String);

// Public recipients shared by every encrypted dotfile.
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileEncryptionConfig {
    /// shared age, SSH, or age-plugin public recipients for all encrypted dotfiles
    #[serde(default)]
    pub recipients: Vec<String>,
}

// `[history.origin]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OriginTomlConfig {
    /// the repository url
    pub url: String,
    /// the setup branch
    #[serde(default = "default_branch")]
    #[schemars(extend("default" = "main"))]
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
