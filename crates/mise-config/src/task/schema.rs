//! The JSON schema for tasks, written by hand next to the declaration types.
//!
//! `task_props`, `task_template` and `task_config` are derived from [`TaskDecl`],
//! [`TaskTemplate`] and [`TaskConfig`] in `crate::schema`. Fields whose parser accepts several
//! shapes (a string or a list, a name or a table, ...) take their schema from a function here,
//! as do the small `$defs` entries the task schema shares. Change them together with the
//! parser, then run `mise run render:schema`.
//!
//! [`TaskDecl`]: super::TaskDecl
//! [`TaskTemplate`]: super::TaskTemplate
//! [`TaskConfig`]: super::TaskConfig

use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::{Map, Value};

use super::TaskTemplate;

/// The hand-written definitions, keyed by their names under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![
        ("task", task()),
        ("task_dependency_item", task_dependency_item()),
        ("task_dependency_table", task_dependency_table()),
        ("task_run_entry", task_run_entry()),
        ("task_watch", task_watch()),
    ]
}

pub(super) fn task_props_alias(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "alias for this task", "type": "string"},
            {
                "description": "alias for this task",
                "items": {"description": "alias for this task", "type": "string"},
                "type": "array"
            }
        ]
    })
}

pub(super) fn task_props_confirm(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "confirmation message before running this task", "type": "string"},
            {
                "description": "confirmation message before running this task with optional default value and custom answer labels",
                "properties": {
                    "message": {
                        "description": "confirmation message before running this task",
                        "type": "string"
                    },
                    "default": {
                        "description": "default value for confirmation (yes/no/true/false)",
                        "enum": ["yes", "no", "y", "n", "true", "false"],
                        "type": "string"
                    },
                    "yes": {"description": "custom label for the affirmative answer", "type": "string"},
                    "no": {"description": "custom label for the negative answer", "type": "string"}
                },
                "required": ["message"],
                "unevaluatedProperties": false,
                "type": "object"
            }
        ]
    })
}

pub(super) fn task_props_depends(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "task with args to run before this task", "type": "string"},
            {
                "description": "task with args to run before this task",
                "type": "array",
                "items": {"$ref": "#/$defs/task_dependency_item"}
            },
            {"$ref": "#/$defs/task_dependency_table"}
        ]
    })
}

pub(super) fn task_props_daemons(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "[experimental] project daemons that must be running and ready before this task runs",
        "oneOf": [
            {
                "description": "true requires every daemon declared in [daemons]",
                "type": "boolean"
            },
            {"description": "name of a daemon declared in [daemons]", "type": "string"},
            {
                "description": "names of daemons declared in [daemons]",
                "type": "array",
                "items": {"type": "string"}
            }
        ]
    })
}

pub(super) fn task_props_depends_post(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "task with args to run after this task", "type": "string"},
            {
                "description": "task with args to run after this task",
                "type": "array",
                "items": {"$ref": "#/$defs/task_dependency_item"}
            },
            {"$ref": "#/$defs/task_dependency_table"}
        ]
    })
}

pub(super) fn task_props_wait_for(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "task with args to wait for completion first", "type": "string"},
            {
                "description": "task with args to wait for completion first",
                "items": {"$ref": "#/$defs/task_dependency_item"},
                "type": "array"
            },
            {"$ref": "#/$defs/task_dependency_table"}
        ]
    })
}

pub(super) fn task_props_tools(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "tools to install/activate before running this task",
        "additionalProperties": {"$ref": "#/$defs/tool"},
        "type": "object",
        "patternProperties": {
            "^github:": {"$ref": "#/$defs/tool_github"},
            "^gitlab:": {"$ref": "#/$defs/tool_gitlab"},
            "^forgejo:": {"$ref": "#/$defs/tool_forgejo"},
            "^ubi:": {"$ref": "#/$defs/tool_ubi"},
            "^http:": {"$ref": "#/$defs/tool_http"},
            "^s3:": {"$ref": "#/$defs/tool_s3"},
            "^aqua:": {"$ref": "#/$defs/tool_aqua"},
            "^cargo:": {"$ref": "#/$defs/tool_cargo"},
            "^npm:": {"$ref": "#/$defs/tool_npm"},
            "^(pypi|pipx):": {"$ref": "#/$defs/tool_pypi"},
            "^gem:": {"$ref": "#/$defs/tool_gem"},
            "^go:": {"$ref": "#/$defs/tool_go"},
            "^conda:": {"$ref": "#/$defs/tool_conda"},
            "^spm:": {"$ref": "#/$defs/tool_spm"},
            "^packslip:": {"$ref": "#/$defs/tool_packslip"},
            "^spinel:": {"$ref": "#/$defs/tool_spinel"},
            "^(core:)?python(\\[.*\\])?$": {"$ref": "#/$defs/tool_core_python"},
            "^(core:)?java(\\[.*\\])?$": {"$ref": "#/$defs/tool_core_java"},
            "^(core:)?rust(\\[.*\\])?$": {"$ref": "#/$defs/tool_core_rust"},
            "^(core:dotnet|dotnet|dotnet-core)(\\[.*\\])?$": {"$ref": "#/$defs/tool_core_dotnet"}
        }
    })
}

