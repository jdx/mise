//! The structure of the JSON schema for `[tools]`.
//!
//! Which options a tool takes depends on its backend, which only its key names, so the schema
//! picks a backend's option set with `patternProperties` on the key (`^github:`, `^npm:`, ...).
//! None of that is visible to a derive on [`ToolSpecList`](super::ToolSpecList), whose options are
//! raw until mise checks them. The options themselves are the declaration types in `decl.rs`;
//! this file composes them: one wrapper per backend, the platform tables that override some of a
//! backend's options, and the pattern that picks each backend. Change them together with mise's
//! option parsing (`ToolVersionOptions::insert_option`) and the backends' option readers, then
//! run `mise run render:schema`.

use schemars::{Schema, json_schema};
use serde_json::{Value, json};

/// A backend, as the schema picks it by the key of a tool.
struct Backend {
    /// The `$defs` name of the wrapper, `tool_<backend>`.
    def: &'static str,
    /// What a tool key matches to use this backend.
    pattern: &'static str,
    description: &'static str,
    /// The option sets that apply, besides the ones every tool has.
    options: &'static [&'static str],
    /// The `$defs` name of the table of per-platform overrides, if the backend has one.
    platforms: Option<&'static str>,
}

/// A table of per-platform overrides: some of a backend's options, which each platform may set.
struct PlatformTable {
    def: &'static str,
    /// The `$defs` name of the options the keys come from.
    from: &'static str,
    keys: &'static [&'static str],
    /// Options only a platform has, written as JSON.
    local: &'static [(&'static str, &'static str)],
}

const BACKENDS: &[Backend] = &[
    Backend {
        def: "tool_github",
        pattern: "^github:",
        description: "a tool installed with the github backend",
        options: &["tool_forge_options", "tool_github_options"],
        platforms: Some("tool_forge_platforms"),
    },
    Backend {
        def: "tool_gitlab",
        pattern: "^gitlab:",
        description: "a tool installed with the gitlab backend",
        options: &["tool_forge_options", "tool_gitlab_options"],
        platforms: Some("tool_forge_platforms"),
    },
    Backend {
        def: "tool_forgejo",
        pattern: "^forgejo:",
        description: "a tool installed with the forgejo backend",
        options: &["tool_forge_options", "tool_forgejo_options"],
        platforms: Some("tool_forge_platforms"),
    },
    Backend {
        def: "tool_ubi",
        pattern: "^ubi:",
        description: "a tool installed with the ubi backend",
        options: &["tool_ubi_options"],
        platforms: Some("tool_ubi_platforms"),
    },
    Backend {
        def: "tool_http",
        pattern: "^http:",
        description: "a tool installed with the http backend",
        options: &["tool_http_options"],
        platforms: Some("tool_http_platforms"),
    },
    Backend {
        def: "tool_s3",
        pattern: "^s3:",
        description: "a tool installed with the s3 backend",
        options: &["tool_s3_options"],
        platforms: Some("tool_s3_platforms"),
    },
    Backend {
        def: "tool_aqua",
        pattern: "^aqua:",
        description: "a tool installed with the aqua backend",
        options: &["tool_aqua_options"],
        platforms: None,
    },
    Backend {
        def: "tool_cargo",
        pattern: "^cargo:",
        description: "a tool installed with the cargo backend",
        options: &["tool_cargo_options"],
        platforms: Some("tool_cargo_platforms"),
    },
    Backend {
        def: "tool_npm",
        pattern: "^npm:",
        description: "a tool installed with the npm backend",
        options: &["tool_npm_options"],
        platforms: None,
    },
    Backend {
        def: "tool_pypi",
        pattern: "^(pypi|pipx):",
        description: "a tool installed with the pypi backend",
        options: &["tool_pypi_options"],
        platforms: None,
    },
    Backend {
        def: "tool_gem",
        pattern: "^gem:",
        description: "a tool installed with the gem backend",
        options: &["tool_gem_options"],
        platforms: None,
    },
    Backend {
        def: "tool_go",
        pattern: "^go:",
        description: "a tool installed with the go backend",
        options: &["tool_go_options"],
        platforms: None,
    },
    Backend {
        def: "tool_conda",
        pattern: "^conda:",
        description: "a tool installed with the conda backend",
        options: &["tool_conda_options"],
        platforms: None,
    },
    Backend {
        def: "tool_spm",
        pattern: "^spm:",
        description: "a tool installed with the spm backend",
        options: &["tool_spm_options"],
        platforms: Some("tool_spm_platforms"),
    },
    Backend {
        def: "tool_packslip",
        pattern: "^packslip:",
        description: "a tool installed with the packslip backend",
        options: &["tool_packslip_options"],
        platforms: None,
    },
    Backend {
        def: "tool_spinel",
        pattern: "^spinel:",
        description: "a tool installed with the spinel backend",
        options: &["tool_spinel_options"],
        platforms: None,
    },
    Backend {
        def: "tool_core_python",
        pattern: "^(core:)?python(\\[.*\\])?$",
        description: "the core python tool",
        options: &["tool_core_python_options"],
        platforms: None,
    },
    Backend {
        def: "tool_core_java",
        pattern: "^(core:)?java(\\[.*\\])?$",
        description: "the core java tool",
        options: &["tool_core_java_options"],
        platforms: None,
    },
    Backend {
        def: "tool_core_rust",
        pattern: "^(core:)?rust(\\[.*\\])?$",
        description: "the core rust tool",
        options: &["tool_core_rust_options"],
        platforms: None,
    },
    Backend {
        def: "tool_core_dotnet",
        pattern: "^(core:dotnet|dotnet|dotnet-core)(\\[.*\\])?$",
        description: "the core dotnet tool",
        options: &["tool_core_dotnet_options"],
        platforms: None,
    },
];

