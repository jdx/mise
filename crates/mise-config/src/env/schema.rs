//! The JSON schema for `[env]` and `[vars]`, written by hand next to their parser.
//!
//! The parser in `super` accepts shapes a derive cannot describe: an ordered `_` table of
//! directives, cross-field rules between `value`, `default`, `required` and `age`, and
//! deprecated spellings. Each function below is one `$defs` entry in `schema/mise.json`;
//! change it together with the parser, then run `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by the JSON pointer it replaces in `schema/mise.json`.
pub(crate) fn fragments() -> Vec<(&'static str, Schema)> {
    vec![
        ("/$defs/env", env()),
        ("/$defs/vars", vars()),
        ("/$defs/env_directive", env_directive()),
        ("/$defs/env_value", env_value()),
        ("/$defs/env_path", env_path()),
        ("/$defs/env_paths", env_paths()),
        ("/$defs/env_tools", env_tools()),
        ("/$defs/env_file_expand", env_file_expand()),
        ("/$defs/env_file_find_up", env_file_find_up()),
        ("/$defs/env_redact", env_redact()),
        ("/$defs/env_required", env_required()),
        ("/$defs/env_default", env_default()),
        ("/$defs/env_age", env_age()),
        ("/$defs/env_age_value", env_age_value()),
        ("/$defs/env_age_format", env_age_format()),
    ]
}

fn env() -> Schema {
    json_schema!({
        "additionalProperties": {
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "value": {"$ref": "#/$defs/env_value"},
                        "tools": {"$ref": "#/$defs/env_tools"},
                        "redact": {"$ref": "#/$defs/env_redact"},
                        "required": {
                            "const": false,
                            "description": "a variable with its own value cannot be required; only `required = false` is accepted"
                        }
                    },
                    "required": ["value"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "default": {"$ref": "#/$defs/env_default"},
                        "tools": {"$ref": "#/$defs/env_tools"},
                        "redact": {"$ref": "#/$defs/env_redact"}
                    },
                    "required": ["default"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "tools": {"$ref": "#/$defs/env_tools"},
                        "redact": {"$ref": "#/$defs/env_redact"},
                        "required": {
                            "oneOf": [{"const": true}, {"type": "string"}],
                            "description": "a variable without a value must be required: `true` or help text on how to set it"
                        }
                    },
                    "required": ["required"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "age": {"$ref": "#/$defs/env_age"},
                        "tools": {"$ref": "#/$defs/env_tools"},
                        "redact": {"$ref": "#/$defs/env_redact"},
                        "required": {"$ref": "#/$defs/env_required"}
                    },
                    "required": ["age"],
                    "unevaluatedProperties": false,
                    "description": "[experimental] age-encrypted environment variable"
                },
                {
                    "type": "object",
                    "properties": {
                        "age": {
                            "type": "object",
                            "properties": {
                                "value": {"$ref": "#/$defs/env_age_value"},
                                "format": {"$ref": "#/$defs/env_age_format"},
                                "tools": {"$ref": "#/$defs/env_tools"},
                                "redact": {"$ref": "#/$defs/env_redact"},
                                "required": {"$ref": "#/$defs/env_required"}
                            },
                            "required": ["value"],
                            "unevaluatedProperties": false,
                            "description": "[experimental] age-encrypted value (complex format)"
                        }
                    },
                    "required": ["age"],
                    "unevaluatedProperties": false,
                    "description": "[experimental] age-encrypted environment variable"
                },
                {"$ref": "#/$defs/env_value"}
            ]
        },
        "description": "environment variables",
        "properties": {
            "_": {
                "description": "environment modules",
                "additionalProperties": true,
                "properties": {
                    "file": {
                        "oneOf": [
                            {"$ref": "#/$defs/env_directive"},
                            {
                                "description": "environment file to load (dotenv, json, yaml, or toml)",
                                "type": "string"
                            },
                            {
                                "description": "environment files to load (dotenv, json, yaml, or toml)",
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        {
                                            "description": "environment file to load (dotenv, json, yaml, or toml)",
                                            "type": "string"
                                        },
                                        {"$ref": "#/$defs/env_directive"}
                                    ]
                                }
                            }
                        ]
                    },
                    "path": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "path": {"$ref": "#/$defs/env_path"},
                                    "paths": {"$ref": "#/$defs/env_paths"},
                                    "tools": {"$ref": "#/$defs/env_tools"}
                                },
                                "oneOf": [
                                    {
                                        "type": "object",
                                        "properties": {"path": {"$ref": "#/$defs/env_path"}},
                                        "required": ["path"]
                                    },
                                    {
                                        "type": "object",
                                        "properties": {"paths": {"$ref": "#/$defs/env_paths"}},
                                        "required": ["paths"]
                                    }
                                ]
                            },
                            {"description": "PATH entry to add", "type": "string"},
                            {
                                "description": "PATH entries to add",
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        {"description": "PATH entry to add", "type": "string"},
                                        {
                                            "type": "object",
                                            "properties": {
                                                "path": {"$ref": "#/$defs/env_path"},
                                                "tools": {"$ref": "#/$defs/env_tools"}
                                            }
                                        }
                                    ]
                                }
                            }
                        ]
                    },
                    "python": {
                        "description": "python environment",
                        "properties": {
                            "venv": {
                                "oneOf": [
                                    {
                                        "description": "path to python virtual environment to use",
                                        "type": "string"
                                    },
                                    {
                                        "description": "virtualenv options",
                                        "properties": {
                                            "create": {
                                                "default": false,
                                                "description": "create a new virtual environment if one does not exist",
                                                "type": "boolean"
                                            },
                                            "path": {
                                                "description": "path to python virtual environment to use",
                                                "type": "string"
                                            },
                                            "python": {
                                                "description": "python version to use",
                                                "type": "string"
                                            },
                                            "python_create_args": {
                                                "description": "additional arguments to pass to python when creating a virtual environment",
                                                "type": "array",
                                                "items": {"type": "string"}
                                            },
                                            "uv_create_args": {
                                                "description": "additional arguments to pass to uv when creating a virtual environment",
                                                "type": "array",
                                                "items": {"type": "string"}
                                            }
                                        },
                                        "required": ["path"],
                                        "additionalProperties": false,
                                        "type": "object"
                                    }
                                ]
                            }
                        },
                        "additionalProperties": false,
                        "type": "object"
                    },
                    "source": {
                        "oneOf": [
                            {"$ref": "#/$defs/env_directive"},
                            {"description": "bash script to load", "type": "string"},
                            {
                                "description": "bash scripts to load",
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        {"description": "bash script to load", "type": "string"},
                                        {"$ref": "#/$defs/env_directive"}
                                    ]
                                }
                            }
                        ]
                    }
                },
                "type": "object"
            }
        },
        "type": "object"
    })
}

