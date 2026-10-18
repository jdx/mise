use indexmap::IndexMap;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// dotfiles history: what is never captured, reload commands, and the setup repository (https://mise.jdx.dev/history.html)
// `[history]` as parsed from a single mise.toml, before the layers are merged.
#[derive(Debug, Default, Clone, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HistoryTomlConfig {
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
    #[cfg_attr(
        feature = "schema",
        schemars(regex(pattern = r"^[A-Za-z0-9][A-Za-z0-9._-]{0,62}$"))
    )]
    pub machine: Option<String>,
    /// commands run once after a rollback or undo writes a path matching the glob (trusted global or system config only)
    #[serde(default)]
    pub reload: IndexMap<String, String>,
    /// the setup repository this machine publishes to and fetches from; machine-local, written by `mise bootstrap dotfiles origin set`
    #[serde(default)]
    pub origin: Option<OriginTomlConfig>,
}

// Public recipients shared by every encrypted dotfile.
#[derive(Debug, Clone, Deserialize, Default)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct FileEncryptionConfig {
    /// shared age, SSH, or age-plugin public recipients for all encrypted dotfiles
    #[serde(default)]
    pub recipients: Vec<String>,
}

// `[history.origin]`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct OriginTomlConfig {
    /// the repository url
    pub url: String,
    /// the setup branch
    #[serde(default = "default_branch")]
    #[cfg_attr(feature = "schema", schemars(extend("default" = "main")))]
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
