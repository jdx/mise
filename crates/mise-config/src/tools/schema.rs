//! The JSON schema for `[tools]`, written by hand next to its declaration types.
//!
//! Which options a tool takes depends on its backend, which only its key names, so the schema
//! picks per-backend option sets with `patternProperties` on the key (`^github:`, `^npm:`, ...).
//! None of that is visible to a derive on [`ToolSpecList`](super::ToolSpecList), whose options are
//! raw until mise checks them. Change these together with mise's option parsing
//! (`ToolVersionOptions::insert_option`) and the backends' option readers, then run
//! `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by the JSON pointer it replaces in `schema/mise.json`.
pub(crate) fn fragments() -> Vec<(&'static str, Schema)> {
    vec![
        ("/properties/tools", tools()),
        ("/$defs/tool", tool()),
        ("/$defs/tool_bool", tool_bool()),
        ("/$defs/tool_bool_text", tool_bool_text()),
        ("/$defs/tool_platforms", tool_platforms()),
        ("/$defs/tool_forge_options", tool_forge_options()),
        ("/$defs/tool_github_options", tool_github_options()),
        ("/$defs/tool_forge_platforms", tool_forge_platforms()),
        ("/$defs/tool_github", tool_github()),
        ("/$defs/tool_gitlab_options", tool_gitlab_options()),
        ("/$defs/tool_gitlab", tool_gitlab()),
        ("/$defs/tool_forgejo_options", tool_forgejo_options()),
        ("/$defs/tool_forgejo", tool_forgejo()),
        ("/$defs/tool_ubi_options", tool_ubi_options()),
        ("/$defs/tool_ubi_platforms", tool_ubi_platforms()),
        ("/$defs/tool_ubi", tool_ubi()),
        ("/$defs/tool_http_options", tool_http_options()),
        ("/$defs/tool_http_platforms", tool_http_platforms()),
        ("/$defs/tool_http", tool_http()),
        ("/$defs/tool_s3_options", tool_s3_options()),
        ("/$defs/tool_s3_platforms", tool_s3_platforms()),
        ("/$defs/tool_s3", tool_s3()),
        ("/$defs/tool_aqua_options", tool_aqua_options()),
        ("/$defs/tool_aqua", tool_aqua()),
        ("/$defs/tool_cargo_options", tool_cargo_options()),
        ("/$defs/tool_cargo_platforms", tool_cargo_platforms()),
        ("/$defs/tool_cargo", tool_cargo()),
        ("/$defs/tool_npm_options", tool_npm_options()),
        ("/$defs/tool_npm", tool_npm()),
        ("/$defs/tool_pypi_options", tool_pypi_options()),
        ("/$defs/tool_pypi", tool_pypi()),
        ("/$defs/tool_gem_options", tool_gem_options()),
        ("/$defs/tool_gem", tool_gem()),
        ("/$defs/tool_go_options", tool_go_options()),
        ("/$defs/tool_go", tool_go()),
        ("/$defs/tool_conda_options", tool_conda_options()),
        ("/$defs/tool_conda", tool_conda()),
        ("/$defs/tool_spm_options", tool_spm_options()),
        ("/$defs/tool_spm_platforms", tool_spm_platforms()),
        ("/$defs/tool_spm", tool_spm()),
        ("/$defs/tool_packslip_options", tool_packslip_options()),
        ("/$defs/tool_packslip", tool_packslip()),
        ("/$defs/tool_spinel_options", tool_spinel_options()),
        ("/$defs/tool_spinel", tool_spinel()),
        (
            "/$defs/tool_core_python_options",
            tool_core_python_options(),
        ),
        ("/$defs/tool_core_python", tool_core_python()),
        ("/$defs/tool_core_java_options", tool_core_java_options()),
        ("/$defs/tool_core_java", tool_core_java()),
        ("/$defs/tool_core_rust_options", tool_core_rust_options()),
        ("/$defs/tool_core_rust", tool_core_rust()),
        (
            "/$defs/tool_core_dotnet_options",
            tool_core_dotnet_options(),
        ),
        ("/$defs/tool_core_dotnet", tool_core_dotnet()),
        ("/$defs/tool_options", tool_options()),
    ]
}

