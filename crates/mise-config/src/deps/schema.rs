//! The JSON schema for `[deps]`, written by hand next to its declaration types.
//!
//! [`DepsDecl`](super::DepsDecl) flattens every provider into one map, but the schema lists the
//! built-in providers by name so an editor can offer them. Change it together with the types in
//! `super` and mise's built-in providers, then run `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![("deps_provider", deps_provider())]
}

pub(crate) fn deps() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "disable": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Disable specific deps providers"
            },
            "npm": {"$ref": "#/$defs/deps_provider"},
            "yarn": {"$ref": "#/$defs/deps_provider"},
            "pnpm": {"$ref": "#/$defs/deps_provider"},
            "bun": {"$ref": "#/$defs/deps_provider"},
            "aube": {"$ref": "#/$defs/deps_provider"},
            "deno": {"$ref": "#/$defs/deps_provider"},
            "go": {"$ref": "#/$defs/deps_provider"},
            "pip": {"$ref": "#/$defs/deps_provider"},
            "poetry": {"$ref": "#/$defs/deps_provider"},
            "uv": {"$ref": "#/$defs/deps_provider"},
            "bundler": {"$ref": "#/$defs/deps_provider"},
            "composer": {"$ref": "#/$defs/deps_provider"},
            "dart": {"$ref": "#/$defs/deps_provider"},
            "flutter": {"$ref": "#/$defs/deps_provider"},
            "git-submodule": {"$ref": "#/$defs/deps_provider"}
        },
        "additionalProperties": {"$ref": "#/$defs/deps_provider"}
    })
}

fn deps_provider() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "auto": {
                "type": "boolean",
                "default": false,
                "description": "Auto-run before `mise x` and `mise run`"
            },
            "sources": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Files/patterns to check for changes"
            },
            "outputs": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Files/directories that should be newer than sources"
            },
            "run": {"type": "string", "description": "Command to run when stale"},
            "env": {
                "type": "object",
                "additionalProperties": {"type": "string"},
                "description": "Environment variables to set"
            },
            "dir": {"type": "string", "description": "Working directory for the command"},
            "description": {"type": "string", "description": "Description shown in output"},
            "depends": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Other deps providers that must complete before this one runs"
            },
            "timeout": {
                "type": "string",
                "description": "Timeout for the run command (e.g., \"30s\", \"5m\", \"1h\")"
            }
        },
        "unevaluatedProperties": false,
        "description": "Prepare provider configuration"
    })
}
