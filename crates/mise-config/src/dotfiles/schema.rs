//! The JSON schema for `[dotfiles]` and `[dotfile_groups]`, written by hand.
//!
//! Each entry stays a TOML value until mise expands it, so a malformed one warns and is skipped
//! instead of failing the whole file, and there is no Rust type to derive the schema from. Change
//! it together with mise's dotfiles expansion, then run `mise run render:schema`.

use schemars::{Schema, json_schema};

pub(crate) fn dotfiles() -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {
            "oneOf": [
                {"type": "string", "description": "source path for a whole-file dotfile entry"},
                entry()
            ]
        }
    })
}

/// A table entry: a whole file, or an edit to part of one.
fn entry() -> Schema {
    json_schema!({
        "type": "object",
        "description": "whole-file dotfile entry or file edit entry",
        "properties": {
            "encrypt": {
                "type": "boolean",
                "default": false,
                "description": "encrypt tracked file contents before storing them in Git; the live file remains plaintext"
            },
            "allow_plaintext": {
                "type": "boolean",
                "default": false,
                "description": "allow this directly tracked credential-named file to be saved in plaintext history and any connected origin"
            },
            "source": {"type": "string", "description": "source file or directory"},
            "content": {
                "type": "string",
                "description": "literal inline file content; alternative to source"
            },
            "mode": {
                "type": "string",
                "description": "how to apply a whole-file source to the target; omitted uses dotfiles.default_mode. \"absent\" removes a file or symlink at the target and takes no source. \"track\" saves the file's history where it is, and \"track-local\" saves it in this machine's local-only history, which is never shared",
                "anyOf": [
                    {
                        "enum": [
                            "symlink",
                            "symlink-each",
                            "copy",
                            "template",
                            "absent",
                            "track",
                            "track-local"
                        ]
                    },
                    {"type": "string"}
                ]
            },
            "exclude": {
                "type": "array",
                "description": "glob patterns to skip when the mode walks a directory (symlink-each, copy with a directory source, or a tracked directory, where they are relative to the tracked path); a pattern without \"/\" matches any path component, one with \"/\" is anchored to the root",
                "items": {"type": "string"}
            },
            "include": {
                "type": "array",
                "description": "history only (mode = \"track\"): the only paths of the tracked directory that are captured, relative to it and matched like \"exclude\"; without it the whole tree is captured with the builtin credential filtering, and an explicit \"exclude\" still wins. A selected file is captured even when it looks like a credential store, in plaintext unless the entry sets encrypt",
                "items": {"type": "string"}
            },
            "manifest": {
                "type": "string",
                "description": "source manifest used to select managed files from a directory",
                "enum": ["git"]
            },
            "remove_empty": {
                "type": "boolean",
                "default": false,
                "description": "template mode only: remove the target when the template renders empty or whitespace-only; a target that changed since mise last wrote it needs --force"
            },
            "dot_prefix": {
                "type": "boolean",
                "default": false,
                "description": "symlink-each and directory copy only: deploy each source path component named \"dot-<name>\" as \".<name>\", like GNU Stow's --dotfiles; exclude still matches source names"
            },
            "relative": {
                "type": "boolean",
                "description": "symlink and symlink-each only: link by a path relative to the link's directory instead of an absolute one; overrides the dotfiles.relative_symlinks setting (ignored on Windows)"
            },
            "permissions": {
                "type": "string",
                "description": "octal permissions for the target, such as \"0600\"; overrides what copy, template, and inline content entries set, and on its own manages only the permissions of an existing target (Unix only)",
                "pattern": "^(0o)?[0-7]{1,4}$"
            },
            "block": {
                "type": "string",
                "description": "block content, maintained between marker comments"
            },
            "template": {
                "type": "string",
                "description": "template engine to render block content with; currently only \"tera\"",
                "anyOf": [{"enum": ["tera"]}, {"type": "string"}]
            },
            "line": {"type": "string", "description": "exact line to ensure exists"},
            "position": {
                "type": "string",
                "default": "append",
                "description": "where to insert a missing line",
                "anyOf": [{"enum": ["append", "prepend"]}, {"type": "string"}]
            },
            "comment": {
                "type": "string",
                "description": "comment prefix for edit marker lines; inferred from the file extension when omitted"
            },
            "merge": {
                "oneOf": [{"type": "boolean"}, {"const": "missing"}],
                "description": "edit entry: true sets the keys of source (default: the target's path under dotfiles.root) on a JSON, TOML, or YAML target and leaves the target's other keys to the application that also writes it; \"missing\" sets only the keys the target has no value for; the format comes from the target's extension"
            },
            "variants": {
                "type": "array",
                "description": "OS/profile variants: history streams for track mode, or optional destinations for deployment modes",
                "items": variant()
            }
        },
        "allOf": [
            {
                "if": {"required": ["merge"], "properties": {"merge": {"enum": [true, "missing"]}}},
                "then": {
                    "properties": {
                        "mode": false,
                        "content": false,
                        "permissions": false,
                        "exclude": false,
                        "include": false,
                        "manifest": false,
                        "autosave": false,
                        "encrypt": false,
                        "allow_plaintext": false,
                        "variants": false,
                        "enabled": false,
                        "remove_empty": false,
                        "relative": false,
                        "dot_prefix": false,
                        "group": false,
                        "block": false,
                        "line": false,
                        "position": false,
                        "comment": false
                    }
                }
            },
            {
                "anyOf": [
                    {"properties": {"relative": {"const": false}}},
                    {
                        "required": ["mode"],
                        "properties": {"mode": {"enum": ["symlink", "symlink-each"]}}
                    },
                    {
                        "properties": {
                            "mode": false,
                            "content": false,
                            "permissions": false,
                            "block": false,
                            "line": false
                        }
                    }
                ]
            },
            {
                "anyOf": [
                    {"properties": {"remove_empty": {"const": false}}},
                    {
                        "properties": {
                            "mode": {"const": "template"},
                            "content": false,
                            "block": false,
                            "line": false
                        },
                        "anyOf": [
                            {"required": ["source"]},
                            {"required": ["mode"]},
                            {"properties": {"permissions": false}}
                        ]
                    }
                ],
                "if": {"required": ["mode"], "properties": {"mode": {"const": "absent"}}},
                "then": {
                    "properties": {
                        "source": false,
                        "content": false,
                        "exclude": false,
                        "manifest": false,
                        "permissions": false,
                        "remove_empty": false,
                        "dot_prefix": {"const": false},
                        "encrypt": {"const": false},
                        "block": false,
                        "line": false,
                        "position": false,
                        "template": false,
                        "comment": false
                    }
                }
            },
            {
                "oneOf": [
                    {
                        "required": ["manifest"],
                        "properties": {
                            "source": {},
                            "content": false,
                            "permissions": false,
                            "mode": {"enum": ["copy", "symlink-each"]},
                            "exclude": {},
                            "manifest": {},
                            "block": false,
                            "line": false,
                            "template": false,
                            "comment": false
                        }
                    },
                    {"properties": {"manifest": false}}
                ]
            },
            {
                "if": {"required": ["dot_prefix"], "properties": {"dot_prefix": {"const": true}}},
                "then": {
                    "properties": {
                        "content": false,
                        "mode": {"enum": ["copy", "symlink-each"]},
                        "block": false,
                        "line": false,
                        "template": false,
                        "comment": false
                    },
                    "anyOf": [
                        {"required": ["source"]},
                        {"required": ["mode"]},
                        {"properties": {"permissions": false}}
                    ]
                }
            },
            {
                "oneOf": [
                    {
                        "required": ["permissions"],
                        "properties": {
                            "mode": {"enum": ["copy", "template"]},
                            "block": false,
                            "template": false,
                            "comment": false
                        },
                        "anyOf": [
                            {"required": ["source"]},
                            {"required": ["content"]},
                            {"required": ["mode"]},
                            {"properties": {"exclude": false, "manifest": false, "encrypt": false}}
                        ]
                    },
                    {"properties": {"permissions": false}}
                ]
            },
            {
                "oneOf": [
                    {
                        "required": ["content"],
                        "properties": {
                            "source": false,
                            "manifest": false,
                            "mode": false,
                            "exclude": false,
                            "block": false,
                            "line": false,
                            "position": false,
                            "template": false,
                            "comment": false
                        }
                    },
                    {
                        "required": ["line"],
                        "properties": {
                            "content": false,
                            "permissions": false,
                            "mode": false,
                            "exclude": false,
                            "source": false,
                            "block": false,
                            "template": false,
                            "comment": false
                        }
                    },
                    {"properties": {"content": false, "line": false, "position": false}}
                ]
            },
            {
                "oneOf": [
                    {
                        "required": ["include", "mode"],
                        "properties": {"mode": {"enum": ["track", "track-local"]}}
                    },
                    {"properties": {"include": false}}
                ]
            },
            {
                "oneOf": [
                    {
                        "required": ["allow_plaintext", "mode"],
                        "properties": {"mode": {"enum": ["track", "track-local"]}}
                    },
                    {"properties": {"allow_plaintext": false}}
                ]
            }
        ],
        "anyOf": [
            {"properties": {"encrypt": {"const": false}}},
            {"properties": {"content": false, "block": false, "line": false, "template": false}}
        ]
    })
}

