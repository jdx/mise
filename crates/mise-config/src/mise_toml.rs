//! The top level of a `mise.toml`: every key mise reads from it.
//!
//! `schema/mise.json` is generated from [`MiseTomlDecl`]. mise still parses a file into its own
//! `MiseToml`, and a test in mise checks that the two accept the same keys; parsing through this
//! type is the next step.

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::alias::Alias;
use crate::bootstrap::BootstrapTomlConfig;
use crate::daemons::{DaemonSettings, GroupDeclaration};
use crate::history::HistoryTomlConfig;
use crate::min_version::MinVersionSpec;
use crate::oci::OciConfig;
use crate::schema::{hand_written, refs};
use crate::wrappers::CommandWrapper;

// The doc comments on these fields are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.
//
// A field whose schema is a `$ref`, or written by hand, takes it from `schema_with`; until mise
// parses through this struct, such a field's type is just the value it holds. Every field is
// optional, and the struct-level `default` keeps any of them from being required.

/// config file for mise version manager (mise.toml)
#[derive(Debug, Default, JsonSchema)]
#[schemars(default)]
pub struct MiseTomlDecl {
    /// custom shorthands
    #[schemars(schema_with = "alias_map", extend("deprecated" = true))]
    pub alias: Option<IndexMap<String, Alias>>,
    /// Tool version aliases
    #[schemars(schema_with = "alias_map")]
    pub tool_alias: Option<IndexMap<String, Alias>>,
    /// shell aliases
    #[schemars(schema_with = "shell_alias")]
    pub shell_alias: Option<IndexMap<String, String>>,
    /// commands that intercept a binary name before delegating to the active toolset
    pub wrappers: Option<IndexMap<String, CommandWrapper>>,
    /// environment variables to set
    #[schemars(schema_with = "env")]
    pub env: Option<toml::Value>,
    /// config files merged into this one, ranking just below it: a local path (relative to this file), git::<url>//<path>.toml?ref=<ref> or oci::<registry>/<repo>[:tag|@sha256:<digest>] (the artifact holds a mise.toml). Paranoid mode requires a commit sha or digest for remote includes and rejects local ones; otherwise a branch or tag is refreshed after fetch_remote_versions_cache
    #[schemars(schema_with = "include")]
    pub include: Option<Vec<String>>,
    /// dotenv file(s) to load. Deprecated; use env._.file instead. This will be removed in mise 2027.4.0.
    #[schemars(schema_with = "dotenv_files", extend("deprecated" = true))]
    pub env_file: Option<Vec<String>>,
    /// dotenv file(s) to load. Deprecated; use env._.file instead. This will be removed in mise 2027.4.0.
    #[schemars(schema_with = "dotenv_files", extend("deprecated" = true))]
    pub dotenv: Option<Vec<String>>,
    /// PATH entries to add. Deprecated; use env._.path instead. This will be removed in mise 2027.4.0.
    #[schemars(schema_with = "path_entries", extend("deprecated" = true))]
    pub env_path: Option<Vec<String>>,
    /// marks this config as a monorepo root for task path syntax
    #[schemars(schema_with = "refs::monorepo_root")]
    pub monorepo_root: Option<bool>,
    /// deprecated alias of monorepo_root
    #[schemars(extend("deprecated" = true))]
    pub experimental_monorepo_root: Option<bool>,
    /// configuration for monorepo task discovery
    #[schemars(schema_with = "refs::monorepo")]
    pub monorepo: Option<toml::Value>,
    /// minimum version of mise required to use this config
    pub min_version: Option<MinVersionSpec>,
    /// [experimental] Sources for mise secrets: values resolved only when mise starts a task or `mise x` command that was granted them. Project config only.
    #[schemars(schema_with = "secrets")]
    pub secrets: Option<toml::Value>,
    /// env or vars keys to redact from logs
    pub redactions: Option<Vec<String>>,
    /// plugins to use
    #[schemars(schema_with = "plugins")]
    pub plugins: Option<IndexMap<String, String>>,
    /// configure deps providers
    #[schemars(schema_with = "deps")]
    pub deps: Option<toml::Value>,
    pub oci: Option<OciConfig>,
    /// mise settings
    #[schemars(schema_with = "settings")]
    pub settings: Option<toml::Table>,
    /// dotfiles applied with `mise dotfiles apply` or `mise bootstrap`, keyed by target path or a logical name when every variant overrides the target; an omitted source for the latter resolves the relative key under dotfiles.root
    #[schemars(schema_with = "dotfiles")]
    pub dotfiles: Option<toml::Value>,
    // The schema has no description for `[dotfile_groups]` yet.
    #[schemars(schema_with = "dotfile_groups")]
    pub dotfile_groups: Option<toml::Value>,
    pub bootstrap: Option<BootstrapTomlConfig>,
    pub history: Option<HistoryTomlConfig>,
    /// configuration for task execution and management
    #[schemars(schema_with = "refs::task_config")]
    pub task_config: Option<toml::Value>,
    /// policy for tools declared by this config root
    #[schemars(schema_with = "refs::tool_config")]
    pub tool_config: Option<toml::Value>,
    /// task runner tasks
    #[schemars(schema_with = "task_map")]
    pub tasks: Option<toml::Table>,
    /// task templates that can be extended by tasks via extends
    #[schemars(schema_with = "task_template_map")]
    pub task_templates: Option<toml::Table>,
    /// dev tools to use
    #[schemars(schema_with = "tools")]
    pub tools: Option<crate::tools::ToolsDecl>,
    /// hooks to run on events like cd, enter, leave
    #[schemars(schema_with = "refs::hooks")]
    pub hooks: Option<toml::Value>,
    /// variables to set for use in config templates
    #[schemars(schema_with = "refs::vars")]
    pub vars: Option<toml::Value>,
    /// files to watch for changes and run scripts when modified
    #[schemars(schema_with = "refs::watch_files")]
    pub watch_files: Option<toml::Value>,
    // `[_]` holds the user's own data, which mise never reads.
    #[schemars(rename = "_", schema_with = "custom")]
    pub custom: Option<toml::Table>,
    #[schemars(schema_with = "refs::doctor")]
    pub doctor: Option<toml::Value>,
    /// Experimental project daemons: custom pitchfork commands or managed presets.
    #[schemars(schema_with = "daemons")]
    pub daemons: Option<toml::Table>,
    /// Experimental shared servers owned by the user. Only allowed in global configuration.
    #[schemars(schema_with = "daemon_providers")]
    pub daemon_providers: Option<toml::Table>,
    pub daemons_settings: Option<DaemonSettings>,
    /// Experimental named groups of project daemons. Members are daemon names or other group names declared in the same project.
    pub daemon_groups: Option<IndexMap<String, GroupDeclaration>>,
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
    json_schema!({"type": "object", "additionalProperties": true})
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

// Raw on purpose in mise: a malformed `[secrets]` must not fail the whole file.
fn secrets(_: &mut SchemaGenerator) -> Schema {
    hand_written(json_schema!({
        "type": "object",
        "properties": {
            "fnox": {
                "description": "Use fnox (fnox.toml discovered from this config's directory). https://mise.jdx.dev/environments/secrets/fnox.html",
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "profile": {
                        "type": "string",
                        "description": "fnox profile (fnox -P); defaults to FNOX_PROFILE, then \"default\""
                    }
                }
            }
        }
    }))
}

fn tools(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::tools::schema::tools())
}

fn deps(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::deps::schema::deps())
}

fn dotfiles(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::dotfiles::schema::dotfiles())
}

fn dotfile_groups(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::dotfiles::schema::dotfile_groups())
}

fn daemons(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::daemons::schema::daemons())
}

fn daemon_providers(_: &mut SchemaGenerator) -> Schema {
    hand_written(crate::daemons::schema::daemon_providers())
}
