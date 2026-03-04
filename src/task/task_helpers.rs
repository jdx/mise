use crate::config::env_directive::EnvDirective;
use crate::task::Task;
use std::path::{Path, PathBuf};

/// Internal marker propagated to injected tasks so they keep the parent interactive barrier.
pub const INTERACTIVE_CHAIN_ENV: &str = "__MISE_INTERACTIVE_CHAIN";
/// Internal marker propagated to injected tasks when parent keeps a permit.
pub const PARENT_PERMIT_CHAIN_ENV: &str = "__MISE_PARENT_PERMIT_CHAIN";

/// Check if a task needs a runtime slot from the scheduler semaphore.
/// Only shell/script tasks execute external commands and need a concurrency slot.
/// Orchestrator-only tasks (pure groups of sub-tasks) do not.
pub fn task_uses_runtime_slot(task: &Task) -> bool {
    task.file.is_some() || task.has_runtime_script()
}

/// Returns true when the task belongs to an interactive injected subgraph.
pub fn task_is_interactive_chain(task: &Task) -> bool {
    task.inherited_env.0.iter().any(|directive| {
        matches!(
            directive,
            EnvDirective::Val(k, v, _) if k == INTERACTIVE_CHAIN_ENV && v == "1"
        )
    })
}

/// Returns true when the task belongs to an injected subgraph whose parent holds a permit.
pub fn task_is_parent_permit_chain(task: &Task) -> bool {
    task.inherited_env.0.iter().any(|directive| {
        matches!(
            directive,
            EnvDirective::Val(k, v, _) if k == PARENT_PERMIT_CHAIN_ENV && v == "1"
        )
    })
}

/// Interactive tasks and their injected descendants must keep a global runtime barrier.
pub fn task_propagates_interactive_barrier(task: &Task) -> bool {
    task.interactive || task_is_interactive_chain(task)
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