pub(super) fn task_props_outputs(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {
                "description": "files created by this task (entries starting with `!` are excluded; use `\\!` to escape a literal `!` prefix)",
                "items": {
                    "description": "glob pattern or path to files created by this task (entries starting with `!` are excluded)",
                    "type": "string"
                },
                "type": "array"
            },
            {
                "description": "glob pattern or path to files created by this task (entries starting with `!` are excluded)",
                "type": "string"
            },
            {
                "unevaluatedProperties": false,
                "properties": {
                    "auto": {
                        "description": "automatically touch an internal tracked file instead of specifying outputs; `false` is the same as leaving `outputs` unset",
                        "type": "boolean"
                    }
                },
                "required": ["auto"],
                "type": "object"
            }
        ]
    })
}

pub(super) fn task_props_cache(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "experimental local artifact cache for declared task outputs",
        "type": "object",
        "unevaluatedProperties": false,
        "properties": {
            "enabled": {
                "default": false,
                "description": "store and restore task outputs from the local mise cache",
                "type": "boolean"
            },
            "audit": {
                "default": false,
                "description": "report project files read or written outside declared cache sources and outputs",
                "type": "boolean"
            },
            "env": {
                "description": "ambient environment variable names whose values affect the cache key",
                "items": {"type": "string"},
                "type": "array"
            },
            "command_inputs": {
                "description": "commands whose stdout and stderr affect the cache key",
                "items": {"type": "string"},
                "type": "array"
            }
        }
    })
}

pub(super) fn task_props_rust_cache(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "deprecated": true,
        "description": "deprecated no-op; use mbx for Rust action caching instead: https://mr-boxington.jdx.dev/getting-started",
        "oneOf": [
            {"type": "boolean"},
            {
                "type": "object",
                "unevaluatedProperties": false,
                "properties": {
                    "enabled": {"type": "boolean", "default": true},
                    "verify": {
                        "type": "boolean",
                        "default": false,
                        "deprecated": true,
                        "description": "deprecated no-op"
                    }
                }
            }
        ]
    })
}

pub(super) fn task_props_output(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "output style for this task (prefix, interleave, keep-order, replacing, timed, quiet, silent). Independent of the quiet/silent verbosity fields.",
        "enum": ["prefix", "interleave", "keep-order", "replacing", "timed", "quiet", "silent"],
        "type": "string"
    })
}

pub(super) fn task_props_silent(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "default": false,
        "description": "suppress all output for this task",
        "oneOf": [{"type": "boolean"}, {"enum": ["stdout", "stderr"], "type": "string"}]
    })
}

pub(super) fn task_props_run(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/task_run_entry"},
            {"type": "array", "items": {"$ref": "#/$defs/task_run_entry"}}
        ]
    })
}

pub(super) fn task_props_run_windows(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/task_run_entry"},
            {"type": "array", "items": {"$ref": "#/$defs/task_run_entry"}}
        ]
    })
}

pub(super) fn task_props_sources(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "oneOf": [
            {
                "description": "files that this task depends on (entries starting with `!` are excluded; use `\\!` to escape a literal `!` prefix)",
                "items": {
                    "description": "glob pattern or path to files that this task depends on (entries starting with `!` are excluded)",
                    "type": "string"
                },
                "type": "array"
            },
            {
                "description": "glob pattern or path to files that this task depends on (entries starting with `!` are excluded)",
                "type": "string"
            }
        ]
    })
}

pub(super) fn task_props_secrets(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "[experimental] Secret keys this task receives when it starts, e.g. [\"DEPLOY_KEY\"]. Dependencies do not inherit them. See `mise secrets ls`.",
        "oneOf": [
            {"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"},
            {
                "type": "array",
                "items": {"type": "string", "pattern": "^[A-Za-z_][A-Za-z0-9_]*$"}
            }
        ]
    })
}

pub(super) fn task_config_cache(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "default experimental local artifact cache configuration for eligible tasks with sources and explicit outputs",
        "type": "object",
        "unevaluatedProperties": false,
        "properties": {
            "enabled": {
                "default": false,
                "description": "store and restore eligible task outputs from the local mise cache",
                "type": "boolean"
            },
            "audit": {
                "default": false,
                "description": "report project files read or written outside declared cache sources and outputs",
                "type": "boolean"
            },
            "env": {
                "description": "ambient environment variable names whose values affect eligible task cache keys",
                "items": {"type": "string"},
                "type": "array"
            },
            "command_inputs": {
                "description": "commands whose stdout and stderr affect eligible task cache keys",
                "items": {"type": "string"},
                "type": "array"
            }
        }
    })
}