fn tools() -> Schema {
    json_schema!({
        "patternProperties": {
            "^github:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_github"}},
                    {"$ref": "#/$defs/tool_github"}
                ]
            },
            "^gitlab:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_gitlab"}},
                    {"$ref": "#/$defs/tool_gitlab"}
                ]
            },
            "^forgejo:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_forgejo"}},
                    {"$ref": "#/$defs/tool_forgejo"}
                ]
            },
            "^ubi:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_ubi"}},
                    {"$ref": "#/$defs/tool_ubi"}
                ]
            },
            "^http:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_http"}},
                    {"$ref": "#/$defs/tool_http"}
                ]
            },
            "^s3:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_s3"}},
                    {"$ref": "#/$defs/tool_s3"}
                ]
            },
            "^aqua:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_aqua"}},
                    {"$ref": "#/$defs/tool_aqua"}
                ]
            },
            "^cargo:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_cargo"}},
                    {"$ref": "#/$defs/tool_cargo"}
                ]
            },
            "^npm:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_npm"}},
                    {"$ref": "#/$defs/tool_npm"}
                ]
            },
            "^(pypi|pipx):": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_pypi"}},
                    {"$ref": "#/$defs/tool_pypi"}
                ]
            },
            "^gem:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_gem"}},
                    {"$ref": "#/$defs/tool_gem"}
                ]
            },
            "^go:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_go"}},
                    {"$ref": "#/$defs/tool_go"}
                ]
            },
            "^conda:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_conda"}},
                    {"$ref": "#/$defs/tool_conda"}
                ]
            },
            "^spm:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_spm"}},
                    {"$ref": "#/$defs/tool_spm"}
                ]
            },
            "^packslip:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_packslip"}},
                    {"$ref": "#/$defs/tool_packslip"}
                ]
            },
            "^spinel:": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_spinel"}},
                    {"$ref": "#/$defs/tool_spinel"}
                ]
            },
            "^(core:)?python(\\[.*\\])?$": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_core_python"}},
                    {"$ref": "#/$defs/tool_core_python"}
                ]
            },
            "^(core:)?java(\\[.*\\])?$": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_core_java"}},
                    {"$ref": "#/$defs/tool_core_java"}
                ]
            },
            "^(core:)?rust(\\[.*\\])?$": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_core_rust"}},
                    {"$ref": "#/$defs/tool_core_rust"}
                ]
            },
            "^(core:dotnet|dotnet|dotnet-core)(\\[.*\\])?$": {
                "oneOf": [
                    {"type": "array", "items": {"$ref": "#/$defs/tool_core_dotnet"}},
                    {"$ref": "#/$defs/tool_core_dotnet"}
                ]
            }
        },
        "additionalProperties": {
            "oneOf": [{"items": {"$ref": "#/$defs/tool"}, "type": "array"}, {"$ref": "#/$defs/tool"}]
        },
        "description": "dev tools to use",
        "type": "object"
    })
}

fn tool() -> Schema {
    json_schema!({
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
                                            "run": {"type": "string"},
                                            "when": {
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
        "additionalProperties": {"type": "object"}
    })
}

fn tool_forge_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "version_prefix": {
                "description": "release tag prefix stripped from versions and re-added when resolving tags",
                "type": "string"
            },
            "asset_pattern": {
                "description": "glob (* and ?) matched against release asset names, tera templated; replaces autodetection",
                "type": "string"
            },
            "additional_asset_patterns": {
                "description": "extra archives from the same release extracted over the primary asset, in order",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "matching": {
                "description": "substring an asset name must contain, applied before platform autodetection",
                "type": "string"
            },
            "matching_regex": {
                "description": "regex an asset name must match, applied before platform autodetection",
                "type": "string"
            },
            "no_app": {
                "description": "penalize macos .app bundle assets during autodetection",
                "$ref": "#/$defs/tool_bool"
            },
            "filter_bins": {
                "description": "only expose these binaries on PATH",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "format": {
                "description": "force the archive format instead of detecting it from the file name",
                "type": "string",
                "enum": [
                    "tar.gz",
                    "tgz",
                    "gz",
                    "tar.xz",
                    "txz",
                    "xz",
                    "tar.bz2",
                    "tbz2",
                    "tbz",
                    "bz2",
                    "tar.zst",
                    "tzst",
                    "zst",
                    "tar",
                    "zip",
                    "vsix",
                    "7z",
                    "tar.br",
                    "tbr",
                    "br",
                    "tar.lz4",
                    "tlz4",
                    "lz4",
                    "tar.sz",
                    "tsz",
                    "sz",
                    "rar",
                    "raw"
                ]
            },
            "checksum": {
                "description": "expected artifact digest as algo:hash (sha256, sha512, sha1, md5, blake3)",
                "type": "string"
            },
            "size": {
                "description": "expected artifact size in bytes",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "strip_components": {
                "description": "leading directory components to strip on extract (auto 1 for single-root archives)",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "bin": {
                "description": "executable name (or safe relative path) to install the downloaded binary as",
                "type": "string"
            },
            "rename_exe": {
                "description": "rename the extracted executable: a name, or a table of glob = new name",
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "additionalProperties": {"type": "string"}}
                ]
            },
            "bin_path": {
                "description": "directory inside the install that contains binaries (tera templated)",
                "type": "string"
            }
        }
    })
}

