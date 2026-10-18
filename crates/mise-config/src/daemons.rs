//! `[daemons]`, `[daemons_settings]` and `[daemon_groups]`.

use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// A `[daemons]` entry. A definition stays a raw table that mise-daemons checks when it builds the
// daemon, so its schema is still written by hand in schema/mise.json.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Declaration {
    Preset(String),
    Definition(toml::Table),
}

/// Experimental project-wide daemon options.
// Every field is optional so a higher-precedence file can set one key without discarding the
// others; see [`DaemonSettings::merge`].
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonSettings {
    /// Fixed pitchfork namespace for this project, replacing the hashed default so other projects can refer to its daemons by qualified ID.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Append a worktree-specific suffix to an explicit namespace when the project root is a linked git worktree.
    #[serde(default)]
    #[schemars(extend("default" = true))]
    pub namespace_per_worktree: Option<bool>,
}

impl DaemonSettings {
    /// Overlay a higher-precedence file's table onto this one, key by key. A
    /// `mise.local.toml` that sets only `namespace_per_worktree` must not drop
    /// the `namespace` that `mise.toml` established, because other projects
    /// refer to daemons by the qualified ID that namespace produces.
    pub fn merge(&mut self, other: Self) {
        if other.namespace.is_some() {
            self.namespace = other.namespace;
        }
        if other.namespace_per_worktree.is_some() {
            self.namespace_per_worktree = other.namespace_per_worktree;
        }
    }

    pub fn namespace_per_worktree(&self) -> bool {
        self.namespace_per_worktree.unwrap_or(true)
    }
}

// The table form of a `[daemon_groups]` entry.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupTable {
    /// Daemon or group names in this project.
    #[schemars(length(min = 1))]
    daemons: Vec<String>,
}

// A `[daemon_groups]` entry: a bare list of members or a table with `daemons`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum GroupDeclaration {
    /// Daemon or group names in this project.
    List(#[schemars(length(min = 1))] Vec<String>),
    Table(GroupTable),
}

impl GroupDeclaration {
    pub fn members(&self) -> &[String] {
        match self {
            GroupDeclaration::List(members) => members,
            GroupDeclaration::Table(table) => &table.daemons,
        }
    }
}
