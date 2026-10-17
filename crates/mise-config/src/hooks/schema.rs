//! The JSON schema for `[hooks]`, written by hand next to its declaration types.
//!
//! What an untagged [`HookDefItem`](super::HookDefItem) means depends on the event: a `script`
//! with a `shell` is evaluated in the active shell for `enter`, `leave` and `cd`, while the install
//! hooks ignore the `shell`. A derive cannot tell those apart. Change these together with the types
//! in `super` and mise's `hooks` module, then run `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![
        ("hook_run_string", hook_run_string()),
        ("hook_run_table", hook_run_table()),
        ("hook_script_value", hook_script_value()),
        ("hook_scripts_value", hook_scripts_value()),
        (
            "hook_current_shell_script_table",
            hook_current_shell_script_table(),
        ),
        (
            "hook_current_shell_scripts_table",
            hook_current_shell_scripts_table(),
        ),
        ("hook_spawned_script_table", hook_spawned_script_table()),
        ("hook_spawned_scripts_table", hook_spawned_scripts_table()),
        ("hook_install_script_table", hook_install_script_table()),
        ("hook_install_scripts_table", hook_install_scripts_table()),
        ("hook_task_ref", hook_task_ref()),
        ("hook_definition_item", hook_definition_item()),
        ("hook_definition", hook_definition()),
        (
            "hook_install_definition_item",
            hook_install_definition_item(),
        ),
        ("hook_install_definition", hook_install_definition()),
        ("hooks", hooks()),
    ]
}

fn hook_run_string() -> Schema {
    json_schema!({"description": "inline hook command to run", "type": "string"})
}

fn hook_run_table() -> Schema {
    json_schema!({
        "unevaluatedProperties": false,
        "properties": {
            "run": {"description": "inline hook command to run", "type": "string"},
            "run_windows": {
                "description": "inline hook command to run on Windows",
                "type": "string"
            },
            "shell": {
                "description": "inline shell command to use for run hooks, such as \"bash -c\"",
                "type": "string"
            }
        },
        "anyOf": [{"required": ["run"]}, {"required": ["run_windows"]}],
        "type": "object"
    })
}

fn hook_script_value() -> Schema {
    json_schema!({"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]})
}

fn hook_scripts_value() -> Schema {
    json_schema!({"type": "array", "items": {"type": "string"}})
}

fn hook_current_shell_script_table() -> Schema {
    json_schema!({
        "unevaluatedProperties": false,
        "properties": {
            "script": {
                "description": "script or scripts to evaluate in the active shell for enter/leave/cd hooks",
                "$ref": "#/$defs/hook_script_value"
            },
            "shell": {
                "description": "active shell name for current-shell hooks, such as \"bash\"",
                "type": "string"
            }
        },
        "required": ["script", "shell"],
        "type": "object"
    })
}

fn hook_current_shell_scripts_table() -> Schema {
    json_schema!({
        "unevaluatedProperties": false,
        "properties": {
            "scripts": {
                "description": "scripts to evaluate in the active shell for enter/leave/cd hooks",
                "$ref": "#/$defs/hook_scripts_value"
            },
            "shell": {
                "description": "active shell name for current-shell hooks, such as \"bash\"",
                "type": "string"
            }
        },
        "required": ["scripts", "shell"],
        "type": "object"
    })
}

fn hook_spawned_script_table() -> Schema {
    json_schema!({
        "deprecated": true,
        "unevaluatedProperties": false,
        "properties": {
            "script": {
                "deprecated": true,
                "description": "deprecated alias for run when an enter/leave/cd hook has no shell",
                "$ref": "#/$defs/hook_script_value"
            }
        },
        "required": ["script"],
        "type": "object"
    })
}

fn hook_spawned_scripts_table() -> Schema {
    json_schema!({
        "deprecated": true,
        "unevaluatedProperties": false,
        "properties": {
            "scripts": {
                "deprecated": true,
                "description": "deprecated alias for run when an enter/leave/cd hook has no shell",
                "$ref": "#/$defs/hook_scripts_value"
            }
        },
        "required": ["scripts"],
        "type": "object"
    })
}

fn hook_install_script_table() -> Schema {
    json_schema!({
        "deprecated": true,
        "unevaluatedProperties": false,
        "properties": {
            "script": {
                "deprecated": true,
                "description": "deprecated alias for run in preinstall/postinstall hooks; executes in a subprocess",
                "$ref": "#/$defs/hook_script_value"
            },
            "shell": {
                "deprecated": true,
                "description": "ignored for preinstall/postinstall hooks that use script; use a run hook to select an inline shell",
                "type": "string"
            }
        },
        "required": ["script"],
        "type": "object"
    })
}

fn hook_install_scripts_table() -> Schema {
    json_schema!({
        "deprecated": true,
        "unevaluatedProperties": false,
        "properties": {
            "scripts": {
                "deprecated": true,
                "description": "deprecated alias for run in preinstall/postinstall hooks; joins the scripts and executes them in a subprocess",
                "$ref": "#/$defs/hook_scripts_value"
            },
            "shell": {
                "deprecated": true,
                "description": "ignored for preinstall/postinstall hooks that use scripts; use a run hook to select an inline shell",
                "type": "string"
            }
        },
        "required": ["scripts"],
        "type": "object"
    })
}

fn hook_task_ref() -> Schema {
    json_schema!({
        "unevaluatedProperties": false,
        "properties": {"task": {"description": "mise task to run", "type": "string"}},
        "required": ["task"],
        "type": "object"
    })
}

fn hook_definition_item() -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/hook_run_string"},
            {"$ref": "#/$defs/hook_run_table"},
            {"$ref": "#/$defs/hook_current_shell_script_table"},
            {"$ref": "#/$defs/hook_current_shell_scripts_table"},
            {"$ref": "#/$defs/hook_spawned_script_table"},
            {"$ref": "#/$defs/hook_spawned_scripts_table"},
            {"$ref": "#/$defs/hook_task_ref"}
        ]
    })
}

fn hook_definition() -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/hook_definition_item"},
            {
                "description": "scripts/tasks to run",
                "items": {"$ref": "#/$defs/hook_definition_item"},
                "type": "array"
            }
        ]
    })
}

fn hook_install_definition_item() -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/hook_run_string"},
            {"$ref": "#/$defs/hook_run_table"},
            {"$ref": "#/$defs/hook_install_script_table"},
            {"$ref": "#/$defs/hook_install_scripts_table"},
            {"$ref": "#/$defs/hook_task_ref"}
        ]
    })
}

fn hook_install_definition() -> Schema {
    json_schema!({
        "oneOf": [
            {"$ref": "#/$defs/hook_install_definition_item"},
            {
                "description": "install hooks to run",
                "items": {"$ref": "#/$defs/hook_install_definition_item"},
                "type": "array"
            }
        ]
    })
}

fn hooks() -> Schema {
    json_schema!({
        "description": "hooks to run",
        "type": "object",
        "properties": {
            "enter": {"$ref": "#/$defs/hook_definition"},
            "leave": {"$ref": "#/$defs/hook_definition"},
            "cd": {"$ref": "#/$defs/hook_definition"},
            "preinstall": {"$ref": "#/$defs/hook_install_definition"},
            "postinstall": {"$ref": "#/$defs/hook_install_definition"}
        },
        "additionalProperties": false
    })
}
