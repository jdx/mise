//! The top level of a `mise.toml`: every key mise reads from it.
//!
//! mise parses a file through [`MiseTomlDecl`], and `schema/mise.json` is generated from it. Two
//! fields need types from crates above this one, so they are type parameters: the entries of
//! `[tools]` (mise checks their options while parsing) and `[settings]`. Their defaults,
//! [`ToolSpec`] and a raw table, make the declaration usable on its own.

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Deserialize;

use crate::alias::Alias;
use crate::bootstrap::BootstrapTomlConfig;
use crate::daemons::{DaemonSettings, Declaration, GroupDeclaration};
use crate::de::deserialize_arr;
use crate::deps::DepsDecl;
use crate::doctor::DoctorConfig;
use crate::dotfiles::{DotfileGroupsTomlConfig, DotfilesTomlConfig};
use crate::env::{EnvList, deserialize_root_env, deserialize_vars};
use crate::history::HistoryTomlConfig;
use crate::hooks::{HookDef, Hooks};
use crate::min_version::{MinVersionSpec, deserialize_min_version};
use crate::monorepo::MonorepoConfig;
use crate::oci::OciConfig;
use crate::schema::{hand_written, refs};
use crate::task::{TaskConfig, TaskDecls, TaskTemplateDecls};
use crate::tool_config::ToolConfig;
use crate::tools::{ToolSpec, ToolsDecl};
use crate::watch_files::WatchFile;
use crate::wrappers::CommandWrapper;

// The doc comments on these fields are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.
//
// A field whose schema is a `$ref`, or written by hand, takes it from `schema_with`. Every field
// has a default, so none of them is required.

