use crate::config::Settings;
use crate::task::Task;
use crate::task::task_helpers::task_needs_permit;
use crate::task::task_output::TaskOutput;
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

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

/// Buffered line for prefix mode debounce flushing.
enum PrefixLine {
    Stdout(String, String), // (prefix, line)
    Stderr(String, String), // (prefix, line)
}

/// Per-task buffered lines and first-enqueue timestamp for prefix mode.
struct PrefixTaskBuffer {
    lines: Vec<PrefixLine>,
    first_pending_at: Instant,
}

/// State for debounced prefix output flushing.
struct PrefixDebounceState {
    buffers: IndexMap<Task, PrefixTaskBuffer>,
}

impl PrefixDebounceState {
    fn new() -> Self {
        Self {
            buffers: IndexMap::new(),
        }
    }

    fn on_stdout(&mut self, task: &Task, prefix: String, line: String) {
        self.push(task, PrefixLine::Stdout(prefix, line));
    }

    fn on_stderr(&mut self, task: &Task, prefix: String, line: String) {
        self.push(task, PrefixLine::Stderr(prefix, line));
    }

    fn push(&mut self, task: &Task, line: PrefixLine) {
        let now = Instant::now();
        let buffer = self
            .buffers
            .entry(task.clone())
            .or_insert_with(|| PrefixTaskBuffer {
                lines: Vec::new(),
                first_pending_at: now,
            });
        if buffer.lines.is_empty() {
            buffer.first_pending_at = now;
        }
        buffer.lines.push(line);
    }

    fn drain_ready(&mut self, debounce: Duration, force: bool) -> Vec<PrefixLine> {
        let now = Instant::now();
        let mut drained = Vec::new();
        let mut pending = IndexMap::new();

        for (task, mut buffer) in std::mem::take(&mut self.buffers) {
            let due = force
                || (!buffer.lines.is_empty()
                    && now.duration_since(buffer.first_pending_at) >= debounce);
            if due {
                drained.append(&mut buffer.lines);
            } else {
                pending.insert(task, buffer);
            }
        }

        self.buffers = pending;
        drained
    }

