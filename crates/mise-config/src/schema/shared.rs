//! Definitions written by hand that several sections share.

use schemars::{Schema, json_schema};

/// Each definition, keyed by its name under `$defs` in `schema/mise.json`.
pub(crate) fn defs() -> Vec<(&'static str, Schema)> {
    vec![
        ("os_filter", os_filter()),
        ("os_filter_item", os_filter_item()),
    ]
}

fn os_filter() -> Schema {
    json_schema!({
        "description": "operating system filters to install on",
        "oneOf": [
            {"$ref": "#/$defs/os_filter_item"},
            {"type": "array", "items": {"$ref": "#/$defs/os_filter_item"}, "minItems": 1}
        ]
    })
}

fn os_filter_item() -> Schema {
    json_schema!({
        "description": "operating system or os/arch pair to install on",
        "type": "string",
        "pattern": "^[A-Za-z0-9_.+-]+(/[A-Za-z0-9_.+-]+)?$",
        "examples": ["linux", "macos", "windows", "unix", "macos/arm64", "linux/x64"]
    })
}
