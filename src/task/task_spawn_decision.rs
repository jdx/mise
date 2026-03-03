use crate::task::Task;
use crate::task::task_helpers::task_needs_permit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnBarrierMode {
    Runtime,
    Interactive,
}

pub fn spawn_barrier_mode(task: &Task) -> Option<SpawnBarrierMode> {
    if !task_needs_permit(task) {
        return None;
    }
    if task.is_interactive() {
        Some(SpawnBarrierMode::Interactive)
    } else {
        Some(SpawnBarrierMode::Runtime)
    }
}

pub fn should_skip_spawn(
    is_stopping: bool,
    continue_on_error: bool,
    is_runnable_post_dep: bool,
) -> bool {
    is_stopping && !continue_on_error && !is_runnable_post_dep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;

    #[test]
    fn test_should_skip_spawn_only_for_non_cleanup_when_stopping() {
        // MatrixRef: B14,B16,F14 / C5,C13
        assert!(should_skip_spawn(true, false, false));
        assert!(!should_skip_spawn(true, false, true));
        assert!(!should_skip_spawn(true, true, false));
        assert!(!should_skip_spawn(false, false, false));
    }

    #[test]
    fn test_spawn_barrier_mode_interactive_runtime_orchestrator() {
        // MatrixRef: B01,B05,B08 / C1,C10,C11
        let interactive = Task {
            interactive: Some(true),
            run: vec![RunEntry::Script("read x".to_string())],
            ..Default::default()
        };
        let runtime = Task {
            run: vec![RunEntry::Script("echo hi".to_string())],
            ..Default::default()
        };
        let orchestrator = Task {
            run: vec![RunEntry::SingleTask {
                task: "build".to_string(),
            }],
            ..Default::default()
        };

        assert_eq!(
            spawn_barrier_mode(&interactive),
            Some(SpawnBarrierMode::Interactive)
        );
        assert_eq!(
            spawn_barrier_mode(&runtime),
            Some(SpawnBarrierMode::Runtime)
        );
        assert_eq!(spawn_barrier_mode(&orchestrator), None);
    }
}
