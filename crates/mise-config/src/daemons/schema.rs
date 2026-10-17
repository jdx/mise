//! The JSON schema for `[daemons]` and `[daemon_providers]`, written by hand.
//!
//! A daemon definition and a provider stay raw tables that mise-daemons checks when it builds the
//! daemon, so there is no Rust type to derive them from. Change them together with mise-daemons,
//! then run `mise run render:schema`.

use schemars::{Schema, json_schema};

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![
        ("daemon_port", daemon_port()),
        ("daemon_proxy", daemon_proxy()),
        ("daemon_proxy_tls", daemon_proxy_tls()),
        ("daemon_proxy_idle_timeout", daemon_proxy_idle_timeout()),
        ("daemon_init", daemon_init()),
    ]
}

fn daemon_port() -> Schema {
    json_schema!({
        "description": "A fixed port, or \"auto\" to derive one from the project root so the primary checkout keeps the base port and each linked git worktree gets its own.",
        "oneOf": [
            {"type": "integer", "minimum": 1, "maximum": 65535},
            {
                "type": "string",
                "const": "auto",
                "description": "Offset the preset's default port by this project root's worktree slot."
            },
            {
                "type": "object",
                "required": ["auto"],
                "properties": {
                    "auto": {"const": true, "description": "Enable per-worktree port allocation."},
                    "base": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "Port used by the primary checkout. Defaults to the preset's port; required for custom daemons."
                    },
                    "stride": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "Distance between consecutive worktree slots. Defaults to 1."
                    }
                },
                "additionalProperties": false
            }
        ]
    })
}

fn daemon_proxy() -> Schema {
    json_schema!({
        "description": "Hostname label the reverse proxy routes to this daemon, false to opt out, or true to use the daemon's name. Only a daemon with a port is routed. The hostname is <proxy>.<worktree>.<project>.<tld> in a linked git worktree and <proxy>.<project>.<tld> in the primary checkout.",
        "oneOf": [
            {"type": "string", "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?$", "maxLength": 63},
            {
                "type": "boolean",
                "description": "false opts out of the reverse proxy; true uses the daemon's name, which re-enables routing for a preset that opts out."
            }
        ]
    })
}

fn daemon_proxy_tls() -> Schema {
    json_schema!({
        "description": "Whether the proxy terminates TLS for this daemon or passes the connection through to it.",
        "type": "string",
        "enum": ["terminate", "passthrough"]
    })
}

fn daemon_proxy_idle_timeout() -> Schema {
    json_schema!({
        "description": "Stop this daemon after this long without proxy traffic when the proxy started it, for example \"30m\". false exempts it from idle shutdown even when pitchfork's proxy.idle_timeout is set; \"0\" does the same. Idle shutdown is off unless this or pitchfork's proxy.idle_timeout is set, and a daemon started with `mise daemons start` is never stopped for inactivity. Requires pitchfork 2.27.0.",
        "oneOf": [{"type": "string", "pattern": "^\\s*[0-9]"}, {"type": "boolean", "enum": [false]}]
    })
}

fn daemon_init() -> Schema {
    json_schema!({
        "description": "idempotent setup commands run before the daemon's long-running process",
        "oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]
    })
}

