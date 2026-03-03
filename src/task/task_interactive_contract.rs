use crate::task::Task;
use crate::task::task_helpers::{task_is_pure_orchestrator, task_is_runtime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveValidationError {
    RuntimeRequired,
    SilentIncompatible,
}

impl InteractiveValidationError {
    pub fn message(self) -> &'static str {
        match self {
            Self::RuntimeRequired => {
                "interactive=true is only allowed on runtime tasks (script/file/mixed runtime)"
            }
            Self::SilentIncompatible => {
                "interactive=true is incompatible with silent=true|\"stdout\"|\"stderr\""
            }
        }
    }
}

pub fn validate_interactive_task(task: &Task) -> Result<(), InteractiveValidationError> {
    if !task.is_interactive() {
        return Ok(());
    }
    if task_is_pure_orchestrator(task) || !task_is_runtime(task) {
        return Err(InteractiveValidationError::RuntimeRequired);
    }
    if task.silent.is_silent() {
        return Err(InteractiveValidationError::SilentIncompatible);
    }
    Ok(())
}

pub fn interactive_validation_error(task: &Task) -> Option<String> {
    validate_interactive_task(task)
        .err()
        .map(|err| err.message().to_string())
}

pub fn interactive_non_tty_error(task: &Task, dry_run: bool, stdin_is_tty: bool) -> Option<String> {
    if task.is_interactive() && !dry_run && !stdin_is_tty {
        return Some(format!(
            "interactive task '{}' requires a TTY on stdin; got non-TTY stdin",
            task.name
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{RunEntry, Silent};

    #[test]
    fn test_validate_interactive_task_runtime_ok() {
        // MatrixRef: V10 / C7
        let task = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        assert!(validate_interactive_task(&task).is_ok());
    }

    #[test]
    fn test_validate_interactive_task_rejects_orchestrator() {
        // MatrixRef: V11 / C7
        let task = Task {
            interactive: Some(true),
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        };
        assert_eq!(
            validate_interactive_task(&task),
            Err(InteractiveValidationError::RuntimeRequired)
        );
    }

    #[test]
    fn test_validate_interactive_task_rejects_silent() {
        // MatrixRef: V05,V06,V07 / C8
        let task = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("echo hi".to_string())],
            silent: Silent::Stdout,
            ..Default::default()
        };
        assert_eq!(
            validate_interactive_task(&task),
            Err(InteractiveValidationError::SilentIncompatible)
        );
    }

    #[test]
    fn test_interactive_non_tty_error_message() {
        // MatrixRef: S13 / C17
        let task = Task {
            name: "prompt".to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let err = interactive_non_tty_error(&task, false, false).expect("must fail");
        assert!(err.contains("requires a TTY on stdin"));
    }

    #[test]
    fn test_validate_interactive_task_is_noop_when_interactive_false() {
        // MatrixRef: V02,R05 / C15,C16
        let task = Task {
            interactive: None,
            silent: Silent::Bool(true),
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        };
        assert!(validate_interactive_task(&task).is_ok());
    }
}
