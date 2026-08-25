use crate::file::canonicalize_or_self;
use crate::task::{RunEntry, Task};
use std::path::{Path, PathBuf};

/// Check if a task needs a permit from the semaphore
/// Only shell/script tasks execute external commands and need a concurrency slot.
/// Orchestrator-only tasks (pure groups of sub-tasks) do not.
pub(crate) fn task_needs_permit(task: &Task) -> bool {
    task.file.is_some() || !task.run_script_strings().is_empty()
}

/// Whether this task starts other tasks from its `run` entries.
///
/// Such a task produces no output of its own — `task_needs_permit` is false, and
/// the `Finished in` line is gated on the same condition — but keep-order still
/// needs a slot for it, to anchor the blocks of the tasks it injects at the
/// position the task itself was declared in.
pub(crate) fn task_runs_task_references(task: &Task) -> bool {
    task.run().iter().any(|e| !matches!(e, RunEntry::Script(_)))
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub(crate) fn canonicalize_path(path: &Path) -> PathBuf {
    canonicalize_or_self(path)
}
