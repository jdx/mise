use mise_bootstrap::{ManagedFilePhase, ManagedState};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::refs;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema)]
#[schemars(extend("allOf" = [
    {
        "if": {"required": ["remove_empty"], "properties": {"remove_empty": {"const": true}}},
        "then": {
            "required": ["template"],
            "properties": {"template": {"const": true}, "state": {"const": "present"}}
        }
    },
    {
        "$comment": "A present file without source or content manages only its permissions: it must declare some, and cannot be templated or replaced.",
        "if": {
            "properties": {"state": {"const": "present"}},
            "not": {"anyOf": [{"required": ["source"]}, {"required": ["content"]}]}
        },
        "then": {
            "anyOf": [{"required": ["mode"]}, {"required": ["owner"]}, {"required": ["group"]}],
            "properties": {"template": {"const": false}, "replace": {"const": false}}
        }
    }
]))]
pub struct ManagedFileTomlConfig {
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_file_phase")]
    pub phase: ManagedFilePhase,
    pub source: Option<String>,
    pub content: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    pub template: bool,
    /// remove the target when the template renders to empty or whitespace-only content; requires template = true
    #[serde(default)]
    pub remove_empty: bool,
    #[serde(default)]
    #[schemars(schema_with = "managed_state")]
    pub state: ManagedState,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub notify: Vec<String>,
    /// OS or OS/arch selectors; entries on other platforms are skipped
    #[serde(default, deserialize_with = "crate::packages::deserialize_package_os")]
    #[schemars(schema_with = "refs::os_filter")]
    pub os: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema)]
pub struct ManagedDirectoryTomlConfig {
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_file_phase")]
    pub phase: ManagedFilePhase,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "managed_state")]
    pub state: ManagedState,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub notify: Vec<String>,
    /// OS or OS/arch selectors; entries on other platforms are skipped
    #[serde(default, deserialize_with = "crate::packages::deserialize_package_os")]
    #[schemars(schema_with = "refs::os_filter")]
    pub os: Vec<String>,
}

// `ManagedState` lives in mise-bootstrap, which does not depend on schemars.
fn managed_state(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": "string", "enum": ["present", "absent"]})
}