/// config file for mise version manager (mise.toml)
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(bound(deserialize = "T: Deserialize<'de>, S: Deserialize<'de> + Default"))]
#[schemars(bound = "S: Default")]
pub struct MiseTomlDecl<T = ToolSpec, S = toml::Table> {
    /// custom shorthands
    #[serde(default)]
    #[schemars(schema_with = "alias_map", extend("deprecated" = true))]
    pub alias: IndexMap<String, Alias>,
    /// Tool version aliases
    #[serde(default)]
    #[schemars(schema_with = "alias_map")]
    pub tool_alias: IndexMap<String, Alias>,
    /// shell aliases
    #[serde(default)]
    #[schemars(schema_with = "shell_alias")]
    pub shell_alias: IndexMap<String, String>,
    /// commands that intercept a binary name before delegating to the active toolset
    #[serde(default)]
    pub wrappers: IndexMap<String, CommandWrapper>,
    /// environment variables to set
    #[serde(default, deserialize_with = "deserialize_root_env")]
    #[schemars(schema_with = "env")]
    pub env: EnvList,
    /// config files merged into this one, ranking just below it: a local path (relative to this file), git::<url>//<path>.toml?ref=<ref> or oci::<registry>/<repo>[:tag|@sha256:<digest>] (the artifact holds a mise.toml). Paranoid mode requires a commit sha or digest for remote includes and rejects local ones; otherwise a branch or tag is refreshed after fetch_remote_versions_cache
    #[serde(default, deserialize_with = "deserialize_arr")]
    #[schemars(schema_with = "include")]
    pub include: Vec<String>,
    /// dotenv file(s) to load. Deprecated; use env._.file instead. This will be removed in mise 2027.4.0.
    #[serde(default, deserialize_with = "deserialize_arr")]
    #[schemars(schema_with = "dotenv_files", extend("deprecated" = true))]
    pub env_file: Vec<String>,
    /// dotenv file(s) to load. Deprecated; use env._.file instead. This will be removed in mise 2027.4.0.
    #[serde(default, deserialize_with = "deserialize_arr")]
    #[schemars(schema_with = "dotenv_files", extend("deprecated" = true))]
    pub dotenv: Vec<String>,
    /// PATH entries to add. Deprecated; use env._.path instead. This will be removed in mise 2027.4.0.
    #[serde(default, deserialize_with = "deserialize_arr")]
    #[schemars(schema_with = "path_entries", extend("deprecated" = true))]
    pub env_path: Vec<String>,
    /// marks this config as a monorepo root for task path syntax
    #[serde(default)]
    #[schemars(schema_with = "refs::monorepo_root")]
    pub monorepo_root: Option<bool>,
    /// deprecated alias of monorepo_root
    #[serde(default)]
    #[schemars(extend("deprecated" = true))]
    pub experimental_monorepo_root: Option<bool>,
    /// configuration for monorepo task discovery
    #[serde(default)]
    #[schemars(schema_with = "refs::monorepo")]
    pub monorepo: Option<MonorepoConfig>,
    /// minimum version of mise required to use this config
    #[serde(default, deserialize_with = "deserialize_min_version")]
    pub min_version: Option<MinVersionSpec>,
    /// [experimental] Sources for mise secrets: values resolved only when mise starts a task or `mise x` command that was granted them. Project config only.
    // Raw on purpose: a malformed `[secrets]` must not fail the whole file.
    #[serde(default)]
    #[schemars(with = "Option<crate::secrets::SecretsDecl>")]
    pub secrets: Option<toml::Value>,
    /// env or vars keys to redact from logs
    #[serde(default)]
    pub redactions: Vec<String>,
    /// plugins to use
    #[serde(default)]
    #[schemars(schema_with = "plugins")]
    pub plugins: IndexMap<String, String>,
    /// [experimental] configure deps providers
    #[serde(default)]
    pub deps: Option<DepsDecl>,
    #[serde(default)]
    pub oci: Option<OciConfig>,
    /// mise settings
    #[serde(default)]
    #[schemars(schema_with = "settings")]
    pub settings: S,
    /// dotfiles applied with `mise dotfiles apply` or `mise bootstrap`, keyed by target path or a logical name when every variant overrides the target; an omitted source for the latter resolves the relative key under dotfiles.root
    #[serde(default)]
    #[schemars(schema_with = "dotfiles")]
    pub dotfiles: Option<DotfilesTomlConfig>,
    /// named directory trees of dotfiles, applied with `mise dotfiles apply` or `mise bootstrap` while [bootstrap] dotfile_groups selects them (or is unset)
    #[serde(default)]
    #[schemars(schema_with = "dotfile_groups")]
    pub dotfile_groups: Option<DotfileGroupsTomlConfig>,
    #[serde(default)]
    pub bootstrap: Option<BootstrapTomlConfig>,
    #[serde(default)]
    pub history: Option<HistoryTomlConfig>,
    /// configuration for task execution and management
    #[serde(default)]
    #[schemars(schema_with = "refs::task_config")]
    pub task_config: TaskConfig,
    /// policy for tools declared by this config root
    #[serde(default)]
    #[schemars(schema_with = "refs::tool_config")]
    pub tool_config: ToolConfig,
    /// task runner tasks
    #[serde(default)]
    #[schemars(schema_with = "task_map")]
    pub tasks: TaskDecls,
    /// task templates that can be extended by tasks via extends
    #[serde(default)]
    #[schemars(schema_with = "task_template_map")]
    pub task_templates: TaskTemplateDecls,
    /// dev tools to use
    #[serde(default)]
    #[schemars(schema_with = "tools")]
    pub tools: ToolsDecl<T>,
    /// hooks to run on events like cd, enter, leave
    #[serde(default)]
    #[schemars(schema_with = "refs::hooks")]
    pub hooks: IndexMap<Hooks, HookDef>,
    /// variables to set for use in config templates
    #[serde(default, deserialize_with = "deserialize_vars")]
    #[schemars(schema_with = "refs::vars")]
    pub vars: EnvList,
    /// files to watch for changes and run scripts when modified
    #[serde(default)]
    #[schemars(schema_with = "refs::watch_files")]
    pub watch_files: Vec<WatchFile>,
    // `[_]` holds the user's own data, which mise never reads.
    #[serde(default, rename = "_")]
    #[schemars(schema_with = "custom")]
    pub custom: Option<toml::Value>,
    #[serde(default)]
    #[schemars(schema_with = "refs::doctor")]
    pub doctor: DoctorConfig,
    /// Experimental project daemons: custom pitchfork commands or managed presets.
    #[serde(default)]
    #[schemars(schema_with = "daemons")]
    pub daemons: IndexMap<String, Declaration>,
    /// Experimental shared servers owned by the user. Only allowed in global configuration.
    #[serde(default)]
    #[schemars(schema_with = "daemon_providers")]
    pub daemon_providers: IndexMap<String, toml::Table>,
    #[serde(default)]
    pub daemons_settings: Option<DaemonSettings>,
    /// Experimental named groups of project daemons. Members are daemon names or other group names declared in the same project.
    #[serde(default)]
    pub daemon_groups: IndexMap<String, GroupDeclaration>,
}