    fn drain_task(&mut self, task: &Task) -> Vec<PrefixLine> {
        self.buffers
            .shift_remove(task)
            .map(|mut buffer| std::mem::take(&mut buffer.lines))
            .unwrap_or_default()
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
    prefix_debounce_state: Arc<Mutex<PrefixDebounceState>>,
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
            prefix_debounce_state: self.prefix_debounce_state.clone(),
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
            prefix_debounce_state: Arc::new(Mutex::new(PrefixDebounceState::new())),
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
        // In prefix mode, metadata/error messages are printed immediately.
        // Flush this task's pending debounced lines first to preserve ordering.
        self.flush_prefix_task(task);
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

    pub fn on_prefix_stdout(&self, task: &Task, prefix: String, line: String) {
        self.prefix_debounce_state
            .lock()
            .unwrap()
            .on_stdout(task, prefix, line);
    }

    pub fn on_prefix_stderr(&self, task: &Task, prefix: String, line: String) {
        self.prefix_debounce_state
            .lock()
            .unwrap()
            .on_stderr(task, prefix, line);
    }

    pub fn flush_prefix_debounced(&self, debounce: Duration) {
        if self.output(None) != TaskOutput::Prefix {
            return;
        }
        let lines = self
            .prefix_debounce_state
            .lock()
            .unwrap()
            .drain_ready(debounce, false);
        Self::print_prefix_lines(lines);
    }

    pub fn flush_prefix_all(&self) {
        if self.output(None) != TaskOutput::Prefix {
            return;
        }
        let lines = self
            .prefix_debounce_state
            .lock()
            .unwrap()
            .drain_ready(Duration::ZERO, true);
        Self::print_prefix_lines(lines);
    }

    pub fn flush_prefix_task(&self, task: &Task) {
        if self.output(Some(task)) != TaskOutput::Prefix {
            return;
        }
        let lines = self.prefix_debounce_state.lock().unwrap().drain_task(task);
        Self::print_prefix_lines(lines);
    }

    fn print_prefix_lines(lines: Vec<PrefixLine>) {
        for line in lines {
            match line {
                PrefixLine::Stdout(prefix, line) => print_stdout(&prefix, &line),
                PrefixLine::Stderr(prefix, line) => print_stderr(&prefix, &line),
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
    use super::{OutputHandler, OutputHandlerConfig, PrefixDebounceState, PrefixLine};
    use crate::task::Task;
    use crate::task::task_output::TaskOutput;
    use std::time::Duration;

    #[test]
    fn defaults_to_interleave_for_linear_graphs() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: None,
            silent: false,
            quiet: false,
            raw: false,
            is_linear: true,
            jobs: Some(8),
        });

        assert_eq!(handler.output(None), TaskOutput::Interleave);
    }

    #[test]
    fn defaults_to_prefix_for_non_linear_graphs_with_parallel_jobs() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: None,
            silent: false,
            quiet: false,
            raw: false,
            is_linear: false,
            jobs: Some(8),
        });

        // Non-linear graphs with multiple jobs default to prefix output.
        assert_eq!(handler.output(None), TaskOutput::Prefix);
    }

    #[test]
    fn single_job_forces_interleave_even_for_non_linear_graphs() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: None,
            silent: false,
            quiet: false,
            raw: false,
            is_linear: false,
            jobs: Some(1),
        });

        assert_eq!(handler.output(None), TaskOutput::Interleave);
    }

    #[test]
    fn explicit_prefix_output_overrides_linear_default() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: Some(TaskOutput::Prefix),
            silent: false,
            quiet: false,
            raw: false,
            is_linear: true,
            jobs: Some(8),
        });

        // Explicitly requested prefix output must override the linear-graph default.
        assert_eq!(handler.output(None), TaskOutput::Prefix);
    }

    fn task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            display_name: name.to_string(),
            ..Task::default()
        }
    }

    #[test]
    fn prefix_debounce_delays_flush_until_debounce_elapsed() {
        let mut state = PrefixDebounceState::new();
        let task = task("t1");
        state.on_stdout(&task, "t1".to_string(), "line-1".to_string());

        let pending = state.drain_ready(Duration::from_secs(60), false);
        assert!(pending.is_empty());

        let drained = state.drain_ready(Duration::ZERO, false);
        assert_eq!(drained.len(), 1);
        assert!(matches!(
            &drained[0],
            PrefixLine::Stdout(prefix, line) if prefix == "t1" && line == "line-1"
        ));
    }

    #[test]
    fn prefix_debounce_force_flushes_all_buffers_in_task_order() {
        let mut state = PrefixDebounceState::new();
        let t1 = task("t1");
        let t2 = task("t2");

        state.on_stdout(&t1, "t1".to_string(), "a".to_string());
        state.on_stderr(&t1, "t1".to_string(), "b".to_string());
        state.on_stdout(&t2, "t2".to_string(), "c".to_string());

        let drained = state.drain_ready(Duration::from_secs(60), true);
        assert_eq!(drained.len(), 3);
        assert!(matches!(
            &drained[0],
            PrefixLine::Stdout(prefix, line) if prefix == "t1" && line == "a"
        ));
        assert!(matches!(
            &drained[1],
            PrefixLine::Stderr(prefix, line) if prefix == "t1" && line == "b"
        ));
        assert!(matches!(
            &drained[2],
            PrefixLine::Stdout(prefix, line) if prefix == "t2" && line == "c"
        ));
    }

    #[test]
    fn eprint_flushes_pending_prefix_buffers_before_printing_metadata() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: Some(TaskOutput::Prefix),
            silent: false,
            quiet: false,
            raw: false,
            is_linear: false,
            jobs: Some(8),
        });
        let task = task("t1");
        handler.on_prefix_stdout(&task, "t1".to_string(), "line-1".to_string());
        assert_eq!(
            handler.prefix_debounce_state.lock().unwrap().buffers.len(),
            1
        );

        handler.eprint(&task, "t1", "metadata");
        assert!(
            handler
                .prefix_debounce_state
                .lock()
                .unwrap()
                .buffers
                .is_empty()
        );
    }

    #[test]
    fn eprint_flushes_only_current_task_prefix_buffer() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            prefix: false,
            interleave: false,
            output: Some(TaskOutput::Prefix),
            silent: false,
            quiet: false,
            raw: false,
            is_linear: false,
            jobs: Some(8),
        });
        let t1 = task("t1");
        let t2 = task("t2");
        handler.on_prefix_stdout(&t1, "t1".to_string(), "line-1".to_string());
        handler.on_prefix_stdout(&t2, "t2".to_string(), "line-2".to_string());
        assert_eq!(
            handler.prefix_debounce_state.lock().unwrap().buffers.len(),
            2
        );

        handler.eprint(&t1, "t1", "metadata");
        let guard = handler.prefix_debounce_state.lock().unwrap();
        assert!(!guard.buffers.contains_key(&t1));
        assert!(guard.buffers.contains_key(&t2));
    }
}