fn tool_github_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "api_url": {
                "description": "api base url, for github enterprise or self-hosted instances",
                "type": "string"
            },
            "github_attestations": {
                "description": "verify github artifact attestations when available",
                "$ref": "#/$defs/tool_bool"
            },
            "slsa_signer_identity": {
                "description": "expected fulcio certificate identity for slsa provenance, tera templated",
                "type": "string"
            },
            "slsa_signer_issuer": {
                "description": "expected oidc issuer for slsa provenance; requires slsa_signer_identity",
                "type": "string"
            }
        }
    })
}

fn tool_forge_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {
                "asset_pattern": {"$ref": "#/$defs/tool_forge_options/properties/asset_pattern"},
                "additional_asset_patterns": {"$ref": "#/$defs/tool_forge_options/properties/additional_asset_patterns"},
                "url": {
                    "description": "direct download url for this platform, tera templated; skips release asset selection",
                    "type": "string"
                },
                "no_app": {"$ref": "#/$defs/tool_forge_options/properties/no_app"},
                "filter_bins": {"$ref": "#/$defs/tool_forge_options/properties/filter_bins"},
                "checksum": {"$ref": "#/$defs/tool_forge_options/properties/checksum"},
                "size": {"$ref": "#/$defs/tool_forge_options/properties/size"},
                "strip_components": {"$ref": "#/$defs/tool_forge_options/properties/strip_components"},
                "bin": {"$ref": "#/$defs/tool_forge_options/properties/bin"},
                "rename_exe": {"$ref": "#/$defs/tool_forge_options/properties/rename_exe"},
                "bin_path": {"$ref": "#/$defs/tool_forge_options/properties/bin_path"},
                "format": {"$ref": "#/$defs/tool_forge_options/properties/format"}
            }
        }
    })
}

fn tool_github() -> Schema {
    json_schema!({
        "description": "a tool installed with the github backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_forge_options"},
                    {"$ref": "#/$defs/tool_github_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_forge_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_forge_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_gitlab_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "api_url": {"description": "api base url, for self-hosted gitlab instances", "type": "string"}
        }
    })
}

fn tool_gitlab() -> Schema {
    json_schema!({
        "description": "a tool installed with the gitlab backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_forge_options"},
                    {"$ref": "#/$defs/tool_gitlab_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_forge_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_forge_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_forgejo_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "api_url": {
                "description": "api base url, for self-hosted forgejo or gitea instances",
                "type": "string"
            }
        }
    })
}

fn tool_forgejo() -> Schema {
    json_schema!({
        "description": "a tool installed with the forgejo backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_forge_options"},
                    {"$ref": "#/$defs/tool_forgejo_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_forge_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_forge_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_ubi_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "exe": {
                "description": "name of the executable inside the archive to extract",
                "type": "string"
            },
            "rename_exe": {"description": "name to give the extracted executable", "type": "string"},
            "matching": {
                "description": "substring to match release filenames when several match the os and arch",
                "type": "string"
            },
            "matching_regex": {
                "description": "regex release filenames must match before os and arch matching",
                "type": "string"
            },
            "provider": {
                "description": "forge used to fetch releases and assets",
                "type": "string",
                "enum": ["github", "gitlab"]
            },
            "api_url": {
                "description": "provider api base url, for self-hosted instances",
                "type": "string"
            },
            "tag_regex": {
                "description": "regex release tags must match to be listed as versions",
                "type": "string"
            },
            "extract_all": {
                "description": "extract every file in the archive instead of only the binary",
                "$ref": "#/$defs/tool_bool"
            },
            "bin_path": {
                "description": "directory in the install containing binaries, used with extract_all",
                "type": "string"
            }
        }
    })
}

