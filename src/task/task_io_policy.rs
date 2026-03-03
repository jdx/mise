use crate::task::Task;
use crate::task::task_interactive_contract::interactive_non_tty_error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveStream {
    Inherit,
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIoPolicy {
    pub interactive: bool,
    pub raw: bool,
    pub passthrough_stdio: bool,
    pub stdout: InteractiveStream,
    pub stderr: InteractiveStream,
    pub warn_interactive_raw_redundant: bool,
    pub warn_interactive_redactions: bool,
    pub warn_raw_redactions: bool,
}

pub fn build_task_io_policy(
    task: &Task,
    base_raw: bool,
    dry_run: bool,
    stdin_is_tty: bool,
    has_redactions: bool,
) -> Result<TaskIoPolicy, String> {
    if let Some(msg) = interactive_non_tty_error(task, dry_run, stdin_is_tty) {
        return Err(msg);
    }

    let interactive = task.is_interactive();
    let raw = base_raw || interactive;
    let warn_interactive_redactions = interactive && has_redactions;
    let warn_raw_redactions = !interactive && raw && has_redactions;

    Ok(TaskIoPolicy {
        interactive,
        raw,
        passthrough_stdio: interactive,
        stdout: if task.silent.suppresses_stdout() {
            InteractiveStream::Null
        } else {
            InteractiveStream::Inherit
        },
        stderr: if task.silent.suppresses_stderr() {
            InteractiveStream::Null
        } else {
            InteractiveStream::Inherit
        },
        warn_interactive_raw_redundant: interactive && task.raw,
        warn_interactive_redactions,
        warn_raw_redactions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{RunEntry, Silent};

    #[test]
    fn test_build_task_io_policy_interactive_requires_tty() {
        // MatrixRef: S13,N01 / C17
        let task = Task {
            name: "ask".to_string(),
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let err = build_task_io_policy(&task, false, false, false, false).expect_err("must fail");
        assert!(err.contains("requires a TTY on stdin"));
    }

    #[test]
    fn test_build_task_io_policy_interactive_forces_raw_and_passthrough() {
        // MatrixRef: S01,S02,S03,S04,S05,S06,S07,S08 / C2,C6,C9
        let task = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let policy = build_task_io_policy(&task, false, false, true, false).unwrap();
        assert!(policy.raw);
        assert!(policy.passthrough_stdio);
        assert_eq!(policy.stdout, InteractiveStream::Inherit);
        assert_eq!(policy.stderr, InteractiveStream::Inherit);
    }

    #[test]
    fn test_build_task_io_policy_interactive_raw_is_redundant_warning() {
        // MatrixRef: V08 / C9
        let task = Task {
            interactive: Some(true),
            raw: true,
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let policy = build_task_io_policy(&task, false, false, true, false).unwrap();
        assert!(policy.warn_interactive_raw_redundant);
    }

    #[test]
    fn test_build_task_io_policy_redaction_warnings() {
        // MatrixRef: S09,S10 / C14,C16
        let interactive = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let non_interactive_raw = Task {
            raw: true,
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };

        let p1 = build_task_io_policy(&interactive, false, false, true, true).unwrap();
        assert!(p1.warn_interactive_redactions);
        assert!(!p1.warn_raw_redactions);

        let p2 = build_task_io_policy(&non_interactive_raw, true, false, true, true).unwrap();
        assert!(!p2.warn_interactive_redactions);
        assert!(p2.warn_raw_redactions);
    }

    #[test]
    fn test_build_task_io_policy_interactive_stream_suppression() {
        // MatrixRef: C01 / C2,C6
        let mut task = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        task.silent = Silent::Stdout;
        let policy = build_task_io_policy(&task, false, false, true, false).unwrap();
        assert_eq!(policy.stdout, InteractiveStream::Null);
        assert_eq!(policy.stderr, InteractiveStream::Inherit);
    }

    #[test]
    fn test_build_task_io_policy_non_interactive_baseline() {
        // MatrixRef: R05 / C16
        let task = Task {
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        let policy = build_task_io_policy(&task, false, false, true, false).unwrap();
        assert!(!policy.interactive);
        assert!(!policy.passthrough_stdio);
        assert!(!policy.raw);
        assert!(!policy.warn_interactive_raw_redundant);
        assert!(!policy.warn_interactive_redactions);
        assert!(!policy.warn_raw_redactions);
    }
}
