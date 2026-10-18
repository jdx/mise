//! `[daemons]`, `[daemons_settings]` and `[daemon_groups]`.

use schemars::JsonSchema;
use serde::Deserialize;

use schemars::{Schema, SchemaGenerator, json_schema};

use crate::dotfiles::schema::any_table;
use crate::schema::refs;

fn always_true(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"const": true})
}

fn preset_names(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "enum": ["cockroachdb", "nats", "postgres", "redis", "spicedb"]})
}

fn provider_preset_names(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"enum": ["postgres", "cockroachdb", "nats"]})
}

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

// A `[daemons]` entry. A definition stays a raw table that mise-daemons checks when it builds the
// daemon, so its schema comes from the `Daemon*Decl` types below.
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

// The doc comments on the declaration types below are their descriptions in schema/mise.json.
// A definition stays a raw table that mise-daemons reads key by key, and keys it does not know
// are forwarded to pitchfork, so these types describe what a table may hold; nothing parses
// into them.

/// A fixed port, or "auto" to derive one from the project root so the primary checkout keeps the base port and each linked git worktree gets its own.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonPort {
    Fixed(#[schemars(range(min = 1, max = 65535))] u16),
    /// Offset the preset's default port by this project root's worktree slot.
    Auto(#[schemars(extend("const" = "auto"))] String),
    PerWorktree(DaemonPortAuto),
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonPortAuto {
    /// Enable per-worktree port allocation.
    #[schemars(schema_with = "always_true")]
    pub auto: bool,
    /// Port used by the primary checkout. Defaults to the preset's port; required for custom daemons.
    #[schemars(range(min = 1, max = 65535))]
    pub base: Option<u16>,
    /// Distance between consecutive worktree slots. Defaults to 1.
    #[schemars(range(min = 1, max = 65535))]
    pub stride: Option<u16>,
}

/// Hostname label the reverse proxy routes to this daemon, false to opt out, or true to use the daemon's name. Only a daemon with a port is routed. The hostname is <proxy>.<worktree>.<project>.<tld> in a linked git worktree and <proxy>.<project>.<tld> in the primary checkout.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonProxy {
    Label(
        #[schemars(length(max = 63), regex(pattern = r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?$"))] String,
    ),
    /// false opts out of the reverse proxy; true uses the daemon's name, which re-enables routing for a preset that opts out.
    Enabled(bool),
}

/// Whether the proxy terminates TLS for this daemon or passes the connection through to it.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DaemonProxyTls {
    Terminate,
    Passthrough,
}

