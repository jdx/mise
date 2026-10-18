use mise_bootstrap::{ManagedFilePhase, ManagedState};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::schema::refs;

// The doc comments on these types are their descriptions in schema/mise.json, which editors show
// to users. Notes for Rust readers go in plain comments.

/// file whose content, owner, group, and mode mise manages, keyed by an absolute or ~/ target path
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
    /// file to copy the content from; a relative path resolves from the declaring config's directory and ~/ from your home directory
    pub source: Option<String>,
    /// inline file content; cannot be combined with source
    pub content: Option<String>,
    /// user that should own the file; left as it is when unset
    pub owner: Option<String>,
    /// group that should own the file; left as it is when unset
    pub group: Option<String>,
    /// octal permissions such as "0640"; defaults to "0644" for a file with source or content, otherwise left as it is
    pub mode: Option<String>,
    /// render source or content with mise's template engine; without it, template syntax is written unchanged
    #[serde(default)]
    pub template: bool,
    /// remove the target when the template renders to empty or whitespace-only content; requires template = true
    #[serde(default)]
    pub remove_empty: bool,
    /// present creates or updates the file; absent removes it whatever its content
    #[serde(default)]
    #[schemars(schema_with = "managed_state")]
    pub state: ManagedState,
    /// replace a path of another type at the target, such as an empty directory; requires source or content
    #[serde(default)]
    pub replace: bool,
    /// system services declared in [bootstrap.services] to reload or restart when mise changes this file
    #[serde(default)]
    pub notify: Vec<String>,
    /// OS or OS/arch selectors; entries on other platforms are skipped
    #[serde(default, deserialize_with = "crate::packages::deserialize_package_os")]
    #[schemars(schema_with = "refs::os_filter")]
    pub os: Vec<String>,
}

/// directory whose owner, group, and mode mise manages, keyed by an absolute or ~/ target path
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema)]
pub struct ManagedDirectoryTomlConfig {
    #[serde(default)]
    #[schemars(schema_with = "refs::bootstrap_file_phase")]
    pub phase: ManagedFilePhase,
    /// user that should own the directory; left as it is when unset, and parents mise creates get the system defaults
    pub owner: Option<String>,
    /// group that should own the directory; left as it is when unset, and parents mise creates get the system defaults
    pub group: Option<String>,
    /// octal permissions such as "0755"; defaults to "0755"
    pub mode: Option<String>,
    /// present creates the directory with any missing parents; absent removes it, which must then be empty unless recursive is set
    #[serde(default)]
    #[schemars(schema_with = "managed_state")]
    pub state: ManagedState,
    /// with state = "absent", delete the directory and everything in it
    #[serde(default)]
    pub recursive: bool,
    /// replace a path of another type at the target, such as a file
    #[serde(default)]
    pub replace: bool,
    /// system services declared in [bootstrap.services] to reload or restart when mise changes this directory
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
