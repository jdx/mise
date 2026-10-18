//! The parts of the `[dotfiles]` schema a derive cannot express, written by hand.
//!
//! The declaration types in `super` give each key its type and description. Which keys an entry
//! may combine depends on its mode and on whether it is a whole-file entry or an edit, so those
//! rules are written here and added to the derived schema. Change them together with
//! mise-system's dotfiles parsers, then run `mise run render:schema`.

use schemars::{Schema, SchemaGenerator, json_schema};
use serde_json::{Value, json};

/// Adds the rules between an entry's keys to its derived schema.
pub(crate) fn entry_rules(schema: &mut Schema) {
    let rules: Vec<Value> = vec![
        rule_0(),
        rule_1(),
        rule_2(),
        rule_3(),
        rule_4(),
        rule_5(),
        rule_6(),
        rule_7(),
        rule_8(),
    ];
    schema.insert("allOf".into(), rules.into());
    // an encrypted entry is a whole file from a source, never inline content or an edit
    schema.insert(
        "anyOf".into(),
        json!([
            {"properties": {"encrypt": {"const": false}}},
            {"properties": {"content": false, "block": false, "line": false, "template": false}}
        ]),
    );
}

/// A merge edits a structured file, so it takes none of the whole-file or line keys.
fn rule_0() -> Value {
    json!({
        "if": {"properties": {"merge": {"enum": [true, "missing"]}}, "required": ["merge"]},
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
    })
}

/// `relative` applies to symlink modes only.
fn rule_1() -> Value {
    json!({
        "anyOf": [
            {"properties": {"relative": {"const": false}}},
            {"properties": {"mode": {"enum": ["symlink", "symlink-each"]}}, "required": ["mode"]},
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
    })
}

/// `remove_empty` applies to templates only, and `absent` takes no source or content.
fn rule_2() -> Value {
    json!({
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
        "if": {"properties": {"mode": {"const": "absent"}}, "required": ["mode"]},
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
    })
}

/// `manifest` selects files from a directory source in copy or symlink-each mode.
fn rule_3() -> Value {
    json!({
        "oneOf": [
            {
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
                },
                "required": ["manifest"]
            },
            {"properties": {"manifest": false}}
        ]
    })
}

/// `dot_prefix` applies to directory walks only.
fn rule_4() -> Value {
    json!({
        "if": {"properties": {"dot_prefix": {"const": true}}, "required": ["dot_prefix"]},
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
    })
}

/// `permissions` applies to copy, template and inline content, or on its own to an existing target.
fn rule_5() -> Value {
    json!({
        "oneOf": [
            {
                "properties": {
                    "mode": {"enum": ["copy", "template"]},
                    "block": false,
                    "template": false,
                    "comment": false
                },
                "required": ["permissions"],
                "anyOf": [
                    {"required": ["source"]},
                    {"required": ["content"]},
                    {"required": ["mode"]},
                    {"properties": {"exclude": false, "manifest": false, "encrypt": false}}
                ]
            },
            {"properties": {"permissions": false}}
        ]
    })
}

/// `content`, `line` and the other keys exclude each other.
fn rule_6() -> Value {
    json!({
        "oneOf": [
            {
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
                },
                "required": ["content"]
            },
            {
                "properties": {
                    "content": false,
                    "permissions": false,
                    "mode": false,
                    "exclude": false,
                    "source": false,
                    "block": false,
                    "template": false,
                    "comment": false
                },
                "required": ["line"]
            },
            {"properties": {"content": false, "line": false, "position": false}}
        ]
    })
}

/// `include` applies to track modes only.
fn rule_7() -> Value {
    json!({
        "oneOf": [
            {
                "properties": {"mode": {"enum": ["track", "track-local"]}},
                "required": ["include", "mode"]
            },
            {"properties": {"include": false}}
        ]
    })
}

/// `allow_plaintext` applies to track modes only.
fn rule_8() -> Value {
    json!({
        "oneOf": [
            {
                "properties": {"mode": {"enum": ["track", "track-local"]}},
                "required": ["allow_plaintext", "mode"]
            },
            {"properties": {"allow_plaintext": false}}
        ]
    })
}

/// `merge`: `true`, or `"missing"`.
pub(crate) fn merge(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"oneOf": [{"type": "boolean"}, {"const": "missing"}]})
}

/// A variant's `os`: one name or a list of them.
pub(crate) fn one_or_many_strings(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"oneOf": [{"type": "string"}, {"type": "array", "items": {"type": "string"}}]})
}

/// A group entry written as a table, checked when mise expands the group.
pub(crate) fn any_table(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "object"})
}
