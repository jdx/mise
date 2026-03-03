use crate::task::Task;
use std::path::{Path, PathBuf};

pub const STATIC_INTERNAL_TASK_PREFIX: &str = "__mise_static::";
pub const STATIC_BARRIER_START_SEGMENT: &str = "__barrier_start__";
pub const STATIC_BARRIER_END_SEGMENT: &str = "__barrier_end__";

/// Check if a task needs a permit from the semaphore
/// Only runtime tasks execute external commands and need a concurrency slot.
/// Orchestrator-only tasks (pure groups of sub-tasks) do not.
pub fn task_needs_permit(task: &Task) -> bool {
    task_is_runtime(task)
}

/// Runtime tasks execute shell/file work themselves.
/// This includes mixed tasks that also orchestrate sub-tasks via run entries.
pub fn task_is_runtime(task: &Task) -> bool {
    task.file.is_some() || !task.run_script_strings().is_empty()
}

/// Pure orchestrator tasks only reference other tasks in `run` and have no
/// direct runtime/file work themselves.
pub fn task_is_pure_orchestrator(task: &Task) -> bool {
    task.file.is_none() && !task.run().is_empty() && task.run_script_strings().is_empty()
}

#[derive(Debug, Default)]
pub struct ReadyTaskBuckets {
    pub runtime_non_interactive: Vec<Task>,
    pub interactive_runtime: Vec<Task>,
    pub orchestrators: Vec<Task>,
}

pub fn classify_ready_tasks(ready: impl IntoIterator<Item = Task>) -> ReadyTaskBuckets {
    let mut buckets = ReadyTaskBuckets::default();
    for task in ready {
        if task_is_runtime(&task) {
            if task.is_interactive() {
                buckets.interactive_runtime.push(task);
            } else {
                buckets.runtime_non_interactive.push(task);
            }
        } else {
            buckets.orchestrators.push(task);
        }
    }
    buckets.runtime_non_interactive.sort();
    buckets.interactive_runtime.sort();
    buckets.orchestrators.sort();
    buckets
}

pub fn task_has_static_internal_name(task: &Task) -> bool {
    task.name.starts_with(STATIC_INTERNAL_TASK_PREFIX)
}

pub fn task_logical_name(task: &Task) -> &str {
    if task_has_static_internal_name(task) {
        if let Some(original) = task
            .aliases
            .iter()
            .find(|name| !name.is_empty() && !name.starts_with(STATIC_INTERNAL_TASK_PREFIX))
        {
            return original;
        }
        if let Some(alias) = task.aliases.iter().find(|name| !name.is_empty()) {
            return alias;
        }
        if !task.display_name.is_empty() {
            return &task.display_name;
        }
    }
    &task.name
}

pub fn task_is_static_join_noop(task: &Task) -> bool {
    task_has_static_internal_name(task)
        && task.display_name.is_empty()
        && task.file.is_none()
        && task.run().is_empty()
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;

    #[test]
    fn test_task_is_runtime_for_script_task() {
        // MatrixRef: V10 / C7
        let task = Task {
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        assert!(task_is_runtime(&task));
        assert!(!task_is_pure_orchestrator(&task));
    }

    #[test]
    fn test_task_is_runtime_for_file_task() {
        // MatrixRef: V09 / C7
        let task = Task {
            file: Some("script.sh".into()),
            ..Default::default()
        };
        assert!(task_is_runtime(&task));
        assert!(!task_is_pure_orchestrator(&task));
    }

    #[test]
    fn test_task_is_pure_orchestrator() {
        // MatrixRef: V11 / C7
        let task = Task {
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        };
        assert!(!task_is_runtime(&task));
        assert!(task_is_pure_orchestrator(&task));
    }

    #[test]
    fn test_task_is_runtime_for_mixed_run_entries() {
        // MatrixRef: V12 / C7
        let task = Task {
            run: vec![
                RunEntry::Script("echo hi".to_string()),
                RunEntry::SingleTask {
                    task: "build".to_string(),
                },
            ],
            ..Default::default()
        };
        assert!(task_is_runtime(&task));
        assert!(!task_is_pure_orchestrator(&task));
    }

    #[test]
    fn test_task_has_static_internal_name() {
        let internal = Task {
            name: format!("{STATIC_INTERNAL_TASK_PREFIX}wrapper::child::1"),
            ..Default::default()
        };
        let normal = Task {
            name: "child".to_string(),
            ..Default::default()
        };
        assert!(task_has_static_internal_name(&internal));
        assert!(!task_has_static_internal_name(&normal));
    }

    #[test]
    fn test_task_is_static_join_noop() {
        let join = Task {
            name: format!("{STATIC_INTERNAL_TASK_PREFIX}join::1"),
            display_name: String::new(),
            ..Default::default()
        };
        let internal_runtime = Task {
            name: format!("{STATIC_INTERNAL_TASK_PREFIX}wrapper::child::1"),
            display_name: "child".to_string(),
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        assert!(task_is_static_join_noop(&join));
        assert!(!task_is_static_join_noop(&internal_runtime));
    }

    #[test]
    fn test_task_logical_name_prefers_non_internal_alias_for_static_tasks() {
        let task = Task {
            name: format!("{STATIC_INTERNAL_TASK_PREFIX}wrapper::child::9"),
            aliases: vec![
                format!("{STATIC_INTERNAL_TASK_PREFIX}parent::child::1"),
                "child".to_string(),
            ],
            ..Default::default()
        };
        assert_eq!(task_logical_name(&task), "child");
    }

    #[test]
    fn test_task_logical_name_falls_back_to_display_name_for_static_tasks() {
        let task = Task {
            name: format!("{STATIC_INTERNAL_TASK_PREFIX}wrapper::join::1"),
            display_name: "join".to_string(),
            ..Default::default()
        };
        assert_eq!(task_logical_name(&task), "join");
    }

    #[test]
    fn test_classify_ready_tasks_buckets_and_sorts_deterministically() {
        let ready = vec![
            Task {
                name: "ask".to_string(),
                args: vec!["z".to_string()],
                interactive: Some(true),
                run: vec![RunEntry::Script("read x".to_string())],
                ..Default::default()
            },
            Task {
                name: "build".to_string(),
                args: vec!["b".to_string()],
                run: vec![RunEntry::Script("echo hi".to_string())],
                ..Default::default()
            },
            Task {
                name: "group".to_string(),
                run: vec![RunEntry::SingleTask {
                    task: "build".to_string(),
                }],
                ..Default::default()
            },
            Task {
                name: "ask".to_string(),
                args: vec!["a".to_string()],
                interactive: Some(true),
                run: vec![RunEntry::Script("read x".to_string())],
                ..Default::default()
            },
        ];
        let buckets = classify_ready_tasks(ready);

        assert_eq!(
            buckets
                .runtime_non_interactive
                .iter()
                .map(|t| (t.name.clone(), t.args.clone()))
                .collect::<Vec<_>>(),
            vec![("build".to_string(), vec!["b".to_string()])]
        );
        assert_eq!(
            buckets
                .interactive_runtime
                .iter()
                .map(|t| (t.name.clone(), t.args.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("ask".to_string(), vec!["a".to_string()]),
                ("ask".to_string(), vec!["z".to_string()]),
            ]
        );
        assert_eq!(
            buckets
                .orchestrators
                .iter()
                .map(|t| t.name.clone())
                .collect::<Vec<_>>(),
            vec!["group".to_string()]
        );
    }
}
