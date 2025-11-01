use crate::config::Settings;
use crate::task::Task;
use crate::task::task_output::TaskOutput;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type KeepOrderOutputs = (Vec<(String, String)>, Vec<(String, String)>);

/// Handles task output routing, formatting, and display
pub struct OutputHandler {
    pub keep_order_output: Mutex<IndexMap<Task, KeepOrderOutputs>>,
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

impl OutputHandler {
    pub fn new(
        prefix: bool,
        interleave: bool,
        output: Option<TaskOutput>,
        silent: bool,
        quiet: bool,
        raw: bool,
        is_linear: bool,
        jobs: Option<usize>,
    ) -> Self {
        Self {
            keep_order_output: Mutex::new(IndexMap::new()),
            task_prs: IndexMap::new(),
            timed_outputs: Arc::new(Mutex::new(IndexMap::new())),
            prefix,
            interleave,
            output,
            silent,
            quiet,
            raw,
            is_linear,
            jobs,
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
        } else if let Some(output) = Settings::get().task_output {
            output
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
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
