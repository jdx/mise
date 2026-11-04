//! `[[watch_files]]`.

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

// An entry runs either a script (`run`, optionally with `shell`) or a task, never both.
#[derive(Debug, Clone, Serialize, Deserialize, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct WatchFile {
    pub patterns: Vec<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    pub task: Option<String>,
}

// Written by hand: one flat struct parses both forms, but the schema lists them as two closed
// variants so tombi, which ignores `not`, still rejects a task entry with a `shell`.
impl JsonSchema for WatchFile {
    fn schema_name() -> Cow<'static, str> {
        "WatchFile".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        let patterns = json_schema!({
            "type": "array",
            "description": "patterns to watch for",
            "items": {"type": "string"}
        });
        json_schema!({
            "type": "object",
            "description": "file to watch for changes",
            "oneOf": [
                {
                    "unevaluatedProperties": false,
                    "properties": {
                        "run": {"type": "string", "description": "script to run when file changes"},
                        "shell": {
                            "type": "string",
                            "description": "inline shell command used to run script when file changes"
                        },
                        "patterns": patterns
                    },
                    "required": ["run", "patterns"]
                },
                {
                    "unevaluatedProperties": false,
                    "properties": {
                        "task": {"type": "string", "description": "mise task to run when file changes"},
                        "patterns": patterns
                    },
                    "required": ["task", "patterns"]
                }
            ]
        })
    }
}
