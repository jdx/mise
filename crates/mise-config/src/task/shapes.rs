//! The declaration types behind the schema for the task fields that accept several shapes: a
//! string or a list, a name or a table, and the like.
//!
//! [`TaskDecl`](super::TaskDecl) and [`TaskConfig`](super::TaskConfig) keep the runtime type of
//! each of these fields and take their schema from the type here, through `schemars(with = ..)`.
//! Nothing parses into these types: they describe what the parsers in `super` accept, and
//! `schema_matches_parser` checks every config in the docs and the e2e tests against both. The
//! doc comments are the descriptions in `schema/mise.json`; notes for Rust readers go in plain
//! comments.

use std::collections::BTreeMap;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};

use crate::schema::{EnvVarValue, refs};

// A task, or a template, may name its aliases as one string or a list of them.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskAlias {
    /// alias for this task
    One(String),
    /// alias for this task
    Many(Vec<TaskAliasName>),
}

/// alias for this task
#[derive(JsonSchema)]
pub struct TaskAliasName(pub String);

/// prompt to answer before this task's own command runs, after its dependencies
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskConfirmDecl {
    /// confirmation message before running this task
    Message(String),
    /// confirmation message before running this task with optional default value and custom answer labels
    Table(TaskConfirmTable),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskConfirmTable {
    /// confirmation message before running this task
    pub message: String,
    /// default value for confirmation (yes/no/true/false)
    #[schemars(extend("enum" = ["yes", "no", "y", "n", "true", "false"]))]
    pub default: Option<String>,
    /// custom label for the affirmative answer
    pub yes: Option<String>,
    /// custom label for the negative answer
    pub no: Option<String>,
}

// `depends`, `depends_post` and `wait_for` take the same shapes and say what they are for.
macro_rules! task_dependencies {
    ($name:ident, $doc:literal, $what:literal) => {
        #[doc = $doc]
        #[derive(JsonSchema)]
        #[serde(untagged)]
        #[schemars(transform = crate::schema::one_of)]
        pub enum $name {
            #[doc = $what]
            One(String),
            #[doc = $what]
            Many(#[schemars(schema_with = "dependency_items")] ()),
            Table(#[schemars(schema_with = "refs::task_dependency_table")] ()),
        }
    };
}

task_dependencies!(
    TaskDepends,
    "tasks that must finish before this task runs",
    "task with args to run before this task"
);
task_dependencies!(
    TaskDependsPost,
    "tasks to run after this task finishes",
    "task with args to run after this task"
);
task_dependencies!(
    TaskWaitFor,
    "tasks to wait for if they are already scheduled, without adding them to the run",
    "task with args to wait for completion first"
);

fn dependency_items(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"$ref": "#/$defs/task_dependency_item"}})
}

