use crate::config::env_directive::EnvDirective;
use crate::task::{RunEntry, Task};
use eyre::{Result, bail};
use std::path::{Path, PathBuf};

/// Canonical tie-break key for deterministic task ordering:
/// (task.name, args, env key-values).
pub type TaskOrderKey = (String, Vec<String>, Vec<(String, String)>);

/// Check if a task needs a permit from the semaphore
/// Only shell/script tasks execute external commands and need a concurrency slot.
/// Orchestrator-only tasks (pure groups of sub-tasks) do not.
pub fn task_needs_permit(task: &Task) -> bool {
    task.file.is_some() || !task.run_script_strings().is_empty()
}

pub fn task_order_key(task: &Task) -> TaskOrderKey {
    let mut env_key: Vec<(String, String)> = task
        .env
        .0
        .iter()
        .filter_map(|d| match d {
            EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .collect();
    env_key.sort();
    (task.name.clone(), task.args.clone(), env_key)
}

/// True when a runtime task also orchestrates sub-task execution from run entries.
/// These tasks need a stable owner so injected interactive children can resolve
/// barrier admission without self-deadlock.
pub fn task_requires_runtime_owner(task: &Task) -> bool {
    task_needs_permit(task)
        && task.run().iter().any(|entry| {
            matches!(
                entry,
                RunEntry::SingleTask { .. } | RunEntry::TaskGroup { .. }
            )
        })
}

/// True when task is both interactive and runtime (i.e. launches a user process).
pub fn task_is_interactive_runtime(task: &Task) -> bool {
    task.interactive && task_needs_permit(task)
}

/// True when an interactive runtime task should hold the global scheduler barrier.
/// Ownership inheritance for injected runtime subgraphs is handled by scheduler admission.
pub fn task_is_interactive_barrier_runtime(task: &Task) -> bool {
    task_is_interactive_runtime(task)
}

/// Validate task-level interactive configuration invariants.
pub fn validate_interactive_config(task: &Task) -> Result<()> {
    if !task.interactive {
        return Ok(());
    }

    if !task_needs_permit(task) {
        bail!(
            "task '{}' has interactive=true but is not a runtime task (pure {{task}}/{{tasks}} tasks are not interactive)",
            task.name
        );
    }

    if task.silent.is_silent() {
        bail!(
            "task '{}' has interactive=true which is incompatible with silent=true|\"stdout\"|\"stderr\"",
            task.name
        );
    }

    Ok(())
}

/// Validate runtime TTY precondition for interactive tasks.
pub fn validate_interactive_stdin(task: &Task, stdin_is_tty: bool) -> Result<()> {
    if task.interactive && !stdin_is_tty {
        bail!(
            "task '{}' has interactive=true but stdin is not a TTY (run from an interactive terminal)",
            task.name
        );
    }
    Ok(())
}

/// Canonicalize a path for use as cache key
/// Falls back to original path if canonicalization fails
pub fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{
        task_is_interactive_barrier_runtime, task_is_interactive_runtime, task_needs_permit,
        task_order_key, task_requires_runtime_owner, validate_interactive_config,
        validate_interactive_stdin,
    };
    use crate::config::env_directive::EnvDirective;
    use crate::task::{RunEntry, Silent, Task};

    #[test]
    fn runtime_helper_identifies_mixed_runtime_tasks() {
        // Matrix: V12 (C7)
        let task = Task {
            run: vec![
                RunEntry::Script("echo hi".to_string()),
                RunEntry::SingleTask {
                    task: "other".to_string(),
                },
            ],
            ..Default::default()
        };
        assert!(task_needs_permit(&task));
        assert!(task_requires_runtime_owner(&task));
        assert!(task_is_interactive_runtime(&Task {
            interactive: true,
            ..task.clone()
        }));
        assert!(task_is_interactive_barrier_runtime(&Task {
            interactive: true,
            ..task
        }));
    }

    #[test]
    fn interactive_runtime_helper_rejects_pure_orchestrators() {
        // Matrix: V11 (C7)
        let task = Task {
            interactive: true,
            run: vec![RunEntry::SingleTask {
                task: "other".to_string(),
            }],
            ..Default::default()
        };
        assert!(!task_is_interactive_runtime(&task));
        assert!(!task_is_interactive_barrier_runtime(&task));
        assert!(!task_requires_runtime_owner(&task));
    }

    #[test]
    fn runtime_owner_helper_rejects_pure_script_runtime() {
        let task = Task {
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        assert!(task_needs_permit(&task));
        assert!(!task_requires_runtime_owner(&task));
    }

    #[test]
    fn task_order_key_uses_name_args_env() {
        let task = Task {
            name: "b".to_string(),
            args: vec!["arg".to_string()],
            ..Default::default()
        }
        .with_dependency_env(&[
            EnvDirective::from(("B".to_string(), "2".to_string())),
            EnvDirective::from(("A".to_string(), "1".to_string())),
        ]);
        assert_eq!(
            task_order_key(&task),
            (
                "b".to_string(),
                vec!["arg".to_string()],
                vec![
                    ("A".to_string(), "1".to_string()),
                    ("B".to_string(), "2".to_string())
                ]
            )
        );
    }

    #[test]
    fn interactive_rejected_for_pure_orchestrator() {
        // Matrix: V11 (C7)
        let task = Task {
            name: "orchestrator".to_string(),
            interactive: true,
            run: vec![RunEntry::SingleTask {
                task: "other".to_string(),
            }],
            ..Default::default()
        };
        let err = validate_interactive_config(&task).unwrap_err().to_string();
        assert!(err.contains("not a runtime task"));
    }

    #[test]
    fn interactive_rejected_with_silent_modes() {
        // Matrix: V05/V06/V07 (C8)
        for silent in [Silent::Bool(true), Silent::Stdout, Silent::Stderr] {
            let task = Task {
                name: "interactive".to_string(),
                interactive: true,
                run: vec![RunEntry::Script("echo hi".to_string())],
                silent,
                ..Default::default()
            };
            let err = validate_interactive_config(&task).unwrap_err().to_string();
            assert!(err.contains("incompatible"));
        }
    }

    #[test]
    fn interactive_with_raw_is_allowed() {
        // Matrix: V08 (C9)
        let task = Task {
            name: "interactive-raw".to_string(),
            interactive: true,
            raw: true,
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        validate_interactive_config(&task).unwrap();
    }

    #[test]
    fn interactive_on_file_runtime_is_allowed() {
        // Matrix: V09 (C7)
        let task = Task {
            name: "interactive-file".to_string(),
            interactive: true,
            file: Some(std::path::PathBuf::from("task.sh")),
            ..Default::default()
        };
        validate_interactive_config(&task).unwrap();
    }

    #[test]
    fn interactive_on_run_script_runtime_is_allowed() {
        // Matrix: V10 (C7)
        let task = Task {
            name: "interactive-run".to_string(),
            interactive: true,
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        validate_interactive_config(&task).unwrap();
    }

    #[test]
    fn interactive_stdin_requires_tty() {
        // Matrix: S13/N01 (C17)
        let task = Task {
            name: "interactive".to_string(),
            interactive: true,
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        let err = validate_interactive_stdin(&task, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("stdin is not a TTY"));
    }

    #[test]
    fn non_interactive_stdin_pipe_is_allowed() {
        // Matrix: S13/N01 (C17, C16)
        let task = Task {
            name: "non-interactive".to_string(),
            interactive: false,
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        validate_interactive_stdin(&task, false).unwrap();
    }
}
