//! What a `[tasks.<name>]` table or a `[task_templates.<name>]` table declares, before mise
//! renders templates, resolves tools or merges overlays. mise turns a [`TaskDecl`] into its
//! runtime `Task` with `Task::from_decl`.

use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, de::Error as _};

use super::{
    G7_TEXT, RunEntry, Silent, TaskCacheConfig, TaskConfirm, TaskDaemons, TaskDep, TaskOutput,
    TaskOutputs, TaskRustCacheConfig, TaskSecrets, TaskToolValue, TaskWatchOptions,
};
use crate::env::EnvList;

/// A task as declared in TOML. Unlike mise's `Task`, the flags a `[tasks.<name>]` overlay may
/// set or leave alone (`hide`, `raw`, `raw_args`, `interactive`, `quiet`, `silent`) are
/// `Option`s, so an absent key and an explicit `false` stay distinct.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDecl {
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
    /// Project daemons that must be running and ready before this task's body
    /// starts. Skipped along with dependencies under `--skip-deps`.
    #[serde(default)]
    pub daemons: Option<TaskDaemons>,
    /// [experimental] Secret keys this task receives when it starts. Dependencies and
    /// subtasks get only their own lists.
    #[serde(default)]
    pub secrets: Option<TaskSecrets>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub depends_post: Vec<TaskDep>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub wait_for: Vec<TaskDep>,
    #[serde(default)]
    pub env: EnvList,
    #[serde(default, deserialize_with = "crate::env::deserialize_vars")]
    pub vars: EnvList,
    #[serde(default)]
    pub dir: Option<String>,
    pub hide: Option<bool>,
    #[serde(default)]
    pub global: bool,
    pub raw: Option<bool>,
    /// When true, mise does not parse arguments to the task at all:
    /// the usage spec parser is bypassed and `--help`/`-h` are passed through
    /// to the underlying command instead of being intercepted by mise.
    /// Useful when the task is a thin proxy for a tool that already has its
    /// own argument parser (e.g. `next build`, Django manage.py, argparse).
    pub raw_args: Option<bool>,
    pub interactive: Option<bool>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub watch: Option<TaskWatchOptions>,
    #[serde(default)]
    pub outputs: TaskOutputs,
    /// Experimental local artifact cache configuration.
    #[serde(default)]
    pub cache: Option<TaskCacheConfig>,
    /// Deprecated compatibility field; enabled values emit an mbx migration warning.
    #[serde(default)]
    pub rust_cache: Option<TaskRustCacheConfig>,
    #[serde(default)]
    pub shell: Option<String>,
    pub quiet: Option<bool>,
    pub silent: Option<Silent>,
    /// Per-task output *style* override (prefix/interleave/keep-order/…).
    /// Orthogonal to `quiet`/`silent` verbosity; see [`TaskOutput`].
    #[serde(default)]
    pub output: Option<TaskOutput>,
    #[serde(default)]
    pub tools: IndexMap<String, TaskToolValue>,
    #[serde(default)]
    pub usage: String,
    #[serde(default)]
    pub timeout: Option<String>,
    /// A shell command checked before the task runs; exiting 0 skips the task.
    #[serde(default)]
    pub skip_if: Option<String>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub run: Vec<RunEntry>,
    #[serde(default, deserialize_with = "crate::de::deserialize_arr")]
    pub run_windows: Vec<RunEntry>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub file: Option<PathBuf>,
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
    /// Name of the task template to extend
    #[serde(default)]
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