const PLATFORM_TABLES: &[PlatformTable] = &[
    PlatformTable {
        def: "tool_forge_platforms",
        from: "tool_forge_options",
        keys: &[
            "asset_pattern",
            "additional_asset_patterns",
            "no_app",
            "filter_bins",
            "checksum",
            "size",
            "strip_components",
            "bin",
            "rename_exe",
            "bin_path",
            "format",
        ],
        local: &[(
            "url",
            "{\"description\": \"direct download url for this platform, tera templated; skips release asset selection\", \"type\": \"string\"}",
        )],
    },
    PlatformTable {
        def: "tool_ubi_platforms",
        from: "tool_ubi_options",
        keys: &["bin_path"],
        local: &[],
    },
    PlatformTable {
        def: "tool_http_platforms",
        from: "tool_http_options",
        keys: &[
            "url",
            "headers",
            "headers_forward",
            "checksum",
            "checksum_url",
            "size",
            "format",
            "strip_components",
            "bin",
            "rename_exe",
            "bin_path",
            "windows_script_interpreter",
        ],
        local: &[],
    },
    PlatformTable {
        def: "tool_s3_platforms",
        from: "tool_s3_options",
        keys: &[
            "url",
            "checksum",
            "size",
            "format",
            "strip_components",
            "bin",
            "rename_exe",
            "bin_path",
            "region",
            "endpoint",
            "version_list_url",
            "version_prefix",
            "version_regex",
            "version_json_path",
            "version_expr",
        ],
        local: &[],
    },
    PlatformTable {
        def: "tool_cargo_platforms",
        from: "tool_cargo_options",
        keys: &["bin"],
        local: &[],
    },
    PlatformTable {
        def: "tool_spm_platforms",
        from: "tool_spm_options",
        keys: &[
            "provider",
            "api_url",
            "artifactbundle",
            "artifactbundle_asset",
        ],
        local: &[],
    },
];

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    let mut defs = vec![
        ("tool", tool()),
        ("tool_bool", tool_bool()),
        ("tool_bool_text", tool_bool_text()),
        ("tool_platforms", tool_platforms()),
    ];
    defs.extend(PLATFORM_TABLES.iter().map(|t| (t.def, platform_table(t))));
    defs.extend(BACKENDS.iter().map(|b| (b.def, backend(b))));
    defs
}

pub(crate) fn tools() -> Schema {
    let patterns: serde_json::Map<String, Value> = BACKENDS
        .iter()
        .map(|b| {
            let wrapper = json!({"$ref": format!("#/$defs/{}", b.def)});
            (
                b.pattern.to_string(),
                json!({
                    "description": b.description,
                    "oneOf": [{"type": "array", "items": wrapper}, wrapper]
                }),
            )
        })
        .collect();
    json_schema!({
        "patternProperties": patterns,
        "additionalProperties": {
            "description": "a version of the tool, or a table with its version and options",
            "oneOf": [{"items": {"$ref": "#/$defs/tool"}, "type": "array"}, {"$ref": "#/$defs/tool"}]
        },
        "type": "object"
    })
}

/// The tools of a task, which key a backend by the same patterns as `[tools]` but take one
/// tool each, not a list of them.
pub(crate) fn task_tool_patterns() -> serde_json::Map<String, Value> {
    BACKENDS
        .iter()
        .map(|b| {
            (
                b.pattern.to_string(),
                json!({"$ref": format!("#/$defs/{}", b.def)}),
            )
        })
        .collect()
}

