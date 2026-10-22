//! What a `[tasks.<name>]` table or a `[task_templates.<name>]` table declares, before mise
//! renders templates, resolves tools or merges overlays. mise turns a [`TaskDecl`] into its
//! runtime `Task` with `Task::from_decl`.

use std::path::PathBuf;

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, de::Error as _};

use super::shapes::{
    RustCacheDecl, SilentDecl, TaskAlias, TaskCacheDecl, TaskConfirmDecl, TaskDaemonsDecl,
    TaskDepends, TaskDependsPost, TaskOutputDecl, TaskOutputsDecl, TaskRunDecl, TaskSecretsDecl,
    TaskSourcesDecl, TaskWaitFor,
};
use super::{
    G7_TEXT, RunEntry, Silent, TaskCacheConfig, TaskConfirm, TaskDaemons, TaskDep, TaskOutput,
    TaskOutputs, TaskRustCacheConfig, TaskSecrets, TaskToolValue, TaskWatchOptions,
};
use crate::env::EnvList;

// A task as declared in TOML. Unlike mise's `Task`, the flags a `[tasks.<name>]` overlay may
// set or leave alone (`hide`, `raw`, `raw_args`, `interactive`, `quiet`, `silent`) are
// `Option`s, so an absent key and an explicit `false` stay distinct.
/// common task properties
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskDecl {
    #[serde(
        default,
        rename = "alias",
        deserialize_with = "crate::de::deserialize_arr"
    )]
    #[schemars(with = "TaskAlias")]
    pub aliases: Vec<String>,
    #[serde(default)]
    #[schemars(with = "Option<TaskConfirmDecl>")]
    pub confirm: Option<TaskConfirm>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskDepends")]
    pub depends: Vec<TaskDep>,
    // Project daemons that must be running and ready before this task's body
    // starts. Skipped along with dependencies under `--skip-deps`.
    #[serde(default)]
    #[schemars(with = "Option<TaskDaemonsDecl>")]
    pub daemons: Option<TaskDaemons>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskDependsPost")]
    pub depends_post: Vec<TaskDep>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskWaitFor")]
    pub wait_for: Vec<TaskDep>,
    /// description of task
    #[serde(default)]
    pub description: String,
    /// directory to run script in, default is the project's base directory
    #[serde(default)]
    #[schemars(extend("default" = "{{ config_root }}"))]
    pub dir: Option<String>,
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::refs::env")]
    pub env: EnvList,
    #[serde(default, deserialize_with = "crate::env::deserialize_vars")]
    #[schemars(schema_with = "crate::schema::refs::vars")]
    pub vars: EnvList,
    #[serde(default)]
    #[schemars(schema_with = "super::schema::task_props_tools")]
    pub tools: IndexMap<String, TaskToolValue>,
    /// do not display this task
    #[schemars(extend("default" = false))]
    pub hide: Option<bool>,
    #[serde(default)]
    #[schemars(with = "TaskOutputsDecl")]
    pub outputs: TaskOutputs,
    // Experimental local artifact cache configuration.
    #[serde(default)]
    #[schemars(with = "Option<TaskCacheDecl>")]
    pub cache: Option<TaskCacheConfig>,
    // Deprecated compatibility field; enabled values emit an mbx migration warning.
    #[serde(default)]
    #[schemars(with = "Option<RustCacheDecl>")]
    pub rust_cache: Option<TaskRustCacheConfig>,
    // Per-task output *style* override (prefix/interleave/keep-order/…).
    // Orthogonal to `quiet`/`silent` verbosity; see [`TaskOutput`].
    #[serde(default)]
    #[schemars(with = "Option<TaskOutputDecl>")]
    pub output: Option<TaskOutput>,
    /// do not display mise information for this task
    #[schemars(extend("default" = false))]
    pub quiet: Option<bool>,
    // `default` keeps the field optional, since schemars cannot see the `Option` through
    // `schema_with`; it also replaces the schema's own default, so that is set again.
    #[serde(default)]
    #[schemars(with = "Option<SilentDecl>", extend("default" = false))]
    pub silent: Option<Silent>,
    /// mark task as interactive, acquiring exclusive terminal access while allowing other non-interactive tasks to run in parallel
    #[schemars(extend("default" = false))]
    pub interactive: Option<bool>,
    /// directly connect task to stdin/stdout/stderr
    #[schemars(extend("default" = false))]
    pub raw: Option<bool>,
    /// skip mise's argument parsing for this task — all arguments (including --help/-h) are forwarded verbatim to the underlying command. Use for tasks that proxy a tool with its own argument parser.
    // When true, mise does not parse arguments to the task at all:
    // the usage spec parser is bypassed and `--help`/`-h` are passed through
    // to the underlying command instead of being intercepted by mise.
    // Useful when the task is a thin proxy for a tool that already has its
    // own argument parser (e.g. `next build`, Django manage.py, argparse).
    #[schemars(extend("default" = false))]
    pub raw_args: Option<bool>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskRunDecl")]
    pub run: Vec<RunEntry>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskRunDecl")]
    pub run_windows: Vec<RunEntry>,
    /// Execute an external script
    #[serde(default)]
    pub file: Option<PathBuf>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    #[schemars(with = "TaskSourcesDecl")]
    pub sources: Vec<String>,
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::refs::task_watch")]
    pub watch: Option<TaskWatchOptions>,
    /// specify a shell command to run the script with
    #[serde(default)]
    pub shell: Option<String>,
    /// Specify usage (https://usage.jdx.dev/) specs for the task
    #[serde(default)]
    pub usage: String,
    /// timeout for this task
    #[serde(default)]
    pub timeout: Option<String>,
    /// shell command checked before the task runs; if it exits 0, the task is skipped and counts as successful
    // A shell command checked before the task runs; exiting 0 skips the task.
    #[serde(default)]
    pub skip_if: Option<String>,
    /// block reads, writes, network, and env vars
    #[serde(default)]
    pub deny_all: bool,
    /// block filesystem reads
    #[serde(default)]
    pub deny_read: bool,
    /// block all filesystem writes
    #[serde(default)]
    pub deny_write: bool,
    /// block all network access
    #[serde(default)]
    pub deny_net: bool,
    /// block env var inheritance
    #[serde(default)]
    pub deny_env: bool,
    /// allow reads from specific paths
    #[serde(default)]
    pub allow_read: Vec<PathBuf>,
    /// allow writes to specific paths
    #[serde(default)]
    pub allow_write: Vec<PathBuf>,
    /// allow network to specific hosts; not supported yet, so Linux and macOS reject it
    // Allow network to specific hosts
    #[serde(default)]
    pub allow_net: Vec<String>,
    /// allow specific env vars through
    #[serde(default)]
    pub allow_env: Vec<String>,
    /// experimental ambient env vars preserved under environment denial without affecting the task cache key
    // Preserve ambient env vars when env inheritance is denied without hashing their values
    #[serde(default)]
    pub pass_through_env: Vec<String>,
    // [experimental] Secret keys this task receives when it starts. Dependencies and
    // subtasks get only their own lists.
    #[serde(default)]
    #[schemars(with = "Option<TaskSecretsDecl>")]
    pub secrets: Option<TaskSecrets>,
    // Not in the schema: mise sets it while loading, and the docs never mention it.
    #[serde(default)]
    #[schemars(skip)]
    pub global: bool,
    // Not in the schema: mise sets it while loading, and the docs never mention it.
    #[serde(default)]
    #[schemars(skip)]
    pub args: Vec<String>,
    // Name of the task template to extend
    // In `$defs/task` rather than `task_props`, so templates do not take it.
    #[serde(default)]
    #[schemars(skip)]
    pub extends: Option<String>,
}