fn vars() -> Schema {
    json_schema!({
        "description": "variables to set",
        "type": "object",
        "properties": {
            "_": {
                "description": "vars modules",
                "additionalProperties": true,
                "properties": {
                    "file": {
                        "oneOf": [
                            {"$ref": "#/$defs/env_directive"},
                            {
                                "description": "environment file to load (dotenv, json, yaml, or toml)",
                                "type": "string"
                            },
                            {
                                "description": "environment files to load (dotenv, json, yaml, or toml)",
                                "items": {
                                    "oneOf": [
                                        {
                                            "description": "environment file to load (dotenv, json, yaml, or toml)",
                                            "type": "string"
                                        },
                                        {"$ref": "#/$defs/env_directive"}
                                    ]
                                },
                                "type": "array"
                            }
                        ]
                    },
                    "source": {
                        "oneOf": [
                            {"$ref": "#/$defs/env_directive"},
                            {"description": "bash script to load", "type": "string"},
                            {
                                "description": "bash scripts to load",
                                "items": {
                                    "oneOf": [
                                        {"description": "bash script to load", "type": "string"},
                                        {"$ref": "#/$defs/env_directive"}
                                    ]
                                },
                                "type": "array"
                            }
                        ]
                    }
                },
                "type": "object"
            }
        },
        "additionalProperties": {
            "oneOf": [
                {"description": "value of variable", "type": "string"},
                {"description": "value of variable", "type": "integer"},
                {"description": "value of variable", "const": true},
                {
                    "type": "object",
                    "properties": {
                        "value": {"oneOf": [{"type": "string"}, {"type": "integer"}, {"const": true}]},
                        "redact": {"$ref": "#/$defs/env_redact"}
                    },
                    "required": ["value"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "default": {"$ref": "#/$defs/env_default"},
                        "prompt": {
                            "description": "question to ask once, on a machine with no saved answer, when a command is run with a prompt flag such as mise bootstrap --prompt-vars",
                            "type": "string"
                        },
                        "redact": {"$ref": "#/$defs/env_redact"}
                    },
                    "required": ["default"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "description": "question to ask once, on a machine with no saved answer, when a command is run with a prompt flag such as mise bootstrap --prompt-vars",
                            "type": "string"
                        },
                        "redact": {"$ref": "#/$defs/env_redact"},
                        "required": {"oneOf": [{"const": true}, {"type": "string"}]}
                    },
                    "required": ["required"],
                    "unevaluatedProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "age": {"$ref": "#/$defs/env_age"},
                        "redact": {"$ref": "#/$defs/env_redact"}
                    },
                    "required": ["age"],
                    "unevaluatedProperties": false,
                    "description": "[experimental] age-encrypted variable"
                },
                {
                    "type": "object",
                    "properties": {
                        "age": {
                            "type": "object",
                            "properties": {
                                "value": {"$ref": "#/$defs/env_age_value"},
                                "format": {"$ref": "#/$defs/env_age_format"},
                                "redact": {"$ref": "#/$defs/env_redact"}
                            },
                            "required": ["value"],
                            "unevaluatedProperties": false,
                            "description": "[experimental] age-encrypted value (complex format)"
                        }
                    },
                    "required": ["age"],
                    "unevaluatedProperties": false
                }
            ]
        }
    })
}

