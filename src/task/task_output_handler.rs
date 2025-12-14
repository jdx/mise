use crate::config::Settings;
use crate::task::Task;
use crate::task::task_output::TaskOutput;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type KeepOrderOutputs = (Vec<(String, String)>, Vec<(String, String)>);

/// Configuration for OutputHandler
pub struct OutputHandlerConfig {
    pub prefix: bool,
    pub interleave: bool,
    pub output: Option<TaskOutput>,
    pub silent: bool,
    pub quiet: bool,
    pub raw: bool,
    pub is_linear: bool,
    pub jobs: Option<usize>,
}

/// Handles task output routing, formatting, and display
pub struct OutputHandler {
    pub keep_order_output: Arc<Mutex<IndexMap<Task, KeepOrderOutputs>>>,
    pub task_prs: IndexMap<Task, Arc<Box<dyn SingleReport>>>,
    pub timed_outputs: Arc<Mutex<IndexMap<String, (SystemTime, String)>>>,

    // Configuration from CLI args
    prefix: bool,
    interleave: bool,
    output: Option<TaskOutput>,
    silent: bool,
    quiet: bool,
    raw: bool,
    is_linear: bool,
    jobs: Option<usize>,
}

impl Clone for OutputHandler {
    fn clone(&self) -> Self {
        Self {
            keep_order_output: self.keep_order_output.clone(),
            task_prs: self.task_prs.clone(),
            timed_outputs: self.timed_outputs.clone(),
            prefix: self.prefix,
            interleave: self.interleave,
            output: self.output,
            silent: self.silent,
            quiet: self.quiet,
            raw: self.raw,
            is_linear: self.is_linear,
            jobs: self.jobs,
        }
    }
}

impl OutputHandler {
    pub fn new(config: OutputHandlerConfig) -> Self {
        Self {
            keep_order_output: Arc::new(Mutex::new(IndexMap::new())),
            task_prs: IndexMap::new(),
            timed_outputs: Arc::new(Mutex::new(IndexMap::new())),
            prefix: config.prefix,
            interleave: config.interleave,
            output: config.output,
            silent: config.silent,
            quiet: config.quiet,
            raw: config.raw,
            is_linear: config.is_linear,
            jobs: config.jobs,
        }
    }

    /// Initialize output handling for a task
    pub fn init_task(&mut self, task: &Task) {
        match self.output(Some(task)) {
            TaskOutput::KeepOrder => {
                self.keep_order_output
                    .lock()
                    .unwrap()
                    .insert(task.clone(), Default::default());
            }
            TaskOutput::Replacing => {
                let pr = MultiProgressReport::get().add(&task.estyled_prefix());
                self.task_prs.insert(task.clone(), Arc::new(pr));
            }
            _ => {}
        }
    }

    /// Determine the output mode for a task
    pub fn output(&self, task: Option<&Task>) -> TaskOutput {
        // Check for full silent mode (both streams)
        // Only Silent::Bool(true) means completely silent, not Silent::Stdout or Silent::Stderr
        if let Some(task_ref) = task
            && matches!(task_ref.silent, crate::task::Silent::Bool(true))
        {
            return TaskOutput::Silent;
        }

        // Check global output settings
        if let Some(o) = self.output {
            return o;
        } else if let Some(task_ref) = task {
            // Fall through to other checks if silent is Off
            if self.silent_bool() {
                return TaskOutput::Silent;
            }
            if self.quiet(Some(task_ref)) {
                return TaskOutput::Quiet;
            }
        } else if self.silent_bool() {
            return TaskOutput::Silent;
        } else if self.quiet(task) {
            return TaskOutput::Quiet;
        }

        if self.prefix {
            TaskOutput::Prefix
        } else if self.interleave {
            TaskOutput::Interleave
        } else if self.raw(task) {
            // raw tasks need interleave for stdin/stdout to work properly
            TaskOutput::Interleave
        } else if let Some(output) = Settings::get().task_output {
            output
        } else if self.jobs() == 1 || self.is_linear {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
        }
    }

    /// Print error message for a task
    pub fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        match self.output(Some(task)) {
            TaskOutput::Replacing => {
                let pr = self.task_prs.get(task).unwrap().clone();
                pr.set_message(format!("{prefix} {line}"));
            }
            _ => {
                prefix_eprintln!(prefix, "{line}");
            }
        }
    }

    fn silent_bool(&self) -> bool {
        self.silent || Settings::get().silent || self.output.is_some_and(|o| o.is_silent())
    }

    pub fn silent(&self, task: Option<&Task>) -> bool {
        self.silent_bool() || task.is_some_and(|t| t.silent.is_silent())
    }

    pub fn quiet(&self, task: Option<&Task>) -> bool {
        self.quiet
            || Settings::get().quiet
            || self.output.is_some_and(|o| o.is_quiet())
            || task.is_some_and(|t| t.quiet)
            || self.silent(task)
    }

    pub fn raw(&self, task: Option<&Task>) -> bool {
        self.raw || Settings::get().raw || task.is_some_and(|t| t.raw)
    }

    pub fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            self.jobs.unwrap_or(Settings::get().jobs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> OutputHandlerConfig {
        OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: None,
            silent: false,
            quiet: false,
            raw: false,
            is_linear: false,
            jobs: None,
        }
    }

    fn raw_task() -> Task {
        Task {
            raw: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_raw_task_gets_interleave_output() {
        // When a task has raw=true, it should get Interleave output mode
        // regardless of other settings (except explicit CLI flags)
        let handler = OutputHandler::new(default_config());
        let task = raw_task();

        assert_eq!(handler.output(Some(&task)), TaskOutput::Interleave);
    }

    #[test]
    fn test_raw_handler_gets_interleave_output() {
        // When the handler itself is configured with raw=true
        let config = OutputHandlerConfig {
            raw: true,
            ..default_config()
        };
        let handler = OutputHandler::new(config);

        assert_eq!(handler.output(None), TaskOutput::Interleave);
    }

    #[test]
    fn test_prefix_flag_overrides_raw() {
        // Explicit --prefix flag should still work (user explicitly requested it)
        let config = OutputHandlerConfig {
            prefix: true,
            ..default_config()
        };
        let handler = OutputHandler::new(config);
        let task = raw_task();

        assert_eq!(handler.output(Some(&task)), TaskOutput::Prefix);
    }

    #[test]
    fn test_interleave_flag_with_raw() {
        // Explicit --interleave flag with raw task should be Interleave
        let config = OutputHandlerConfig {
            interleave: true,
            ..default_config()
        };
        let handler = OutputHandler::new(config);
        let task = raw_task();

        assert_eq!(handler.output(Some(&task)), TaskOutput::Interleave);
    }
}
