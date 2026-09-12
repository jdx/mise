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

/// Whether keep-order should reserve an output slot for this task.
///
/// Tasks that produce output, plus the ones that inject other tasks: those
/// produce nothing themselves but anchor their children's blocks at their own
/// declared position. A task that only aggregates `depends` gets neither and
/// stays out — it finishes after everything it waits on, and only the front
/// entry of the buffer map may stream, so its empty slot would hold the live
/// stream for the whole run.
pub(crate) fn task_gets_keep_order_slot(task: &Task) -> bool {
    task_needs_permit(task) || task_runs_task_references(task)
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub(crate) fn canonicalize_path(path: &Path) -> PathBuf {
    canonicalize_or_self(path)
}
