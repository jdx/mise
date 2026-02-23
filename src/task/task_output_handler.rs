use crate::config::Settings;
use crate::task::Task;
use crate::task::task_helpers::task_needs_permit;
use crate::task::task_output::TaskOutput;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// A single line of output, tagged by stream.
pub enum KeepOrderLine {
    Stdout(String, String), // (prefix, line)
    Stderr(String, String), // (prefix, line)
}

/// Streaming state for keep-order mode.
///
/// One task at a time is "active" and streams output in real-time.
/// Other tasks buffer their output. When the active task finishes,
/// any already-finished tasks' buffers are flushed, then the next
/// running task with buffered output is promoted to stream live.
pub struct KeepOrderState {
    /// The task whose output is currently being streamed live
    active: Option<Task>,
    /// Buffered output for non-active tasks (insertion order preserved)
    buffers: IndexMap<Task, Vec<KeepOrderLine>>,
    /// Tasks that finished while not active (in order of completion)
    finished: Vec<Task>,
    /// Set after flush_all — further output prints directly
    done: bool,
}

impl KeepOrderState {
    pub fn new() -> Self {
        Self {
            active: None,
            buffers: IndexMap::new(),
            finished: Vec::new(),
            done: false,
        }
    }

    pub fn init_task(&mut self, task: &Task) {
        self.buffers.entry(task.clone()).or_default();
    }

    /// Whether this task should stream live (is active, or is first in
    /// definition order when no task is active yet).
    fn is_active(&self, task: &Task) -> bool {
        if let Some(active) = &self.active {
            active == task
        } else {
            // No active task yet — only the first task in definition order may claim it
            self.buffers.first().map(|(t, _)| t) == Some(task)
        }
    }

    /// Called when a stdout line is produced by a task's process.
    pub fn on_stdout(&mut self, task: &Task, prefix: String, line: String) {
        if self.done || self.is_active(task) {
            self.active = Some(task.clone());
            print_stdout(&prefix, &line);
        } else {
            self.buffers
                .entry(task.clone())
                .or_default()
                .push(KeepOrderLine::Stdout(prefix, line));
        }
    }

    /// Called when a stderr line is produced by a task's process,
    /// or when metadata (command echo, timing) is emitted for a task.
    pub fn on_stderr(&mut self, task: &Task, prefix: String, line: String) {
        if self.done || self.is_active(task) {
            self.active = Some(task.clone());
            print_stderr(&prefix, &line);
        } else {
            self.buffers
                .entry(task.clone())
                .or_default()
                .push(KeepOrderLine::Stderr(prefix, line));
        }
    }

    /// Called when a task finishes execution.
    pub fn on_task_finished(&mut self, task: &Task) {
        if !self.buffers.contains_key(task) {
            return; // Not a keep-order task
        }
        if self.is_active(task) {
            // Active task finished — clear it, flush waiting tasks, promote next
            self.active = None;
            self.buffers.shift_remove(task);
            self.flush_finished();
            self.promote_next();
        } else {
            // Non-active task finished — remember it for later flushing
            self.finished.push(task.clone());
        }
    }

    /// Flush contiguous finished tasks from the front of the buffer.
    /// Stops at the first non-finished task to preserve definition order.
    fn flush_finished(&mut self) {
        let mut finished: std::collections::HashSet<_> = self.finished.drain(..).collect();
        loop {
            let Some((task, _)) = self.buffers.first() else {
                break;
            };
            if !finished.remove(task) {
                break; // Hit a non-finished task, stop
            }
            let task = task.clone();
            if let Some(lines) = self.buffers.shift_remove(&task) {
                Self::print_lines(&lines);
            }
        }
        // Re-add finished tasks we couldn't flush (behind a still-running task)
        self.finished.extend(finished);
    }

    /// Promote the next buffered (still-running) task to active and
    /// flush its current buffer so it can stream live going forward.
    fn promote_next(&mut self) {
        if let Some((task, _)) = self.buffers.first() {
            let task = task.clone();
            self.active = Some(task.clone());
            if let Some(lines) = self.buffers.get_mut(&task) {
                let lines = std::mem::take(lines);
                Self::print_lines(&lines);
            }
        }
    }

    fn print_lines(lines: &[KeepOrderLine]) {
        for line in lines {
            match line {
                KeepOrderLine::Stdout(prefix, line) => print_stdout(prefix, line),
                KeepOrderLine::Stderr(prefix, line) => print_stderr(prefix, line),
            }
        }
    }

    /// Safety-net: flush any remaining output (called at the very end).
    /// After this, any further output prints directly.
    pub fn flush_all(&mut self) {
        self.active = None;
        self.flush_finished();
        for (_, lines) in self.buffers.drain(..) {
            Self::print_lines(&lines);
        }
        self.done = true;
    }
}

fn print_stdout(prefix: &str, line: &str) {
    if console::colors_enabled() {
        prefix_println!(prefix, "{line}\x1b[0m");
    } else {
        prefix_println!(prefix, "{line}");
    }
}

fn print_stderr(prefix: &str, line: &str) {
    if console::colors_enabled_stderr() {
        prefix_eprintln!(prefix, "{line}\x1b[0m");
    } else {
        prefix_eprintln!(prefix, "{line}");
    }
}

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
    pub keep_order_state: Arc<Mutex<KeepOrderState>>,
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
            keep_order_state: self.keep_order_state.clone(),
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
            keep_order_state: Arc::new(Mutex::new(KeepOrderState::new())),
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
                // Only add tasks that produce output (not orchestrator-only tasks)
                if task_needs_permit(task) {
                    self.keep_order_state.lock().unwrap().init_task(task);
                }
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

        // CLI flags (--prefix, --interleave) override config settings
        if self.prefix {
            TaskOutput::Prefix
        } else if self.interleave {
            TaskOutput::Interleave
        } else if let Some(output) = Settings::get().task.output {
            // Silent/quiet from config override raw (output suppression takes precedence)
            // Other modes (prefix, etc.) allow raw to take precedence for stdin/stdout
            if output.is_silent() || output.is_quiet() {
                output
            } else if self.raw(task) {
                TaskOutput::Interleave
            } else {
                output
            }
        } else if self.raw(task) || self.jobs() == 1 || self.is_linear {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
        }
    }

    /// Print error/metadata message for a task.
    /// For keep-order mode, routes through the streaming state so messages
    /// stay ordered with the task's stdout/stderr.
    pub fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        match self.output(Some(task)) {
            TaskOutput::KeepOrder => {
                self.keep_order_state.lock().unwrap().on_stderr(
                    task,
                    prefix.to_string(),
                    line.to_string(),
                );
            }
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