/// The `$defs` entries for top-level keys that other sections do not share.
pub(crate) fn schema_defs() -> Vec<(&'static str, Schema)> {
    vec![(
        "monorepo_root",
        json_schema!({
            "description": "Marks this config as a monorepo root, enabling target path syntax for tasks. When enabled, tasks can be referenced with paths like //projects/frontend:build",
            "type": "boolean"
        }),
    )]
}

fn alias_map(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "additionalProperties": {"$ref": "#/$defs/tool_alias_entry"}})
}

fn task_map(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "additionalProperties": {"$ref": "#/$defs/task"}})
}

fn task_template_map(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "additionalProperties": {"$ref": "#/$defs/task_template"}})
}

fn settings(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object", "$ref": "#/$defs/settings"})
}

fn env(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [{"$ref": "#/$defs/env"}, {"type": "array", "items": {"$ref": "#/$defs/env"}}]
    })
}

fn shell_alias(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {"description": "command to run", "type": "string"}
    })
}

fn plugins(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {"description": "url to plugin repository", "type": "string"}
    })
}

fn custom(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "free-form data for your own use; mise never reads it",
        "type": "object",
        "additionalProperties": true
    })
}

fn include(_: &mut SchemaGenerator) -> Schema {
    one_or_many("a config fragment", "config fragments")
}

fn dotenv_files(_: &mut SchemaGenerator) -> Schema {
    one_or_many("dotenv file to load", "dotenv files to load")
}

fn path_entries(_: &mut SchemaGenerator) -> Schema {
    one_or_many("PATH entry to add", "PATH entries to add")
}

/// A string, or a list of them, as `mise_config::de::deserialize_arr` accepts.
fn one_or_many(one: &str, many: &str) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": one, "type": "string"},
            {"description": many, "type": "array", "items": {"type": "string"}}
        ]
    })
}

fn tools(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::tools::schema::tools())
}

fn dotfiles(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<IndexMap<String, crate::dotfiles::DotfileEntryDecl>>()
}

fn dotfile_groups(generator: &mut SchemaGenerator) -> Schema {
    let mut schema =
        generator.subschema_for::<IndexMap<String, crate::dotfiles::DotfileGroupDecl>>();
    schema.insert(
        "propertyNames".into(),
        serde_json::json!({"pattern": "^[A-Za-z0-9_.-]+$"}),
    );
    schema
}

fn daemons(generator: &mut SchemaGenerator) -> Schema {
    generator.subschema_for::<IndexMap<String, crate::daemons::DaemonDecl>>()
}

fn daemon_providers(generator: &mut SchemaGenerator) -> Schema {
    let mut schema =
        generator.subschema_for::<IndexMap<String, crate::daemons::DaemonProviderDecl>>();
    schema.insert(
        "propertyNames".into(),
        serde_json::json!({"pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"}),
    );
    schema
}