fn tool_ubi_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {"bin_path": {"$ref": "#/$defs/tool_ubi_options/properties/bin_path"}}
        }
    })
}

fn tool_ubi() -> Schema {
    json_schema!({
        "description": "a tool installed with the ubi backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_ubi_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_ubi_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_ubi_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_http_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "url": {
                "description": "download url, tera templated, http(s) or file://; required for each platform",
                "type": "string"
            },
            "checksum": {"description": "expected artifact digest as algo:hash", "type": "string"},
            "checksum_url": {
                "description": "checksum file or manifest url, used by mise lock without downloading",
                "type": "string"
            },
            "checksum_expr": {
                "description": "expr-lang expression extracting algo:hash from the checksum_url manifest",
                "type": "string"
            },
            "headers": {
                "description": "request headers sent with the download, version_list_url and checksum_url requests; values are tera templated (e.g. Bearer {{ env.TOKEN }})",
                "type": "object",
                "additionalProperties": {"type": "string"}
            },
            "headers_forward": {
                "description": "hosts each header in headers may follow a redirect to; exact hosts or *.suffix, and a header with no entry is never forwarded",
                "type": "object",
                "additionalProperties": {"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]}
            },
            "size": {
                "description": "expected artifact size in bytes",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "format": {
                "description": "force the archive format instead of detecting it from the url",
                "type": "string",
                "enum": [
                    "tar.gz",
                    "tgz",
                    "gz",
                    "tar.xz",
                    "txz",
                    "xz",
                    "tar.bz2",
                    "tbz2",
                    "tbz",
                    "bz2",
                    "tar.zst",
                    "tzst",
                    "zst",
                    "tar",
                    "zip",
                    "vsix",
                    "7z",
                    "tar.br",
                    "tbr",
                    "br",
                    "tar.lz4",
                    "tlz4",
                    "lz4",
                    "tar.sz",
                    "tsz",
                    "sz",
                    "rar",
                    "raw"
                ]
            },
            "strip_components": {
                "description": "leading directory components to strip on extract (auto 1 for single-root archives)",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "bin": {
                "description": "name for a downloaded raw or compressed single binary (ignored for archives)",
                "type": "string"
            },
            "rename_exe": {
                "description": "rename the extracted executable: a name, or a table of glob = new name",
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "additionalProperties": {"type": "string"}}
                ]
            },
            "bin_path": {
                "description": "directory inside the install that contains binaries (tera templated)",
                "type": "string"
            },
            "shared_extraction": {
                "description": "share extracted content between installs with identical artifacts",
                "$ref": "#/$defs/tool_bool"
            },
            "windows_script_interpreter": {
                "description": "windows only: write a .cmd launcher running the raw script with this interpreter",
                "type": "string"
            },
            "version_list_url": {
                "description": "url returning the version list (text, json or html)",
                "type": "string"
            },
            "version_regex": {
                "description": "regex extracting versions from the list (first capture group or whole match)",
                "type": "string"
            },
            "version_json_path": {
                "description": "jq-like path extracting versions from json, such as .[].version",
                "type": "string"
            },
            "version_expr": {
                "description": "expr-lang expression post-processing versions (variables: body, versions)",
                "type": "string"
            }
        }
    })
}

fn tool_http_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {
                "url": {"$ref": "#/$defs/tool_http_options/properties/url"},
                "headers": {"$ref": "#/$defs/tool_http_options/properties/headers"},
                "headers_forward": {"$ref": "#/$defs/tool_http_options/properties/headers_forward"},
                "checksum": {"$ref": "#/$defs/tool_http_options/properties/checksum"},
                "checksum_url": {"$ref": "#/$defs/tool_http_options/properties/checksum_url"},
                "size": {"$ref": "#/$defs/tool_http_options/properties/size"},
                "format": {"$ref": "#/$defs/tool_http_options/properties/format"},
                "strip_components": {"$ref": "#/$defs/tool_http_options/properties/strip_components"},
                "bin": {"$ref": "#/$defs/tool_http_options/properties/bin"},
                "rename_exe": {"$ref": "#/$defs/tool_http_options/properties/rename_exe"},
                "bin_path": {"$ref": "#/$defs/tool_http_options/properties/bin_path"},
                "windows_script_interpreter": {"$ref": "#/$defs/tool_http_options/properties/windows_script_interpreter"}
            }
        }
    })
}

