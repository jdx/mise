use crate::task::Task;
use std::path::{Path, PathBuf};

/// Check if a task needs a permit from the semaphore
/// Only shell/script tasks execute external commands and need a concurrency slot.
/// Orchestrator-only tasks (pure groups of sub-tasks) do not.
pub fn task_needs_permit(task: &Task) -> bool {
    task.file.is_some() || !task.run_script_strings().is_empty()
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