/// One OS or profile variant of an entry.
fn variant() -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "os": {"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]},
            "profile": {
                "type": "string",
                "description": "mise environment selected with -E or MISE_ENV"
            },
            "default": {"type": "boolean"},
            "machine": {
                "type": "boolean",
                "description": "track mode only: a separate stream on every machine, never applied on another machine; must be the only variant and cannot be combined with encrypt"
            },
            "target": {
                "type": "string",
                "description": "deployment destination override; requires either an explicit source or a safe relative entry key when every variant has a target, and is not supported in track mode"
            }
        }
    })
}

pub(crate) fn dotfile_groups() -> Schema {
    json_schema!({
        "type": "object",
        "description": "named directory trees of dotfiles, applied with `mise dotfiles apply` or `mise bootstrap` while [bootstrap] dotfile_groups selects them (or is unset)",
        "propertyNames": {"pattern": "^[A-Za-z0-9_.-]+$"},
        "additionalProperties": {
            "type": "object",
            "additionalProperties": false,
            "required": ["root"],
            "properties": {
                "root": {
                    "type": "string",
                    "description": "the directory tree the group deploys; a relative path resolves against dotfiles.root"
                },
                "target": {
                    "type": "string",
                    "description": "target directory, absolute or starting with ~/",
                    "default": "~"
                },
                "mode": {
                    "type": "string",
                    "description": "how the tree is deployed",
                    "enum": ["symlink-each", "copy", "symlink"],
                    "default": "symlink-each"
                },
                "exclude": {
                    "type": "array",
                    "description": "glob patterns of source paths to skip, as for [dotfiles] exclude",
                    "items": {"type": "string"}
                },
                "dot_prefix": {
                    "type": "boolean",
                    "default": false,
                    "description": "deploy each source path component named \"dot-<name>\" as \".<name>\", like GNU Stow's --dotfiles"
                },
                "manifest": {
                    "type": "string",
                    "description": "source manifest used to select managed files from the tree",
                    "enum": ["git"]
                },
                "relative": {
                    "type": "boolean",
                    "description": "link by a path relative to the link's directory; overrides the dotfiles.relative_symlinks setting (ignored on Windows)"
                },
                "entries": {
                    "type": "object",
                    "description": "whole-file entries for parts of the tree, keyed by target path inside the group's target and written like [dotfiles] entries; each is cut out of the walk. A missing source is found under the root at the entry's path inside the target, and a relative source starts at the root",
                    "additionalProperties": {
                        "oneOf": [
                            {
                                "type": "string",
                                "description": "source path, relative to the group root"
                            },
                            {"type": "object", "description": "whole-file entry, as in [dotfiles]"}
                        ]
                    }
                }
            }
        }
    })
}