fn tool_http() -> Schema {
    json_schema!({
        "description": "a tool installed with the http backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_http_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_http_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_http_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_s3_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "url": {
                "description": "s3://bucket/key object url, tera templated; required for each platform",
                "type": "string"
            },
            "checksum": {"description": "expected artifact digest as algo:hash", "type": "string"},
            "size": {
                "description": "expected artifact size in bytes",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "format": {
                "description": "force the archive format when the object key has no usable extension",
                "type": "string",
                "enum": [
                    "tar.gz",
                    "tgz",
                    "gz",
                    "tar.xz",
                    "txz",
                    "xz",
                    "tar.bz2",
                    "tbz2",
                    "tbz",
                    "bz2",
                    "tar.zst",
                    "tzst",
                    "zst",
                    "tar",
                    "zip",
                    "vsix",
                    "7z",
                    "tar.br",
                    "tbr",
                    "br",
                    "tar.lz4",
                    "tlz4",
                    "lz4",
                    "tar.sz",
                    "tsz",
                    "sz",
                    "rar",
                    "raw"
                ]
            },
            "strip_components": {
                "description": "leading directory components to strip on extract (auto 1 for single-root archives)",
                "oneOf": [{"type": "integer"}, {"type": "string"}]
            },
            "bin": {
                "description": "executable name (or safe relative path) to install the downloaded binary as",
                "type": "string"
            },
            "rename_exe": {
                "description": "rename the extracted executable: a name, or a table of glob = new name",
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "additionalProperties": {"type": "string"}}
                ]
            },
            "bin_path": {
                "description": "directory inside the install that contains binaries (tera templated)",
                "type": "string"
            },
            "region": {"description": "aws region used for the s3 client", "type": "string"},
            "endpoint": {
                "description": "custom s3-compatible endpoint url (minio and similar); enables path-style addressing",
                "type": "string"
            },
            "version_list_url": {"description": "s3:// url of a manifest file listing versions", "type": "string"},
            "version_prefix": {
                "description": "s3 key prefix to list objects under for version discovery",
                "type": "string"
            },
            "version_regex": {
                "description": "regex extracting versions from the manifest or object keys",
                "type": "string"
            },
            "version_json_path": {
                "description": "jq-like path extracting versions from json, such as .[].version",
                "type": "string"
            },
            "version_expr": {
                "description": "expr-lang expression post-processing versions (variables: body, versions)",
                "type": "string"
            }
        }
    })
}

fn tool_s3_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {
                "url": {"$ref": "#/$defs/tool_s3_options/properties/url"},
                "checksum": {"$ref": "#/$defs/tool_s3_options/properties/checksum"},
                "size": {"$ref": "#/$defs/tool_s3_options/properties/size"},
                "format": {"$ref": "#/$defs/tool_s3_options/properties/format"},
                "strip_components": {"$ref": "#/$defs/tool_s3_options/properties/strip_components"},
                "bin": {"$ref": "#/$defs/tool_s3_options/properties/bin"},
                "rename_exe": {"$ref": "#/$defs/tool_s3_options/properties/rename_exe"},
                "bin_path": {"$ref": "#/$defs/tool_s3_options/properties/bin_path"},
                "region": {"$ref": "#/$defs/tool_s3_options/properties/region"},
                "endpoint": {"$ref": "#/$defs/tool_s3_options/properties/endpoint"},
                "version_list_url": {"$ref": "#/$defs/tool_s3_options/properties/version_list_url"},
                "version_prefix": {"$ref": "#/$defs/tool_s3_options/properties/version_prefix"},
                "version_regex": {"$ref": "#/$defs/tool_s3_options/properties/version_regex"},
                "version_json_path": {"$ref": "#/$defs/tool_s3_options/properties/version_json_path"},
                "version_expr": {"$ref": "#/$defs/tool_s3_options/properties/version_expr"}
            }
        }
    })
}

