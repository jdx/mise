//! The structure of the JSON schema for tasks.
//!
//! `task_props`, `task_template` and `task_config` are derived from [`TaskDecl`],
//! [`TaskTemplate`] and [`TaskConfig`] in `crate::schema`, and the fields whose parser accepts
//! several shapes take theirs from the types in `shapes.rs`. What is left here is the composition:
//! the `task` definition, the `tools` of a task, which follow the backend table of `[tools]`, and
//! the template schema, which forbids the keys a template's parser does not read. Change them
//! together with the parser, then run `mise run render:schema`.
//!
//! [`TaskDecl`]: super::TaskDecl
//! [`TaskTemplate`]: super::TaskTemplate
//! [`TaskConfig`]: super::TaskConfig

use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::{Map, Value};

use super::TaskTemplate;

/// The hand-written definitions, keyed by their names under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![("task", task())]
}

/// The tools a task installs before it runs: any tool, with the options of its backend.
pub(super) fn task_props_tools(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "tools to install/activate before running this task",
        "additionalProperties": {"$ref": "#/$defs/tool"},
        "type": "object",
        "patternProperties": crate::tools::task_tool_patterns()
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