/// task name and args
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskDependencyItem {
    One(String),
    Args(#[schemars(length(min = 1))] Vec<String>),
    Table(#[schemars(schema_with = "refs::task_dependency_table")] ()),
}

/// task dependency with args, env, and optional flag
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskDependencyTable {
    /// task name, alias, or pattern
    pub task: String,
    /// arguments to pass to the task
    pub args: Option<Vec<String>>,
    /// environment variables to set for this dependency only
    pub env: Option<BTreeMap<String, EnvVarValue>>,
    /// do not fail when the task name or pattern has no matches
    pub optional: Option<bool>,
}

/// [experimental] project daemons that must be running and ready before this task runs
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskDaemonsDecl {
    /// true requires every daemon declared in [daemons]
    All(bool),
    /// name of a daemon declared in [daemons]
    One(String),
    /// names of daemons declared in [daemons]
    Many(Vec<String>),
}

/// files or directories this task creates; compared with `sources` to skip the task when it is up to date
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskOutputsDecl {
    /// files created by this task (entries starting with `!` are excluded; use `\!` to escape a literal `!` prefix)
    Many(Vec<TaskOutputPattern>),
    /// glob pattern or path to files created by this task (entries starting with `!` are excluded)
    One(String),
    Auto(TaskOutputsAuto),
}

/// glob pattern or path to files created by this task (entries starting with `!` are excluded)
#[derive(JsonSchema)]
pub struct TaskOutputPattern(pub String);

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskOutputsAuto {
    /// automatically touch an internal tracked file instead of specifying outputs; `false` is the same as leaving `outputs` unset
    pub auto: bool,
}

/// files this task reads; mise skips the task when none changed since its outputs were written
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskSourcesDecl {
    /// files that this task depends on (entries starting with `!` are excluded; use `\!` to escape a literal `!` prefix)
    Many(Vec<TaskSourcePattern>),
    /// glob pattern or path to files that this task depends on (entries starting with `!` are excluded)
    One(String),
}

/// glob pattern or path to files that this task depends on (entries starting with `!` are excluded)
#[derive(JsonSchema)]
pub struct TaskSourcePattern(pub String);

/// experimental local artifact cache for declared task outputs
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskCacheDecl {
    /// store and restore task outputs from the local mise cache
    #[schemars(extend("default" = false))]
    pub enabled: Option<bool>,
    /// report project files read or written outside declared cache sources and outputs
    #[schemars(extend("default" = false))]
    pub audit: Option<bool>,
    /// ambient environment variable names whose values affect the cache key
    pub env: Option<Vec<String>>,
    /// commands whose stdout and stderr affect the cache key
    pub command_inputs: Option<Vec<String>>,
}

/// default experimental local artifact cache configuration for eligible tasks with sources and explicit outputs
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskConfigCacheDecl {
    /// store and restore eligible task outputs from the local mise cache
    #[schemars(extend("default" = false))]
    pub enabled: Option<bool>,
    /// report project files read or written outside declared cache sources and outputs
    #[schemars(extend("default" = false))]
    pub audit: Option<bool>,
    /// ambient environment variable names whose values affect eligible task cache keys
    pub env: Option<Vec<String>>,
    /// commands whose stdout and stderr affect eligible task cache keys
    pub command_inputs: Option<Vec<String>>,
}

/// source patterns in this group, relative to the config root; may reference other groups with @group:<name>
#[derive(JsonSchema)]
pub struct TaskInputGroup(pub Vec<String>);

/// deprecated no-op; use mbx for Rust action caching instead: https://mr-boxington.jdx.dev/getting-started
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of, extend("deprecated" = true))]
pub enum RustCacheDecl {
    Enabled(bool),
    Table(RustCacheTable),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct RustCacheTable {
    /// deprecated no-op
    #[schemars(extend("default" = true, "deprecated" = true))]
    pub enabled: Option<bool>,
    /// deprecated no-op
    #[schemars(extend("default" = false, "deprecated" = true))]
    pub verify: Option<bool>,
}

/// output style for this task (prefix, interleave, keep-order, replacing, timed, quiet, silent). Independent of the quiet/silent verbosity fields.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TaskOutputDecl {
    Prefix,
    Interleave,
    KeepOrder,
    Replacing,
    Timed,
    Quiet,
    Silent,
}

/// suppress all output for this task
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum SilentDecl {
    All(bool),
    Stream(SilentStream),
}

#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SilentStream {
    Stdout,
    Stderr,
}

// `run` and `run_windows`: one entry or a list of them.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskRunDecl {
    One(#[schemars(schema_with = "refs::task_run_entry")] ()),
    Many(#[schemars(schema_with = "run_entries")] ()),
}

fn run_entries(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "array", "items": {"$ref": "#/$defs/task_run_entry"}})
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskRunEntry {
    /// script to run
    Script(String),
    Task(TaskRunSingle),
    Group(TaskRunGroup),
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskRunSingle {
    /// single task name to run
    pub task: String,
    /// arguments to pass to the task
    pub args: Option<Vec<String>>,
    /// environment variables to set for the task
    pub env: Option<BTreeMap<String, EnvVarValue>>,
}

#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskRunGroup {
    /// parallel task group to run
    pub tasks: Vec<TaskNameAndArgs>,
}

/// task name and args
#[derive(JsonSchema)]
pub struct TaskNameAndArgs(pub String);

/// [experimental] Secret keys this task receives when it starts, e.g. ["DEPLOY_KEY"]. Dependencies do not inherit them. See `mise secrets ls`.
#[derive(JsonSchema)]
#[serde(untagged)]
#[schemars(transform = crate::schema::one_of)]
pub enum TaskSecretsDecl {
    One(#[schemars(regex(pattern = r"^[A-Za-z_][A-Za-z0-9_]*$"))] String),
    Many(Vec<TaskSecretKey>),
}

#[derive(JsonSchema)]
pub struct TaskSecretKey(#[schemars(regex(pattern = r"^[A-Za-z_][A-Za-z0-9_]*$"))] pub String);

/// options used by `mise watch` for this task
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(transform = crate::schema::unevaluated)]
pub struct TaskWatchDecl {
    /// do not load VCS ignore files such as `.gitignore` when watching this task
    #[schemars(extend("default" = false))]
    pub no_vcs_ignore: Option<bool>,
}

/// files/directories to include searching for tasks. Can be local paths, git repository URLs using git:: prefix (e.g., git::https://github.com/org/repo.git//path?ref=branch), or OCI artifacts using oci:: prefix (e.g., oci::ghcr.io/org/tasks:1.0.0)
#[derive(JsonSchema)]
pub struct TaskIncludes(pub Vec<TaskInclude>);

/// file/directory root to include in task execution. Supports local paths, git URLs (git::https://github.com/org/repo.git//path?ref=branch), and OCI artifacts (oci::ghcr.io/org/tasks:1.0.0)
#[derive(JsonSchema)]
pub struct TaskInclude(pub String);