fn tool_s3() -> Schema {
    json_schema!({
        "description": "a tool installed with the s3 backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_s3_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_s3_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_s3_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_aqua_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "symlink_bins": {
                "description": "expose only the registry-declared binaries through a filtered bin directory",
                "$ref": "#/$defs/tool_bool"
            },
            "libc": {
                "description": "force strict glibc or musl asset selection on linux",
                "type": "string",
                "enum": ["glibc", "gnu", "musl"]
            },
            "slsa_signer_identity": {
                "description": "expected fulcio certificate subject for slsa provenance; requires slsa_signer_issuer",
                "type": "string"
            },
            "slsa_signer_issuer": {
                "description": "expected oidc issuer for slsa provenance; requires slsa_signer_identity",
                "type": "string"
            },
            "vars": {
                "description": "values for aqua registry template variables",
                "type": "object",
                "additionalProperties": {"type": "string"}
            }
        }
    })
}

fn tool_aqua() -> Schema {
    json_schema!({
        "description": "a tool installed with the aqua backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_aqua_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_cargo_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "features": {
                "description": "crate features to enable (cargo install --features); skips cargo-binstall",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "default-features": {
                "description": "set to false to pass --no-default-features; skips cargo-binstall",
                "$ref": "#/$defs/tool_bool"
            },
            "bin": {
                "description": "binary to install when a crate has several (cargo install --bin)",
                "type": "string"
            },
            "rename_exe": {
                "description": "rename the installed executable: a name, or a table of glob = new name",
                "oneOf": [
                    {"type": "string"},
                    {"type": "object", "additionalProperties": {"type": "string"}}
                ]
            },
            "crate": {
                "description": "crate to install when a git repository contains several",
                "type": "string"
            },
            "locked": {
                "description": "pass --locked to cargo install (default true)",
                "$ref": "#/$defs/tool_bool"
            }
        }
    })
}

fn tool_cargo_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {"bin": {"$ref": "#/$defs/tool_cargo_options/properties/bin"}}
        }
    })
}

fn tool_cargo() -> Schema {
    json_schema!({
        "description": "a tool installed with the cargo backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_cargo_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_cargo_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_cargo_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_npm_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "allow_builds": {
                "description": "dependency packages whose build scripts are approved; true approves all",
                "oneOf": [
                    {"type": "boolean"},
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "trust_policy_excludes": {
                "description": "packages or version ranges exempt from aube's trustPolicy no-downgrade check",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "allow_low_downloads": {
                "description": "approve the requested package past aube's reputation checks",
                "$ref": "#/$defs/tool_bool_text"
            },
            "allow_exotic_deps": {
                "description": "dependencies allowed from non-registry sources (git, file, tarball url); true allows all",
                "oneOf": [
                    {"type": "boolean"},
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "aube_args": {
                "description": "extra arguments for aube add --global (only with npm.package_manager = aube_cli)",
                "type": "string"
            },
            "pnpm_args": {"description": "extra arguments for pnpm installs (shell-split)", "type": "string"},
            "bun_args": {"description": "extra arguments for bun installs (shell-split)", "type": "string"},
            "npm_args": {"description": "extra arguments for npm installs (shell-split)", "type": "string"}
        }
    })
}

fn tool_npm() -> Schema {
    json_schema!({
        "description": "a tool installed with the npm backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_npm_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_pypi_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "extras": {
                "description": "python package extras to install",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "with": {
                "description": "additional python requirements installed into the tool environment (requires uv)",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "expose": {
                "description": "extra requirements whose executables are exposed (requires uv 0.8.5+)",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "dependency_prereleases": {
                "description": "uv prerelease policy for dependency resolution (requires uv)",
                "type": "string",
                "enum": ["disallow", "allow", "if-necessary", "explicit"]
            },
            "python": {
                "description": "mise-managed CPython version for this tool (minor or exact patch)",
                "type": "string"
            },
            "package_name": {
                "description": "distribution name for git sources, used to attach extras",
                "type": "string"
            },
            "pipx_args": {"description": "extra arguments for pipx install (shell-split)", "type": "string"},
            "uvx": {
                "description": "set to false to install with pipx instead of uv",
                "$ref": "#/$defs/tool_bool"
            },
            "uvx_args": {
                "description": "extra arguments for uv tool install (shell-split)",
                "type": "string"
            },
            "registry_url": {
                "description": "version-listing url containing a {} placeholder for the package name",
                "type": "string"
            }
        }
    })
}

fn tool_pypi() -> Schema {
    json_schema!({
        "description": "a tool installed with the pypi backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_pypi_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_gem_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "source": {
                "description": "gem registry http(s) url used to resolve and install this gem",
                "type": "string"
            }
        }
    })
}

fn tool_gem() -> Schema {
    json_schema!({
        "description": "a tool installed with the gem backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_gem_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_go_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "tags": {
                "description": "build tags passed to go install -tags",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            }
        }
    })
}

