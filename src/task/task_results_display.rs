use crate::exit;
use crate::task::task_output::TaskOutput;
use crate::task::task_output_handler::OutputHandler;
use crate::task::{FailedTasks, Task};
use crate::ui::{style, time};

/// Handles display of task execution results and failure summaries
pub struct TaskResultsDisplay {
    output_handler: OutputHandler,
    failed_tasks: FailedTasks,
    continue_on_error: bool,
    show_timings: bool,
}

impl TaskResultsDisplay {
    pub fn new(
        output_handler: OutputHandler,
        failed_tasks: FailedTasks,
        continue_on_error: bool,
        show_timings: bool,
    ) -> Self {
        Self {
            output_handler,
            failed_tasks,
            continue_on_error,
            show_timings,
        }
    }

    /// Display final results and handle failures
    pub fn display_results(&self, num_tasks: usize, timer: std::time::Instant) {
        self.display_keep_order_output();
        self.display_timing_summary(num_tasks, timer);
        self.maybe_print_failure_summary();
        self.exit_if_failed();
    }

    /// Flush any remaining keep-order output (safety net)
    fn display_keep_order_output(&self) {
        if self.output_handler.output(None) != TaskOutput::KeepOrder {
            return;
        }
        self.output_handler
            .keep_order_state
            .lock()
            .unwrap()
            .flush_all();
    }

    /// Display timing summary if enabled
    fn display_timing_summary(&self, num_tasks: usize, timer: std::time::Instant) {
        if self.show_timings && num_tasks > 1 {
            let msg = format!("Finished in {}", time::format_duration(timer.elapsed()));
            eprintln!("{}", style::edim(msg));
        }
    }

    /// Print failure summary if in continue-on-error mode
    fn maybe_print_failure_summary(&self) {
        if !self.continue_on_error {
            return;
        }

        let failed = self.failed_tasks.lock().unwrap().clone();
        if failed.is_empty() {
            return;
        }

        let count = failed.len();
        eprintln!("{} {} task(s) failed:", style::ered("ERROR"), count);
        for (task, status) in &failed {
            let prefix = task.estyled_prefix();
            let status_str = status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            self.eprint(task, &prefix, &format!("exited with status {}", status_str));
        }
    }

    /// Exit if any tasks failed
    fn exit_if_failed(&self) {
        if let Some((task, status)) = self.failed_tasks.lock().unwrap().first() {
            let prefix = task.estyled_prefix();
            self.eprint(
                task,
                &prefix,
                &format!("{} task failed", style::ered("ERROR")),
            );
            exit(status.unwrap_or(1));
        }
    }

    /// Print error message for a task
    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        self.output_handler.eprint(task, prefix, line);
    }
}