pub(super) fn task_config_rust_cache(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "deprecated": true,
        "description": "deprecated no-op; use mbx for Rust action caching instead: https://mr-boxington.jdx.dev/getting-started",
        "oneOf": [
            {"type": "boolean"},
            {
                "type": "object",
                "unevaluatedProperties": false,
                "properties": {
                    "enabled": {"type": "boolean", "default": true},
                    "verify": {
                        "type": "boolean",
                        "default": false,
                        "deprecated": true,
                        "description": "deprecated no-op"
                    }
                }
            }
        ]
    })
}

pub(super) fn task_config_includes(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "files/directories to include searching for tasks. Can be local paths, git repository URLs using git:: prefix (e.g., git::https://github.com/org/repo.git//path?ref=branch), or OCI artifacts using oci:: prefix (e.g., oci::ghcr.io/org/tasks:1.0.0)",
        "items": {
            "description": "file/directory root to include in task execution. Supports local paths, git URLs (git::https://github.com/org/repo.git//path?ref=branch), and OCI artifacts (oci::ghcr.io/org/tasks:1.0.0)",
            "type": "string"
        },
        "type": "array"
    })
}

fn task() -> Schema {
    json_schema!({
        "description": "a task: a script, a list of scripts and task references, or a table",
        "oneOf": [
            {"description": "script to run", "type": "string"},
            {
                "description": "scripts or task references to run",
                "items": {"$ref": "#/$defs/task_run_entry"},
                "type": "array"
            },
            {
                "allOf": [
                    {"$ref": "#/$defs/task_props"},
                    {
                        "properties": {
                            "extends": {
                                "description": "name of the task template to extend",
                                "type": "string"
                            }
                        }
                    }
                ],
                "unevaluatedProperties": false,
                "type": "object"
            }
        ]
    })
}

fn task_dependency_item() -> Schema {
    json_schema!({
        "description": "task name and args",
        "oneOf": [
            {"type": "string"},
            {"type": "array", "items": {"type": "string"}, "minItems": 1},
            {"$ref": "#/$defs/task_dependency_table"}
        ]
    })
}

fn task_dependency_table() -> Schema {
    json_schema!({
        "description": "task dependency with args, env, and optional flag",
        "type": "object",
        "properties": {
            "task": {"type": "string"},
            "args": {"type": "array", "items": {"type": "string"}},
            "env": {"type": "object", "additionalProperties": {"type": "string"}},
            "optional": {
                "description": "do not fail when the task name or pattern has no matches",
                "type": "boolean"
            }
        },
        "required": ["task"],
        "unevaluatedProperties": false
    })
}

fn task_run_entry() -> Schema {
    json_schema!({
        "oneOf": [
            {"description": "script to run", "type": "string"},
            {
                "type": "object",
                "unevaluatedProperties": false,
                "properties": {
                    "task": {"description": "single task name to run", "type": "string"},
                    "args": {
                        "description": "arguments to pass to the task",
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "env": {
                        "description": "environment variables to set for the task",
                        "type": "object",
                        "additionalProperties": {"type": "string"}
                    }
                },
                "required": ["task"]
            },
            {
                "type": "object",
                "unevaluatedProperties": false,
                "properties": {
                    "tasks": {
                        "description": "parallel task group to run",
                        "items": {"description": "task name and args", "type": "string"},
                        "type": "array"
                    }
                },
                "required": ["tasks"]
            }
        ]
    })
}

fn task_watch() -> Schema {
    json_schema!({
        "description": "options used by `mise watch` for this task",
        "type": "object",
        "unevaluatedProperties": false,
        "properties": {
            "no_vcs_ignore": {
                "default": false,
                "description": "do not load VCS ignore files such as `.gitignore` when watching this task",
                "type": "boolean"
            }
        }
    })
}

/// `task_props` with every key a template cannot set turned off. Those are the keys
/// [`TaskTemplate`]'s parser does not read, plus `secrets`, which it reads only to reject (G7).
pub(crate) fn task_template(task_props: &Value) -> Value {
    let mut forbidden: Vec<&str> = task_props["properties"]
        .as_object()
        .into_iter()
        .flat_map(|properties| properties.keys())
        .map(String::as_str)
        .filter(|key| !template_reads(key))
        .chain(["secrets"])
        .collect();
    forbidden.sort_unstable();
    forbidden.dedup();
    let forbidden: Map<String, Value> = forbidden
        .into_iter()
        .map(|key| (key.to_string(), Value::Bool(false)))
        .collect();
    serde_json::json!({
        "description": "task template that can be extended by tasks",
        "allOf": [{"$ref": "#/$defs/task_props"}, {"properties": forbidden}],
        "unevaluatedProperties": false,
        "type": "object"
    })
}

/// Whether a `[task_templates.<name>]` table reads `key`. An unread key is reported as ignored;
/// a read key with the wrong type fails instead, which is fine here.
fn template_reads(key: &str) -> bool {
    let table = toml::Table::from_iter([(key.to_string(), toml::Value::Boolean(true))]);
    let mut ignored = false;
    let _: Result<TaskTemplate, _> =
        serde_ignored::deserialize(toml::Value::Table(table), |_| ignored = true);
    !ignored
}