fn tool_go() -> Schema {
    json_schema!({
        "description": "a tool installed with the go backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_go_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_conda_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "channel": {
                "description": "conda channel to resolve the package from (default conda-forge)",
                "type": "string"
            }
        }
    })
}

fn tool_conda() -> Schema {
    json_schema!({
        "description": "a tool installed with the conda backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_conda_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_spm_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "provider": {
                "description": "git host used for release info and assets",
                "type": "string",
                "enum": ["github", "gitlab"]
            },
            "api_url": {"description": "provider api url, for self-hosted instances", "type": "string"},
            "artifactbundle": {
                "description": "true requires an artifactbundle, false forces a source build, unset tries the bundle first",
                "$ref": "#/$defs/tool_bool"
            },
            "artifactbundle_asset": {
                "description": "specific *.artifactbundle.zip release asset to use",
                "type": "string"
            },
            "filter_bins": {
                "description": "executable products to install",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "install_command": {
                "description": "command run in the checkout instead of swift build; incompatible with filter_bins",
                "type": "string"
            }
        }
    })
}

fn tool_spm_platforms() -> Schema {
    json_schema!({
        "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
        "type": "object",
        "propertyNames": {"pattern": "^[A-Za-z]+-[A-Za-z0-9_]+$"},
        "additionalProperties": {
            "type": "object",
            "properties": {
                "provider": {"$ref": "#/$defs/tool_spm_options/properties/provider"},
                "api_url": {"$ref": "#/$defs/tool_spm_options/properties/api_url"},
                "artifactbundle": {"$ref": "#/$defs/tool_spm_options/properties/artifactbundle"},
                "artifactbundle_asset": {"$ref": "#/$defs/tool_spm_options/properties/artifactbundle_asset"}
            }
        }
    })
}

fn tool_spm() -> Schema {
    json_schema!({
        "description": "a tool installed with the spm backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [
                    {"$ref": "#/$defs/tool"},
                    {"$ref": "#/$defs/tool_spm_options"},
                    {
                        "properties": {
                            "platforms": {
                                "description": "per-platform option overrides",
                                "$ref": "#/$defs/tool_spm_platforms"
                            },
                            "platform": {
                                "description": "per-platform option overrides (alias of platforms)",
                                "$ref": "#/$defs/tool_spm_platforms"
                            }
                        }
                    }
                ],
                "type": "object"
            }
        ]
    })
}

fn tool_packslip_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "variant": {
                "description": "publisher-declared alternative build to select, such as fips",
                "type": "string"
            },
            "pubkey": {
                "description": "minisign public key line or path to a .pub file pinning the signer",
                "type": "string"
            },
            "identity": {"description": "exact keyless signer certificate identity", "type": "string"},
            "identity_prefix": {"description": "keyless signer certificate identity prefix", "type": "string"},
            "workflow": {
                "description": "repository workflow file, or a list of them, that signs releases on a tag (or on the ref a name ends with, as in release.yml@refs/heads/main); github.com projects only, not valid with pubkey, identity, identity_prefix, or issuer",
                "oneOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}, "minItems": 1}
                ]
            },
            "list_identity_prefix": {
                "description": "separate identity prefix for the vendor release list; needs an issuer, not valid with pubkey",
                "type": "string"
            },
            "issuer": {"description": "oidc issuer for the keyless signer", "type": "string"},
            "allow_unlogged": {
                "description": "accept key-signed bundles without transparency-log evidence",
                "$ref": "#/$defs/tool_bool"
            },
            "ignore_requirements": {
                "description": "install despite confirmed host requirement failures",
                "$ref": "#/$defs/tool_bool"
            },
            "trust": {
                "description": "vendor exempts this tool from configured packslip stampers",
                "type": "string",
                "enum": ["vendor"]
            }
        }
    })
}

