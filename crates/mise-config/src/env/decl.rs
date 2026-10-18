//! The declaration types behind the schema for `[env]` and `[vars]`.
//!
//! The parser in `super` accepts shapes a derive cannot describe on its own: an ordered `_` table
//! of directives, cross-field rules between `value`, `default`, `required` and `age`, and
//! deprecated spellings. It reads them with visitors, so these types describe what it accepts and
//! nothing parses into them; `schema_matches_parser` checks every config in the docs and the e2e
//! tests against both. The doc comments are the descriptions in `schema/mise.json`; notes for Rust
//! readers go in plain comments.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::schema::refs;

/// environment variables
#[derive(JsonSchema)]
pub struct EnvDecl {
    #[serde(rename = "_")]
    pub modules: Option<EnvModules>,
    #[serde(flatten)]
    pub variables: BTreeMap<String, EnvVariable>,
}

/// variables to set
#[derive(JsonSchema)]
pub struct VarsDecl {
    #[serde(rename = "_")]
    pub modules: Option<VarsModules>,
    #[serde(flatten)]
    pub variables: BTreeMap<String, VarsVariable>,
}

/// environment modules
#[derive(JsonSchema)]
#[schemars(extend("additionalProperties" = true))]
pub struct EnvModules {
    pub file: Option<EnvFiles>,
    pub path: Option<EnvPaths>,
    pub python: Option<EnvPython>,
    pub source: Option<EnvSources>,
}

/// vars modules
#[derive(JsonSchema)]
#[schemars(extend("additionalProperties" = true))]
pub struct VarsModules {
    pub file: Option<EnvFiles>,
    pub source: Option<EnvSources>,
}

