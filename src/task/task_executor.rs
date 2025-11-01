use crate::cli::args::ToolArg;
use crate::config::Settings;
use crate::task::Task;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_output_handler::OutputHandler;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Executes tasks with proper context, environment, and output handling
pub struct TaskExecutor {
    pub context_builder: TaskContextBuilder,
    pub output_handler: OutputHandler,
    pub failed_tasks: Arc<Mutex<Vec<(Task, Option<i32>)>>>,

    // CLI flags
    pub force: bool,
    pub cd: Option<PathBuf>,
    pub shell: Option<String>,
    pub tool: Vec<ToolArg>,
    pub timings: bool,
    pub continue_on_error: bool,
}

impl TaskExecutor {
    pub fn new(
        context_builder: TaskContextBuilder,
        output_handler: OutputHandler,
        force: bool,
        cd: Option<PathBuf>,
        shell: Option<String>,
        tool: Vec<ToolArg>,
        timings: bool,
        continue_on_error: bool,
    ) -> Self {
        Self {
            context_builder,
            output_handler,
            failed_tasks: Arc::new(Mutex::new(Vec::new())),
            force,
            cd,
            shell,
            tool,
            timings,
            continue_on_error,
        }
    }

    pub fn is_stopping(&self) -> bool {
        !self.failed_tasks.lock().unwrap().is_empty()
    }

    pub fn add_failed_task(&self, task: Task, status: Option<i32>) {
        let mut failed = self.failed_tasks.lock().unwrap();
        failed.push((task, status.or(Some(1))));
    }

    fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        self.output_handler.eprint(task, prefix, line);
    }

    fn output(&self, task: Option<&Task>) -> crate::task::task_output::TaskOutput {
        self.output_handler.output(task)
    }

    fn quiet(&self, task: Option<&Task>) -> bool {
        self.output_handler.quiet(task)
    }

    fn raw(&self, task: Option<&Task>) -> bool {
        self.output_handler.raw(task)
    }

    pub fn task_timings(&self) -> bool {
        self.timings || Settings::get().task_timings.unwrap_or(false)
    }
}