pub(crate) fn daemons() -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {
            "oneOf": [
                {
                    "type": "string",
                    "description": "Version request for a preset matching the daemon name."
                },
                {
                    "type": "object",
                    "required": ["run"],
                    "not": {
                        "anyOf": [
                            {"required": ["preset"]},
                            {"required": ["project"]},
                            {"required": ["task"]},
                            {"required": ["args"]},
                            {"required": ["provider"]},
                            {"required": ["resource"]}
                        ]
                    },
                    "properties": {
                        "run": {"type": "string"},
                        "init": {"$ref": "#/$defs/daemon_init"},
                        "mise": {"type": "boolean"},
                        "port": {
                            "oneOf": [
                                {"$ref": "#/$defs/daemon_port"},
                                {
                                    "type": "object",
                                    "description": "Pitchfork's structured port configuration, forwarded verbatim.",
                                    "not": {"required": ["auto"]}
                                }
                            ]
                        },
                        "proxy": {"$ref": "#/$defs/daemon_proxy"},
                        "proxy_tls": {"$ref": "#/$defs/daemon_proxy_tls"},
                        "proxy_idle_timeout": {"$ref": "#/$defs/daemon_proxy_idle_timeout"}
                    },
                    "additionalProperties": true
                },
                {
                    "type": "object",
                    "required": ["task"],
                    "not": {
                        "anyOf": [
                            {"required": ["run"]},
                            {"required": ["preset"]},
                            {"required": ["project"]},
                            {"required": ["provider"]},
                            {"required": ["resource"]}
                        ]
                    },
                    "properties": {
                        "task": {
                            "description": "mise task run as this daemon's long-running process",
                            "type": "string"
                        },
                        "args": {
                            "description": "arguments passed to the task after `--`",
                            "type": "array",
                            "items": {"type": "string"}
                        },
                        "init": {"$ref": "#/$defs/daemon_init"},
                        "proxy": {"$ref": "#/$defs/daemon_proxy"},
                        "proxy_tls": {"$ref": "#/$defs/daemon_proxy_tls"},
                        "proxy_idle_timeout": {"$ref": "#/$defs/daemon_proxy_idle_timeout"}
                    },
                    "additionalProperties": true
                },
                {
                    "type": "object",
                    "required": ["preset", "version"],
                    "not": {
                        "anyOf": [
                            {"required": ["project"]},
                            {"required": ["task"]},
                            {"required": ["args"]},
                            {"required": ["provider"]},
                            {"required": ["resource"]}
                        ]
                    },
                    "properties": {
                        "data_dir": {
                            "type": "string",
                            "minLength": 1,
                            "description": "Persistent preset data directory, relative to the declaring project root or absolute. Defaults to mise state storage."
                        },
                        "preset": {
                            "type": "string",
                            "enum": ["cockroachdb", "nats", "postgres", "redis", "spicedb"]
                        },
                        "version": {"type": "string"},
                        "port": {"$ref": "#/$defs/daemon_port"},
                        "ports": {
                            "type": "object",
                            "description": "Overrides for the preset's additional named ports.",
                            "additionalProperties": {
                                "type": "integer",
                                "minimum": 1,
                                "maximum": 65535
                            }
                        },
                        "options": {"type": "object"},
                        "run": {"type": "string"},
                        "init": {"$ref": "#/$defs/daemon_init"},
                        "proxy": {"$ref": "#/$defs/daemon_proxy"},
                        "proxy_tls": {"$ref": "#/$defs/daemon_proxy_tls"},
                        "proxy_idle_timeout": {"$ref": "#/$defs/daemon_proxy_idle_timeout"}
                    },
                    "additionalProperties": true
                },
                {
                    "type": "object",
                    "description": "Daemon declared by another project, run under that project's namespace and root.",
                    "required": ["project"],
                    "properties": {
                        "project": {
                            "type": "string",
                            "description": "Directory of a project whose mise configuration declares the daemon. A relative path resolves against this project root."
                        },
                        "name": {
                            "type": "string",
                            "description": "Daemon name inside the referenced project, when it differs from this key."
                        }
                    },
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "required": ["provider"],
                    "additionalProperties": false,
                    "properties": {
                        "provider": {
                            "type": "string",
                            "description": "Name of a globally defined shared server provider."
                        },
                        "resource": {
                            "type": "string",
                            "pattern": "^[a-z][a-z0-9_]{0,62}$",
                            "description": "Database or NATS account shared by consumers choosing this name. Omit for a checkout-specific resource."
                        }
                    }
                }
            ]
        }
    })
}

pub(crate) fn daemon_providers() -> Schema {
    json_schema!({
        "type": "object",
        "propertyNames": {"pattern": "^[a-z0-9]+(?:-[a-z0-9]+)*$"},
        "additionalProperties": {
            "type": "object",
            "required": ["preset", "version"],
            "additionalProperties": false,
            "properties": {
                "preset": {"enum": ["postgres", "cockroachdb", "nats"]},
                "version": {"type": "string"},
                "port": {"$ref": "#/$defs/daemon_port"},
                "ports": {
                    "type": "object",
                    "additionalProperties": {"type": "integer", "minimum": 1, "maximum": 65535}
                },
                "options": {"type": "object"},
                "data_dir": {"type": "string", "minLength": 1},
                "tool": {"type": "string"}
            }
        }
    })
}