fn tool_packslip() -> Schema {
    json_schema!({
        "description": "a tool installed with the packslip backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_packslip_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_spinel_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "entrypoint": {
                "description": "ruby file to compile, relative to the repository root (default main.rb)",
                "type": "string"
            },
            "bin": {
                "description": "name of the executable written to bin/ (default: the repository name)",
                "type": "string"
            },
            "tag_prefix": {
                "description": "text before the version in each git tag, such as v",
                "type": "string"
            },
            "source_ref": {
                "description": "full 40-character commit SHA to build instead of the tag",
                "type": "string",
                "pattern": "^[0-9a-fA-F]{40}$"
            },
            "spinel": {
                "description": "spinel compiler to run (default: spinel on PATH)",
                "type": "string"
            }
        }
    })
}

fn tool_spinel() -> Schema {
    json_schema!({
        "description": "a tool installed with the spinel backend",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_spinel_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_core_python_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "patch_sysconfig": {
                "description": "patch sysconfig paths of precompiled python after install (default true, unix only)",
                "$ref": "#/$defs/tool_bool"
            },
            "virtualenv": {
                "description": "virtualenv path to activate; use env._.python.venv instead",
                "type": "string",
                "deprecated": true
            }
        }
    })
}

fn tool_core_python() -> Schema {
    json_schema!({
        "description": "the core python tool",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_core_python_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_core_java_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "release_type": {
                "description": "jdk metadata channel: ga (default) or ea for early access builds",
                "type": "string",
                "enum": ["ga", "ea"]
            }
        }
    })
}

fn tool_core_java() -> Schema {
    json_schema!({
        "description": "the core java tool",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_core_java_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_core_rust_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "profile": {
                "description": "rustup profile to install, such as minimal, default or complete",
                "type": "string"
            },
            "components": {
                "description": "rustup components to install",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "targets": {
                "description": "rustup targets to install, for cross-compilation",
                "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
            },
            "mr_boxington": {
                "description": "wrap cargo with mr boxington (mbx); needs mr-boxington in tools (default false)",
                "$ref": "#/$defs/tool_bool"
            }
        }
    })
}

fn tool_core_rust() -> Schema {
    json_schema!({
        "description": "the core rust tool",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_core_rust_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_core_dotnet_options() -> Schema {
    json_schema!({
        "type": "object",
        "properties": {
            "runtime": {
                "description": "install a shared runtime instead of the sdk; the version is the runtime version",
                "type": "string",
                "enum": ["dotnet", "aspnetcore", "windowsdesktop"]
            }
        }
    })
}

fn tool_core_dotnet() -> Schema {
    json_schema!({
        "description": "the core dotnet tool",
        "oneOf": [
            {"description": "version of the tool to install", "type": "string"},
            {
                "allOf": [{"$ref": "#/$defs/tool"}, {"$ref": "#/$defs/tool_core_dotnet_options"}],
                "type": "object"
            }
        ]
    })
}

fn tool_options() -> Schema {
    json_schema!({
        "properties": {
            "version": {"description": "version of the tool to install", "type": "string"},
            "path": {"description": "path to a custom compiled version", "type": "string"},
            "prefix": {"description": "version prefix to resolve", "type": "string"},
            "ref": {"description": "version control reference to compile", "type": "string"},
            "os": {"$ref": "#/$defs/os_filter"},
            "minimum_release_age": {
                "description": "minimum time since a version's release before mise selects it, as a duration such as \"7d\" or an absolute date",
                "type": "string"
            },
            "install_before": {
                "description": "deprecated alias for minimum_release_age",
                "deprecated": true,
                "type": "string"
            },
            "version_order": {
                "description": "ordering applied to remote versions: \"source\" keeps the backend's order, \"semver\" sorts semantic versions",
                "enum": ["source", "semver"]
            },
            "prerelease": {
                "description": "include pre-release versions when listing versions and resolving latest",
                "$ref": "#/$defs/tool_bool"
            },
            "platforms": {
                "description": "per-platform option overrides keyed by <os>-<arch>, such as linux-x64 or macos-arm64",
                "$ref": "#/$defs/tool_platforms"
            },
            "platform": {
                "description": "per-platform option overrides (alias of platforms)",
                "$ref": "#/$defs/tool_platforms"
            }
        },
        "oneOf": [
            {"required": ["version"]},
            {"required": ["path"]},
            {"required": ["prefix"]},
            {"required": ["ref"]}
        ],
        "type": "object"
    })
}