// `env._.file`: a directive, a file, or a list of either.
/// files to load variables from (dotenv, json, yaml, or toml)
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvFiles {
    Directive(#[schemars(schema_with = "refs::env_directive")] ()),
    /// environment file to load (dotenv, json, yaml, or toml)
    One(String),
    /// environment files to load (dotenv, json, yaml, or toml)
    Many(Vec<EnvFileItem>),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvFileItem {
    /// environment file to load (dotenv, json, yaml, or toml)
    One(String),
    Directive(#[schemars(schema_with = "refs::env_directive")] ()),
}

// `env._.source`
/// bash scripts to run, taking the variables they export
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvSources {
    Directive(#[schemars(schema_with = "refs::env_directive")] ()),
    /// bash script to load
    One(String),
    /// bash scripts to load
    Many(Vec<EnvSourceItem>),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvSourceItem {
    /// bash script to load
    One(String),
    Directive(#[schemars(schema_with = "refs::env_directive")] ()),
}

// `env._.path`
/// directories to add to the front of PATH
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvPaths {
    Table(EnvPathTable),
    /// PATH entry to add
    One(String),
    /// PATH entries to add
    Many(Vec<EnvPathItem>),
}

// `path` or `paths`, one of the two.
#[derive(JsonSchema)]
#[schemars(extend(
    "x-mise-open" = true,
    "oneOf" = [
        {"type": "object", "x-mise-open": true, "properties": {"path": {"$ref": "#/$defs/env_path"}}, "required": ["path"]},
        {"type": "object", "x-mise-open": true, "properties": {"paths": {"$ref": "#/$defs/env_paths"}}, "required": ["paths"]}
    ]
))]
pub struct EnvPathTable {
    /// directory to add to PATH, or a list of them
    #[serde(default)]
    #[schemars(schema_with = "refs::env_path")]
    pub path: Option<()>,
    /// directories to add to PATH
    #[serde(default)]
    #[schemars(schema_with = "refs::env_paths")]
    pub paths: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvPathItem {
    /// PATH entry to add
    One(String),
    Table(EnvPathItemTable),
}

#[derive(JsonSchema)]
#[schemars(extend("x-mise-open" = true))]
pub struct EnvPathItemTable {
    /// directory to add to PATH, or a list of them
    #[serde(default)]
    #[schemars(schema_with = "refs::env_path")]
    pub path: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
}

/// python environment
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvPython {
    /// python virtual environment to activate
    pub venv: Option<EnvVenv>,
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvVenv {
    /// path to python virtual environment to use
    Path(String),
    /// virtualenv options
    Options(EnvVenvOptions),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnvVenvOptions {
    /// create a new virtual environment if one does not exist
    #[schemars(extend("default" = false))]
    pub create: Option<bool>,
    /// path to python virtual environment to use
    pub path: String,
    /// python version to use
    pub python: Option<String>,
    /// additional arguments to pass to python when creating a virtual environment
    pub python_create_args: Option<Vec<String>>,
    /// additional arguments to pass to uv when creating a virtual environment
    pub uv_create_args: Option<Vec<String>>,
}

// One `[env]` entry: a value, or a table that says what the value is. Which table it is follows
// from its keys, so each is closed with `unevaluatedProperties`.
/// an environment variable: its value, or a table with `value`, `default`, `required` or `age`
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvVariable {
    Value(EnvValueTable),
    Default(EnvDefaultTable),
    Required(EnvRequiredTable),
    /// [experimental] age-encrypted environment variable
    Age(EnvAgeTable),
    /// [experimental] age-encrypted environment variable
    AgeTable(EnvAgeComplexTable),
    Plain(#[schemars(schema_with = "refs::env_value")] ()),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvValueTable {
    #[schemars(schema_with = "refs::env_value")]
    pub value: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    /// a variable with its own value cannot be required; only `required = false` is accepted
    #[serde(default)]
    #[schemars(schema_with = "always_false")]
    pub required: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvDefaultTable {
    #[schemars(schema_with = "refs::env_default")]
    pub default: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvRequiredTable {
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    /// a variable without a value must be required: `true` or help text on how to set it
    #[schemars(schema_with = "required_true_or_help")]
    pub required: (),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvAgeTable {
    #[schemars(schema_with = "refs::env_age")]
    pub age: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_required")]
    pub required: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvAgeComplexTable {
    pub age: EnvAgeComplex,
}

/// [experimental] age-encrypted value (complex format)
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct EnvAgeComplex {
    #[schemars(schema_with = "refs::env_age_value")]
    pub value: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_age_format")]
    pub format: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_required")]
    pub required: Option<()>,
}

// One `[vars]` entry. Like an `[env]` one, but a value is a string, an integer or `true`, and a
// variable may carry a `prompt`.
/// a variable: its value, or a table with `value`, `default`, `required` or `age`
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum VarsVariable {
    /// value of variable
    Text(String),
    /// value of variable
    Integer(i64),
    /// value of variable
    Flag(#[schemars(schema_with = "always_true")] bool),
    Value(VarsValueTable),
    Default(VarsDefaultTable),
    Required(VarsRequiredTable),
    /// [experimental] age-encrypted variable
    Age(VarsAgeTable),
    AgeTable(VarsAgeComplexTable),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsValueTable {
    /// value of variable
    #[schemars(schema_with = "vars_value")]
    pub value: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsDefaultTable {
    #[schemars(schema_with = "refs::env_default")]
    pub default: (),
    /// question to ask once, on a machine with no saved answer, when a command is run with a prompt flag such as mise bootstrap --prompt-vars
    pub prompt: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsRequiredTable {
    /// question to ask once, on a machine with no saved answer, when a command is run with a prompt flag such as mise bootstrap --prompt-vars
    pub prompt: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    /// fail unless the process environment, a higher-precedence config file, or a saved answer supplies a value: true, or help text shown in the error
    #[schemars(schema_with = "required_true_or_text")]
    pub required: (),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsAgeTable {
    #[schemars(schema_with = "refs::env_age")]
    pub age: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsAgeComplexTable {
    pub age: VarsAgeComplex,
}

/// [experimental] age-encrypted value (complex format)
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct VarsAgeComplex {
    #[schemars(schema_with = "refs::env_age_value")]
    pub value: (),
    #[serde(default)]
    #[schemars(schema_with = "refs::env_age_format")]
    pub format: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
}

fn always_false(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"const": false})
}

fn always_true(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"const": true})
}

fn required_true_or_help(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"oneOf": [{"const": true}, {"type": "string"}]})
}

fn required_true_or_text(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"oneOf": [{"const": true}, {"type": "string"}]})
}

fn vars_value(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"oneOf": [{"type": "string"}, {"type": "integer"}, {"const": true}]})
}

// A directive of `env._.file`, `env._.source` and the like. Which key it carries decides what it
// does, and `path` or `paths` is required.
#[derive(JsonSchema)]
#[schemars(extend(
    "x-mise-open" = true,
    "oneOf" = [
        {"type": "object", "x-mise-open": true, "properties": {"path": {"$ref": "#/$defs/env_path"}}, "required": ["path"]},
        {"type": "object", "x-mise-open": true, "properties": {"paths": {"$ref": "#/$defs/env_paths"}}, "required": ["paths"]}
    ]
))]
pub struct EnvDirectiveDecl {
    /// file to load, or a list of them; may be a glob pattern
    #[serde(default)]
    #[schemars(schema_with = "refs::env_path")]
    pub path: Option<()>,
    /// files to load; each may be a glob pattern
    #[serde(default)]
    #[schemars(schema_with = "refs::env_paths")]
    pub paths: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_tools")]
    pub tools: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_file_expand")]
    pub expand: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_file_find_up")]
    pub find_up: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_redact")]
    pub redact: Option<()>,
    #[serde(default)]
    #[schemars(schema_with = "refs::env_required")]
    pub required: Option<()>,
}

/// environment variable value; false removes the variable
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvValueDecl {
    Text(String),
    Integer(i64),
    Flag(bool),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvPathDecl {
    One(String),
    Many(Vec<String>),
}

/// load tools before resolving
#[derive(JsonSchema)]
pub struct EnvToolsDecl(pub bool);

/// enable shell-style variable expansion for values loaded by env._.file
#[derive(JsonSchema)]
#[schemars(extend("default" = false))]
pub struct EnvFileExpandDecl(pub bool);

/// look for a relative env._.file path in the current directory and each parent directory
#[derive(JsonSchema)]
#[schemars(extend("default" = false))]
pub struct EnvFileFindUpDecl(pub bool);

/// redact the value from logs
#[derive(JsonSchema)]
pub struct EnvRedactDecl(pub bool);

/// require this environment variable to be defined before mise runs or in a later config file. Can be a boolean or a help string. A variable that carries its own `value` cannot also be `required = true`.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvRequiredDecl {
    /// require this environment variable to be defined before mise runs or in a later config file
    Flag(bool),
    /// require this environment variable with user help text on how to set it
    Help(String),
}

/// fallback value to use when the environment variable is unset or empty
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum EnvDefaultDecl {
    /// fallback value to use when the environment variable is unset or empty
    Text(String),
    /// fallback value to use when the environment variable is unset or empty
    Integer(i64),
}

/// [experimental] age-encrypted value (simplified format)
#[derive(JsonSchema)]
pub struct EnvAgeDecl(pub String);

/// [experimental] age-encrypted value
#[derive(JsonSchema)]
pub struct EnvAgeValueDecl(pub String);

/// [experimental] compression format for the encrypted value
#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EnvAgeFormatDecl {
    Raw,
    Zstd,
}
