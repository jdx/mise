use std::path::PathBuf;

use mise_bootstrap::AccountState;
use schemars::JsonSchema;
use serde::Deserialize;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// local Linux group that bootstrap creates, updates, or removes
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[schemars(extend(
    "if" = {
        "required": ["state"],
        "properties": {"state": {"const": "absent"}},
        "additionalProperties": true
    },
    "then" = {
        "properties": {"gid": false, "system": {"const": false}},
        "additionalProperties": true
    }
))]
pub struct GroupTomlConfig {
    /// whether the group should exist
    #[serde(default)]
    #[schemars(schema_with = "account_state")]
    pub state: AccountState,
    /// numeric group ID
    #[schemars(range(max = 4294967295u32))]
    pub gid: Option<u32>,
    /// create a system group
    #[serde(default)]
    pub system: bool,
}

/// local Linux user that bootstrap creates, updates, or removes
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[schemars(extend("allOf" = [
    {
        "if": {
            "required": ["state"],
            "properties": {"state": {"const": "absent"}},
            "additionalProperties": true
        },
        "then": {
            "properties": {
                "uid": false,
                "group": false,
                "groups": false,
                "home": false,
                "shell": false,
                "comment": false,
                "create_home": false,
                "exclusive_groups": {"const": false},
                "system": {"const": false},
                "move_home": {"const": false}
            },
            "additionalProperties": true
        },
        "else": {
            "required": ["group"],
            "properties": {"remove_home": {"const": false}},
            "additionalProperties": true
        }
    },
    {
        "if": {
            "required": ["exclusive_groups"],
            "properties": {"exclusive_groups": {"const": true}},
            "additionalProperties": true
        },
        "then": {"required": ["groups"], "additionalProperties": true}
    },
    {
        "if": {
            "required": ["move_home"],
            "properties": {"move_home": {"const": true}},
            "additionalProperties": true
        },
        "then": {"required": ["home"], "additionalProperties": true}
    }
]))]
pub struct UserTomlConfig {
    /// whether the user should exist
    #[serde(default)]
    #[schemars(schema_with = "account_state")]
    pub state: AccountState,
    /// numeric user ID
    #[schemars(range(max = 4294967295u32))]
    pub uid: Option<u32>,
    /// primary group; required for a present user
    #[schemars(length(max = 32), regex(pattern = r"^[A-Za-z_][A-Za-z0-9_-]*\$?$"))]
    pub group: Option<String>,
    /// supplementary groups
    #[schemars(inner(length(max = 32), regex(pattern = r"^[A-Za-z_][A-Za-z0-9_-]*\$?$")))]
    pub groups: Option<Vec<String>>,
    /// remove the user from supplementary groups not listed in groups
    #[serde(default)]
    pub exclusive_groups: bool,
    /// home directory; an absolute path without ':', CR or LF
    #[schemars(regex(pattern = r"^/[^:\r\n]*$"))]
    pub home: Option<PathBuf>,
    /// login shell; an absolute path without ':', CR or LF
    #[schemars(regex(pattern = r"^/[^:\r\n]*$"))]
    pub shell: Option<PathBuf>,
    /// comment (GECOS) field, without ':', CR or LF
    #[schemars(regex(pattern = r"^[^:\r\n]*$"))]
    pub comment: Option<String>,
    /// create a system user
    #[serde(default)]
    pub system: bool,
    /// create the home directory
    pub create_home: Option<bool>,
    /// move the existing home directory to home
    #[serde(default)]
    pub move_home: bool,
    /// remove the home directory when removing the user
    #[serde(default)]
    pub remove_home: bool,
}

// `AccountState` lives in mise-bootstrap, which does not depend on schemars.
fn account_state(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"enum": ["present", "absent"], "default": "present"})
}
