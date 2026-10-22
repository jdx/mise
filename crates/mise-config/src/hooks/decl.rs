//! The declaration types behind the schema for `[hooks]`.
//!
//! What an untagged [`HookDefItem`](super::HookDefItem) means depends on the event: a `script`
//! with a `shell` is evaluated in the active shell for `enter`, `leave` and `cd`, while the
//! install hooks ignore the `shell`. The parser reads one table for all of them
//! ([`HookTable`](super::HookTable)), so the shapes an event accepts are described here, and
//! nothing parses into these types. A test checks that they name exactly the keys the parser
//! reads. The doc comments are the descriptions in `schema/mise.json`; notes for Rust readers
//! go in plain comments.

use schemars::JsonSchema;

use super::HookScripts;
use crate::schema::refs;

/// inline hook command to run
#[derive(JsonSchema)]
pub struct HookRunString(pub String);

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    transform = crate::schema::unevaluated,
    extend("anyOf" = [{"required": ["run"]}, {"required": ["run_windows"]}])
)]
pub struct HookRunTableDecl {
    /// inline hook command to run
    pub run: Option<String>,
    /// inline hook command to run on Windows
    pub run_windows: Option<String>,
    /// inline shell command to use for run hooks, such as "bash -c"
    pub shell: Option<String>,
}

// Hooks that evaluate in the active shell: `enter`, `leave` and `cd`, when `shell` says which.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct HookCurrentShellScriptTable {
    /// script or scripts to evaluate in the active shell for enter/leave/cd hooks
    #[schemars(schema_with = "refs::hook_script_value")]
    pub script: HookScripts,
    /// active shell name for current-shell hooks, such as "bash"
    pub shell: String,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct HookCurrentShellScriptsTable {
    /// scripts to evaluate in the active shell for enter/leave/cd hooks
    #[schemars(schema_with = "refs::hook_scripts_value")]
    pub scripts: Vec<String>,
    /// active shell name for current-shell hooks, such as "bash"
    pub shell: String,
}

// The same keys without a `shell` run in a subprocess, which is deprecated in favor of `run`.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated, extend("deprecated" = true))]
pub struct HookSpawnedScriptTable {
    /// deprecated alias for run when an enter/leave/cd hook has no shell
    #[schemars(schema_with = "refs::hook_script_value", extend("deprecated" = true))]
    pub script: HookScripts,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated, extend("deprecated" = true))]
pub struct HookSpawnedScriptsTable {
    /// deprecated alias for run when an enter/leave/cd hook has no shell
    #[schemars(schema_with = "refs::hook_scripts_value", extend("deprecated" = true))]
    pub scripts: Vec<String>,
}

// `script` and `scripts` in a preinstall or postinstall hook, which ignore the `shell`.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated, extend("deprecated" = true))]
pub struct HookInstallScriptTable {
    /// deprecated alias for run in preinstall/postinstall hooks; executes in a subprocess
    #[schemars(schema_with = "refs::hook_script_value", extend("deprecated" = true))]
    pub script: HookScripts,
    /// ignored for preinstall/postinstall hooks that use script; use a run hook to select an inline shell
    #[schemars(extend("deprecated" = true))]
    pub shell: Option<String>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated, extend("deprecated" = true))]
pub struct HookInstallScriptsTable {
    /// deprecated alias for run in preinstall/postinstall hooks; joins the scripts and executes them in a subprocess
    #[schemars(schema_with = "refs::hook_scripts_value", extend("deprecated" = true))]
    pub scripts: Vec<String>,
    /// ignored for preinstall/postinstall hooks that use scripts; use a run hook to select an inline shell
    #[schemars(extend("deprecated" = true))]
    pub shell: Option<String>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct HookTaskRef {
    /// mise task to run
    pub task: String,
}

// One hook of an `enter`, `leave` or `cd` event.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum HookDefinitionItem {
    RunString(#[schemars(schema_with = "refs::hook_run_string")] ()),
    RunTable(#[schemars(schema_with = "refs::hook_run_table")] ()),
    CurrentShellScript(#[schemars(schema_with = "refs::hook_current_shell_script_table")] ()),
    CurrentShellScripts(#[schemars(schema_with = "refs::hook_current_shell_scripts_table")] ()),
    SpawnedScript(#[schemars(schema_with = "refs::hook_spawned_script_table")] ()),
    SpawnedScripts(#[schemars(schema_with = "refs::hook_spawned_scripts_table")] ()),
    Task(#[schemars(schema_with = "refs::hook_task_ref")] ()),
}

// An `enter`, `leave` or `cd` hook: one, or a list of them.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum HookDefinition {
    One(#[schemars(schema_with = "refs::hook_definition_item")] ()),
    /// scripts/tasks to run
    Many(#[schemars(schema_with = "hook_definition_items")] ()),
}

fn hook_definition_items(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"items": {"$ref": "#/$defs/hook_definition_item"}, "type": "array"})
}

// One hook of a `preinstall` or `postinstall` event.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum HookInstallDefinitionItem {
    RunString(#[schemars(schema_with = "refs::hook_run_string")] ()),
    RunTable(#[schemars(schema_with = "refs::hook_run_table")] ()),
    Script(#[schemars(schema_with = "refs::hook_install_script_table")] ()),
    Scripts(#[schemars(schema_with = "refs::hook_install_scripts_table")] ()),
    Task(#[schemars(schema_with = "refs::hook_task_ref")] ()),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum HookInstallDefinition {
    One(#[schemars(schema_with = "refs::hook_install_definition_item")] ()),
    /// install hooks to run
    Many(#[schemars(schema_with = "hook_install_definition_items")] ()),
}

fn hook_install_definition_items(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "items": {"$ref": "#/$defs/hook_install_definition_item"},
        "type": "array"
    })
}

/// hooks to run
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HooksDecl {
    /// hooks to run when an activated shell moves into the project
    #[serde(default)]
    #[schemars(schema_with = "refs::hook_definition")]
    pub enter: Option<HookDefinition>,
    /// hooks to run when an activated shell moves out of the project
    #[serde(default)]
    #[schemars(schema_with = "refs::hook_definition")]
    pub leave: Option<HookDefinition>,
    /// hooks to run when an activated shell changes to a directory inside the project
    #[serde(default)]
    #[schemars(schema_with = "refs::hook_definition")]
    pub cd: Option<HookDefinition>,
    /// hooks to run before mise installs tools
    #[serde(default)]
    #[schemars(schema_with = "refs::hook_install_definition")]
    pub preinstall: Option<HookInstallDefinition>,
    /// hooks to run after mise installs tools
    #[serde(default)]
    #[schemars(schema_with = "refs::hook_install_definition")]
    pub postinstall: Option<HookInstallDefinition>,
}