fn reject_template_secrets<'de, D: Deserializer<'de>>(_: D) -> Result<(), D::Error> {
    Err(D::Error::custom(G7_TEXT))
}

/// A task template definition that can be extended by tasks via `extends`
/// Templates are defined in [task_templates.*] sections of mise.toml
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskTemplate {
    #[serde(default)]
    pub description: String,
    #[serde(
        default,
        rename = "alias",
        deserialize_with = "crate::de::deserialize_arr"
    )]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub confirm: Option<TaskConfirm>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub depends: Vec<TaskDep>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub depends_post: Vec<TaskDep>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub wait_for: Vec<TaskDep>,
    #[serde(default)]
    pub daemons: Option<TaskDaemons>,
    /// Always an error: a template cannot list secrets (G7).
    #[serde(
        default,
        rename = "secrets",
        deserialize_with = "reject_template_secrets"
    )]
    pub _secrets: (),
    #[serde(default)]
    pub env: EnvList,
    #[serde(default, deserialize_with = "crate::env::deserialize_vars")]
    pub vars: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub watch: Option<TaskWatchOptions>,
    #[serde(default)]
    pub outputs: TaskOutputs,
    #[serde(default)]
    pub cache: Option<TaskCacheConfig>,
    #[serde(default)]
    pub rust_cache: Option<TaskRustCacheConfig>,
    #[serde(default)]
    pub output: Option<TaskOutput>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub silent: Option<Silent>,
    #[serde(default)]
    pub tools: IndexMap<String, TaskToolValue>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub timeout: Option<String>,
    #[serde(default)]
    pub skip_if: Option<String>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub run: Vec<RunEntry>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub run_windows: Vec<RunEntry>,
    #[serde(default)]
    pub file: Option<String>,
    /// Block reads, writes, network, and env vars
    #[serde(default)]
    pub deny_all: bool,
    /// Block filesystem reads
    #[serde(default)]
    pub deny_read: bool,
    /// Block all filesystem writes
    #[serde(default)]
    pub deny_write: bool,
    /// Block all network access
    #[serde(default)]
    pub deny_net: bool,
    /// Block env var inheritance
    #[serde(default)]
    pub deny_env: bool,
    /// Allow reads from specific paths
    #[serde(default)]
    pub allow_read: Vec<PathBuf>,
    /// Allow writes to specific paths
    #[serde(default)]
    pub allow_write: Vec<PathBuf>,
    /// Allow network to specific hosts
    #[serde(default)]
    pub allow_net: Vec<String>,
    /// Allow specific env vars through
    #[serde(default)]
    pub allow_env: Vec<String>,
    /// Preserve ambient env vars when env inheritance is denied without hashing their values
    #[serde(default)]
    pub pass_through_env: Vec<String>,
}