fn env_directive() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "path": {"$ref": "#/$defs/env_path"},
            "paths": {"$ref": "#/$defs/env_paths"},
            "tools": {"$ref": "#/$defs/env_tools"},
            "expand": {"$ref": "#/$defs/env_file_expand"},
            "find_up": {"$ref": "#/$defs/env_file_find_up"},
            "redact": {"$ref": "#/$defs/env_redact"},
            "required": {"$ref": "#/$defs/env_required"}
        },
        "oneOf": [
            {
                "type": "object",
                "properties": {"path": {"$ref": "#/$defs/env_path"}},
                "required": ["path"]
            },
            {
                "type": "object",
                "properties": {"paths": {"$ref": "#/$defs/env_paths"}},
                "required": ["paths"]
            }
        ]
    })
}

fn env_value() -> Schema {
    json_schema!({
        "description": "environment variable value; false removes the variable",
        "oneOf": [{"type": "string"}, {"type": "integer"}, {"type": "boolean"}]
    })
}

fn env_path() -> Schema {
    json_schema!({"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]})
}

fn env_paths() -> Schema {
    json_schema!({"type": "array", "items": {"type": "string"}})
}

fn env_tools() -> Schema {
    json_schema!({"type": "boolean", "description": "load tools before resolving"})
}

fn env_file_expand() -> Schema {
    json_schema!({
        "type": "boolean",
        "default": false,
        "description": "enable shell-style variable expansion for values loaded by env._.file"
    })
}

fn env_file_find_up() -> Schema {
    json_schema!({
        "type": "boolean",
        "default": false,
        "description": "look for a relative env._.file path in the current directory and each parent directory"
    })
}

fn env_redact() -> Schema {
    json_schema!({"type": "boolean", "description": "redact the value from logs"})
}

fn env_required() -> Schema {
    json_schema!({
        "oneOf": [
            {
                "type": "boolean",
                "description": "require this environment variable to be defined before mise runs or in a later config file"
            },
            {
                "type": "string",
                "description": "require this environment variable with user help text on how to set it"
            }
        ],
        "description": "require this environment variable to be defined before mise runs or in a later config file. Can be a boolean or a help string. A variable that carries its own `value` cannot also be `required = true`."
    })
}

fn env_default() -> Schema {
    json_schema!({
        "oneOf": [
            {
                "type": "string",
                "description": "fallback value to use when the environment variable is unset or empty"
            },
            {
                "type": "integer",
                "description": "fallback value to use when the environment variable is unset or empty"
            }
        ],
        "description": "fallback value to use when the environment variable is unset or empty"
    })
}

fn env_age() -> Schema {
    json_schema!({"type": "string", "description": "[experimental] age-encrypted value (simplified format)"})
}

fn env_age_value() -> Schema {
    json_schema!({"type": "string", "description": "[experimental] age-encrypted value"})
}

fn env_age_format() -> Schema {
    json_schema!({
        "type": "string",
        "enum": ["raw", "zstd"],
        "description": "[experimental] compression format for the encrypted value"
    })
}