/// Stop this daemon after this long without proxy traffic when the proxy started it, for example "30m". false exempts it from idle shutdown even when pitchfork's proxy.idle_timeout is set; "0" does the same. Idle shutdown is off unless this or pitchfork's proxy.idle_timeout is set, and a daemon started with `mise daemons start` is never stopped for inactivity. Requires pitchfork 2.27.0.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonProxyIdleTimeout {
    After(#[schemars(regex(pattern = r"^\s*[0-9]"))] String),
    Never(#[schemars(extend("enum" = [false]))] bool),
}

/// idempotent setup commands run before the daemon's long-running process
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonInit {
    One(String),
    Many(Vec<String>),
}

/// Pitchfork's structured port configuration, forwarded verbatim.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(extend("not" = {"required": ["auto"]}))]
pub struct DaemonPitchforkPort(#[schemars(schema_with = "any_table")] pub toml::Table);

// The port of a custom daemon: mise's own, or pitchfork's structured form.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonCommandPort {
    Mise(#[schemars(schema_with = "refs::daemon_port")] DaemonPort),
    Pitchfork(DaemonPitchforkPort),
}

// One `[daemons]` entry. Which shape a table is follows from its keys, so each names the keys
// that belong to another shape in `not`.
/// A daemon: a version request for the preset of the same name, or a table describing a custom command, task, preset, another project's daemon, or a shared provider.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum DaemonDecl {
    /// Version request for a preset matching the daemon name.
    Version(String),
    Command(Box<DaemonCommandDecl>),
    Task(Box<DaemonTaskDecl>),
    Preset(Box<DaemonPresetDecl>),
    Project(DaemonProjectDecl),
    Provider(DaemonProviderRefDecl),
}

// Extra keys are forwarded to pitchfork, so the table is open.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(extend(
    "additionalProperties" = true,
    "not" = {"anyOf": [
        {"required": ["preset"]},
        {"required": ["project"]},
        {"required": ["task"]},
        {"required": ["args"]},
        {"required": ["provider"]},
        {"required": ["resource"]}
    ]}
))]
pub struct DaemonCommandDecl {
    /// Shell command for the daemon's long-running process.
    pub run: String,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_init")]
    pub init: Option<DaemonInit>,
    /// false runs the process and its setup without the project's tool environment.
    pub mise: Option<bool>,
    pub port: Option<DaemonCommandPort>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy")]
    pub proxy: Option<DaemonProxy>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_tls")]
    pub proxy_tls: Option<DaemonProxyTls>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_idle_timeout")]
    pub proxy_idle_timeout: Option<DaemonProxyIdleTimeout>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(extend(
    "additionalProperties" = true,
    "not" = {"anyOf": [
        {"required": ["run"]},
        {"required": ["preset"]},
        {"required": ["project"]},
        {"required": ["provider"]},
        {"required": ["resource"]}
    ]}
))]
pub struct DaemonTaskDecl {
    /// mise task run as this daemon's long-running process
    pub task: String,
    /// arguments passed to the task after `--`
    pub args: Option<Vec<String>>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_init")]
    pub init: Option<DaemonInit>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy")]
    pub proxy: Option<DaemonProxy>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_tls")]
    pub proxy_tls: Option<DaemonProxyTls>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_idle_timeout")]
    pub proxy_idle_timeout: Option<DaemonProxyIdleTimeout>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[schemars(extend(
    "additionalProperties" = true,
    "not" = {"anyOf": [
        {"required": ["project"]},
        {"required": ["task"]},
        {"required": ["args"]},
        {"required": ["provider"]},
        {"required": ["resource"]}
    ]}
))]
pub struct DaemonPresetDecl {
    /// Persistent preset data directory, relative to the declaring project root or absolute. Defaults to mise state storage.
    #[schemars(length(min = 1))]
    pub data_dir: Option<String>,
    /// Service preset that supplies the daemon's command, port and data directory.
    #[schemars(schema_with = "preset_names")]
    pub preset: String,
    /// Version request for the preset's tool.
    pub version: String,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_port")]
    pub port: Option<DaemonPort>,
    /// Overrides for the preset's additional named ports.
    pub ports: Option<std::collections::BTreeMap<String, DaemonPortNumber>>,
    /// Preset options, such as `database`.
    #[serde(default)]
    #[schemars(schema_with = "any_table")]
    pub options: Option<toml::Table>,
    /// Shell command that replaces the preset's command.
    pub run: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_init")]
    pub init: Option<DaemonInit>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy")]
    pub proxy: Option<DaemonProxy>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_tls")]
    pub proxy_tls: Option<DaemonProxyTls>,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_proxy_idle_timeout")]
    pub proxy_idle_timeout: Option<DaemonProxyIdleTimeout>,
}

/// Port for this named port of the preset.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DaemonPortNumber(#[schemars(range(min = 1, max = 65535))] pub u16);

/// Daemon declared by another project, run under that project's namespace and root.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonProjectDecl {
    /// Directory of a project whose mise configuration declares the daemon. A relative path resolves against this project root.
    pub project: String,
    /// Daemon name inside the referenced project, when it differs from this key.
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonProviderRefDecl {
    /// Name of a globally defined shared server provider.
    pub provider: String,
    /// Database or NATS account shared by consumers choosing this name. Omit for a checkout-specific resource.
    #[schemars(regex(pattern = r"^[a-z][a-z0-9_]{0,62}$"))]
    pub resource: Option<String>,
}

// One `[daemon_providers]` entry: a shared server owned by the user.
/// A PostgreSQL, CockroachDB or NATS server that every checkout shares, with a separate database or NATS account for each.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DaemonProviderDecl {
    /// Service preset that runs the server.
    #[schemars(schema_with = "provider_preset_names")]
    pub preset: String,
    /// Version request for the preset's tool.
    pub version: String,
    #[serde(default)]
    #[schemars(schema_with = "refs::daemon_port")]
    pub port: Option<DaemonPort>,
    /// Overrides for the preset's additional named ports.
    pub ports: Option<std::collections::BTreeMap<String, DaemonPortNumber>>,
    /// Preset options, such as `database`.
    #[serde(default)]
    #[schemars(schema_with = "any_table")]
    pub options: Option<toml::Table>,
    /// Persistent server data directory; a relative path resolves inside the provider's state directory. Defaults to $MISE_STATE_DIR/daemon-providers/<name>/data.
    #[schemars(length(min = 1))]
    pub data_dir: Option<String>,
    /// Tool to install the server from, in place of the preset's default.
    pub tool: Option<String>,
}