/// A tool of this backend: a version, or a table of the options every tool has and the ones its
/// backend adds.
fn backend(backend: &Backend) -> Schema {
    let mut all_of = vec![json!({"$ref": "#/$defs/tool"})];
    all_of.extend(
        backend
            .options
            .iter()
            .map(|options| json!({"$ref": format!("#/$defs/{options}")})),
    );
    if let Some(platforms) = backend.platforms {
        let table = json!({"$ref": format!("#/$defs/{platforms}")});
        all_of.push(json!({"properties": {
            "platforms": {"description": "per-platform option overrides", "$ref": table["$ref"]},
            "platform": {
                "description": "per-platform option overrides (alias of platforms)",
                "$ref": table["$ref"]
            }
        }}));
    }
    json_schema!({
        "description": backend.description,
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {"allOf": all_of, "type": "object"}
        ]
    })
}

fn platform_table(table: &PlatformTable) -> Schema {
    let mut properties: serde_json::Map<String, Value> = table
        .keys
        .iter()
        .map(|key| {
            (
                (*key).to_string(),
                json!({"$ref": format!("#/$defs/{}/properties/{key}", table.from)}),
            )
        })
        .collect();
    for (key, schema) in table.local {
        properties.insert(
            (*key).to_string(),
            serde_json::from_str(schema).expect("a platform option is JSON"),
        );
    }
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "description": "options for this platform, overriding the same options set for the whole tool",
            "type": "object",
            "properties": properties
        }
    })
}

fn tool() -> Schema {
    json_schema!({
        "description": "a version of the tool, or a table with its version and options",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool_options"},
                    {
                        "properties": {
                            "auto_update": {
                                "description": "update this tool before it runs when its check is due; true, or a check interval such as \"12h\". Only honored in global config",
                                "oneOf": [{"type": "boolean"}, {"type": "string"}]
                            },
                            "install_env": {
                                "type": "object",
                                "additionalProperties": {"$ref": "#/$defs/env_value"},
                                "description": "environment variables during tool installation"
                            },
                            "depends": {
                                "oneOf": [
                                    {
                                        "description": "tools that must be installed before this tool",
                                        "type": "string"
                                    },
                                    {
                                        "description": "tools that must be installed before this tool",
                                        "type": "array",
                                        "items": {"type": "string"}
                                    }
                                ]
                            },
                            "lazy": {
                                "description": "defer installation until one of the tool's bootstrap shims is invoked",
                                "type": "boolean",
                                "default": false
                            },
                            "lazy_bins": {
                                "description": "bootstrap shim names for lazy tools without registry bin metadata",
                                "oneOf": [
                                    {"type": "string", "minLength": 1, "pattern": "^[^/\\\\]+$"},
                                    {
                                        "type": "array",
                                        "items": {
                                            "type": "string",
                                            "minLength": 1,
                                            "pattern": "^[^/\\\\]+$"
                                        }
                                    }
                                ]
                            },
                            "postinstall": {
                                "description": "command to run after tool installation, or on every install when requested",
                                "oneOf": [
                                    {"type": "string"},
                                    {
                                        "type": "object",
                                        "properties": {
                                            "run": {
                                                "description": "command to run after the tool installs",
                                                "type": "string"
                                            },
                                            "when": {
                                                "description": "install runs the command only when mise installs the tool; always runs it on every mise install and mise use",
                                                "type": "string",
                                                "enum": ["install", "always"],
                                                "default": "install"
                                            }
                                        },
                                        "required": ["run"],
                                        "additionalProperties": false
                                    }
                                ]
                            }
                        }
                    }
                ],
                "type": "object",
                "unevaluatedProperties": {
                    "oneOf": [
                        {"type": "string"},
                        {"type": "number"},
                        {"type": "boolean"},
                        {"type": "array"},
                        {"type": "object", "additionalProperties": true}
                    ]
                }
            }
        ]
    })
}

fn tool_bool() -> Schema {
    json_schema!({
        "description": "a boolean, also accepted as the strings true/false or the integers 1/0",
        "oneOf": [
            {"type": "boolean"},
            {
                "type": "string",
                "pattern": "^\\s*([Tt][Rr][Uu][Ee]|[Ff][Aa][Ll][Ss][Ee]|1|0)\\s*$"
            },
            {"type": "integer", "enum": [0, 1]}
        ]
    })
}

fn tool_bool_text() -> Schema {
    json_schema!({
        "description": "a boolean, also accepted as the string true or false",
        "oneOf": [
            {"type": "boolean"},
            {"type": "string", "pattern": "^([Tt][Rr][Uu][Ee]|[Ff][Aa][Ll][Ss][Ee])$"}
        ]
    })
}

fn tool_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "description": "options for this platform, overriding the same options set for the whole tool",
            "type": "object"
        }
    })
}
