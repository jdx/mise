use crate::config::Settings;
use crate::task::task_helpers::{task_needs_permit, task_runs_task_references};
use crate::task::task_output::TaskOutput;
use crate::task::{Task, TaskCacheOutput};
use crate::ui::multi_progress_report::MultiProgressReport;
use crate::ui::progress_report::SingleReport;
use crate::ui::style;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type TaskPrMap = Arc<Mutex<IndexMap<Task, Arc<Box<dyn SingleReport>>>>>;
type TimedOutputMap = Arc<Mutex<IndexMap<String, (SystemTime, Vec<String>)>>>;

/// A single line of output, tagged by stream.
pub(crate) enum KeepOrderLine {
    Stdout(String, String), // (prefix, line)
    Stderr(String, String), // (prefix, line)
}

/// Streaming state for keep-order mode.
///
/// One task at a time is "active" and streams output in real-time.
/// Other tasks buffer their output. When the active task finishes,
/// any already-finished tasks' buffers are flushed, then the next
/// running task with buffered output is promoted to stream live.
pub(crate) struct KeepOrderState {
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
    pub(crate) fn new() -> Self {
        Self {
            active: None,
            buffers: IndexMap::new(),
            finished: Vec::new(),
            done: false,
        }
    }

    pub(crate) fn init_task(&mut self, task: &Task) {
        self.buffers.entry(task.clone()).or_default();
    }

    /// Give `tasks` slots immediately before `parent`'s, keeping their relative
    /// order, so tasks a parent injects at runtime occupy the position the
    /// parent itself was declared in rather than landing wherever their first
    /// line happened to arrive.
    ///
    /// `parent` keeps its own (empty) slot afterwards: a later run entry, or a
    /// nested injection by one of these tasks, has to find the same anchor.
    /// `on_task_finished` reaps it.
    ///
    /// A parent that produces output of its own is left alone. keep-order is one
    /// contiguous block per task and cannot express "parent output, children
    /// blocks, more parent output", and moving such a parent behind its children
    /// would reorder lines it had already buffered. Those tasks keep what this
    /// did before -- appended -- only now in the order they were written rather
    /// than the order they happened to print.
    pub(crate) fn insert_tasks_before(&mut self, parent: &Task, tasks: &[Task]) {
        let anchor = if task_needs_permit(parent) {
            None
        } else {
            self.buffers.get_index_of(parent)
        };
        let Some(mut idx) = anchor else {
            for task in tasks {
                self.init_task(task);
            }
            return;
        };
        for task in tasks {
            // `shift_insert` *moves* a key it already holds, which would drag a
            // task out of the position it was given earlier.
            if self.buffers.contains_key(task) {
                continue;
            }
            self.buffers.shift_insert(idx, task.clone(), Vec::new());
            idx += 1;
        }
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
    pub(crate) fn on_stdout(&mut self, task: &Task, prefix: String, line: String) {
        if self.done || self.is_active(task) {
            self.activate(task);
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
    pub(crate) fn on_stderr(&mut self, task: &Task, prefix: String, line: String) {
        if self.done || self.is_active(task) {
            self.activate(task);
            print_stderr(&prefix, &line);
        } else {
            self.buffers
                .entry(task.clone())
                .or_default()
                .push(KeepOrderLine::Stderr(prefix, line));
        }
    }

    /// Retire the slot of a task the run abandoned before it produced anything.
    ///
    /// Only an empty slot is touched. A slot holding lines belongs to a task
    /// that did produce output, and the normal completion path flushes it in
    /// turn -- removing it here could print it out of order, and if the task is
    /// somehow still running its later lines would be stranded at the tail.
    pub(crate) fn retire_unused_slot(&mut self, task: &Task) {
        // The live task's buffer is empty too -- `activate` drained it on the
        // way in -- so "empty" alone does not mean "produced nothing". Retiring
        // it would hand the stream to another task and strand the rest of its
        // output at the tail of the map.
        if self.active.as_ref() == Some(task) {
            return;
        }
        if self.buffers.get(task).is_some_and(|lines| lines.is_empty()) {
            self.on_task_finished(task);
        }
    }

    /// Called when a task finishes execution.
    pub(crate) fn on_task_finished(&mut self, task: &Task) {
        if !self.buffers.contains_key(task) {
            return; // Not a keep-order task
        }
        if self.is_active(task) {
            // Active task finished — clear it, flush waiting tasks, promote next
            self.active = None;
            // `activate` empties the buffer on the way in, so there should be
            // nothing here. Print whatever is anyway: no removal in this type is
            // allowed to drop lines, and losing output silently is the failure
            // this guards against.
            if let Some(lines) = self.buffers.shift_remove(task) {
                Self::print_lines(&lines);
            }
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
        while let Some((task, _)) = self.buffers.first() {
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

    /// Promote the next buffered (still-running) task to active so it can
    /// stream live going forward.
    fn promote_next(&mut self) {
        // Skip an entry holding no lines. That is an anchor: it never produces
        // output of its own, so activating it would pin the live stream to a
        // task that cannot release it until it finishes -- which is after
        // everything it injected -- and the tasks behind it would buffer to the
        // end of the run. Leaving `active` as None costs nothing, since
        // `is_active` already lets the front entry claim the stream on its next
        // line.
        let next = self
            .buffers
            .first()
            .filter(|(_, lines)| !lines.is_empty())
            .map(|(task, _)| task.clone());
        if let Some(task) = next {
            self.activate(&task);
        }
    }

    /// Make `task` the live one, printing whatever it buffered before it got
    /// here.
    ///
    /// A task reaches this with a non-empty buffer whenever it produced output
    /// before it was eligible to stream: `is_active` only lets the first entry
    /// in `buffers` claim the stream, and a task started from a task reference
    /// is not in `buffers` at all until its own first line creates the entry.
    /// Those buffered lines came *before* the one being printed now, so they
    /// have to go out first — flushing them at finish instead would reorder the
    /// task's own output, and dropping them is how #12238 lost it.
    fn activate(&mut self, task: &Task) {
        self.active = Some(task.clone());
        if let Some(lines) = self.buffers.get_mut(task) {
            let lines = std::mem::take(lines);
            Self::print_lines(&lines);
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
    pub(crate) fn flush_all(&mut self) {
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
pub(crate) struct OutputHandlerConfig {
    pub output: Option<TaskOutput>,
    pub silent: bool,
    pub quiet: bool,
    pub raw: bool,
    pub is_linear: bool,
    pub jobs: Option<usize>,
}

/// Handles task output routing, formatting, and display
pub(crate) struct OutputHandler {
    pub keep_order_state: Arc<Mutex<KeepOrderState>>,
    pub task_prs: TaskPrMap,
    pub timed_outputs: TimedOutputMap,

    // Configuration from CLI args
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
    /// Get or lazily create a progress reporter for a task in Replacing mode.
    pub(crate) fn get_or_init_task_pr(&self, task: &Task) -> Arc<Box<dyn SingleReport>> {
        let mut prs = self.task_prs.lock().unwrap();
        if let Some(pr) = prs.get(task) {
            pr.clone()
        } else {
            let pr = MultiProgressReport::get().add(&task.estyled_prefix());
            let pr = Arc::new(pr);
            prs.insert(task.clone(), pr.clone());
            pr
        }
    }

    /// Return the task prefix's ANSI opening sequence when the selected output
    /// path actually preserves the styled prefix.
    pub(crate) fn task_prefix_color(&self, task: &Task) -> String {
        if self.quiet(Some(task)) {
            return String::new();
        }

        let output = self.output(Some(task));
        if console::colors_enabled_stderr() && displays_colored_task_prefix(output) {
            style::prefix_ansi(&task.display_name)
        } else {
            String::new()
        }
    }
}

// This reports whether the mode renders a styled task label, not whether every
// output line is prefixed. Replacing's text fallback still renders that label.
fn displays_colored_task_prefix(output: TaskOutput) -> bool {
    matches!(
        output,
        TaskOutput::Prefix | TaskOutput::KeepOrder | TaskOutput::Replacing | TaskOutput::Timed
    )
}

impl OutputHandler {
    pub(crate) fn new(config: OutputHandlerConfig) -> Self {
        Self {
            keep_order_state: Arc::new(Mutex::new(KeepOrderState::new())),
            task_prs: Arc::new(Mutex::new(IndexMap::new())),
            timed_outputs: Arc::new(Mutex::new(IndexMap::new())),
            output: config.output,
            silent: config.silent,
            quiet: config.quiet,
            raw: config.raw,
            is_linear: config.is_linear,
            jobs: config.jobs,
        }
    }

    /// Initialize output handling for a task
    pub(crate) fn init_task(&mut self, task: &Task) {
        match self.output(Some(task)) {
            TaskOutput::KeepOrder if task_needs_permit(task) || task_runs_task_references(task) => {
                // Tasks that produce output, plus the ones that inject other
                // tasks: those produce nothing themselves but anchor their
                // children's blocks at their own declared position. A task that
                // only aggregates `depends` gets neither and stays out, so it
                // cannot sit at the front holding the live stream all run.
                self.keep_order_state.lock().unwrap().init_task(task);
            }
            TaskOutput::Replacing => {
                self.get_or_init_task_pr(task);
            }
            _ => {}
        }
    }

    /// Determine the output *style* for a task.
    ///
    /// This resolves the stream-rendering style only (prefix/interleave/…) and
    /// never returns `Quiet`. Verbosity ("quiet") is a separate axis applied via
    /// [`quiet`](Self::quiet) at mise's own metadata print sites, so styles and
    /// quietness combine freely (e.g. `output = "prefix"` + `quiet = true` prints
    /// prefixed task lines with none of mise's own chatter). Full-silent is the
    /// one verbosity level that still shows up here, because it nulls both streams.
    pub(crate) fn output(&self, task: Option<&Task>) -> TaskOutput {
        // Full-silent (null BOTH streams) is terminal. This must stay distinct
        // from *partial* per-task silent (`silent = "stdout"`/`"stderr"`), which
        // falls through to a real style and is nulled per-stream in the executor —
        // hence the explicit `Silent::Bool(true)` check rather than `silent(task)`.
        //
        // The global `task.output = "silent"` setting is deliberately NOT part of
        // this guard: it's a *style default* and must be overridable by a per-task
        // `output` field (step 2). When there is no override it is still honored by
        // step 3, which maps it back to `Silent` via `style_with_raw`.
        let full_silent = self.silent
            || Settings::get().silent
            || self.output.is_some_and(|o| o.is_silent())
            || task.is_some_and(|t| matches!(t.silent, crate::task::Silent::Bool(true)))
            || task.is_some_and(|t| t.output == Some(TaskOutput::Silent));
        if full_silent {
            return TaskOutput::Silent;
        }

        // Resolve a STYLE only, in precedence order. `Quiet` values map to
        // `Interleave` (their historical stream behavior); the quiet-ness is kept
        // by the `quiet()` predicate independently.
        // 1. CLI `--output` / `MISE_TASK_OUTPUT`
        if let Some(o) = self.output {
            return o.style_with_raw(self.raw(task));
        }
        // 2. per-task `output` style field
        if let Some(o) = task.and_then(|t| t.output) {
            return o.style_with_raw(self.raw(task));
        }
        // 3. global `task.output` setting (raw downgrades non-suppression styles)
        if let Some(o) = Settings::get().task.output {
            return o.style_with_raw(self.raw(task));
        }
        // 4. defaults
        if self.raw(task) || self.jobs() == 1 || self.is_linear {
            TaskOutput::Interleave
        } else {
            TaskOutput::Prefix
        }
    }

    /// Print error/metadata message for a task.
    /// For keep-order mode, routes through the streaming state so messages
    /// stay ordered with the task's stdout/stderr.
    pub(crate) fn eprint(&self, task: &Task, prefix: &str, line: &str) {
        match self.output(Some(task)) {
            TaskOutput::KeepOrder => {
                self.keep_order_state.lock().unwrap().on_stderr(
                    task,
                    prefix.to_string(),
                    line.to_string(),
                );
            }
            TaskOutput::Replacing => {
                let pr = self.get_or_init_task_pr(task);
                pr.set_message(format!("{prefix} {line}"));
            }
            _ => {
                prefix_eprintln!(prefix, "{line}");
            }
        }
    }

    /// Replay cached task output through the currently selected output style.
    pub(crate) fn replay_cached_output(
        &self,
        task: &Task,
        prefix: &str,
        output: &[TaskCacheOutput],
    ) {
        let mode = self.output(Some(task));
        if mode == TaskOutput::Timed && !task.silent.suppresses_stdout() {
            let stdout = output
                .iter()
                .filter_map(|line| match line {
                    TaskCacheOutput::Stdout(line) => Some(line.clone()),
                    TaskCacheOutput::Stderr(_) => None,
                })
                .collect::<Vec<_>>();
            if !stdout.is_empty() {
                self.timed_outputs
                    .lock()
                    .unwrap()
                    .insert(prefix.to_string(), (SystemTime::now(), stdout));
            }
        }
        for line in output {
            match line {
                TaskCacheOutput::Stdout(_) if task.silent.suppresses_stdout() => continue,
                TaskCacheOutput::Stderr(_) if task.silent.suppresses_stderr() => continue,
                _ => {}
            }
            match (mode, line) {
                (TaskOutput::Silent, _) => {}
                (TaskOutput::Prefix, TaskCacheOutput::Stdout(line)) => {
                    print_stdout(prefix, line);
                }
                (TaskOutput::Prefix, TaskCacheOutput::Stderr(line))
                | (TaskOutput::Timed, TaskCacheOutput::Stderr(line)) => {
                    print_stderr(prefix, line);
                }
                (TaskOutput::KeepOrder, TaskCacheOutput::Stdout(line)) => {
                    self.keep_order_state.lock().unwrap().on_stdout(
                        task,
                        prefix.to_string(),
                        line.clone(),
                    );
                }
                (TaskOutput::KeepOrder, TaskCacheOutput::Stderr(line)) => {
                    self.keep_order_state.lock().unwrap().on_stderr(
                        task,
                        prefix.to_string(),
                        line.clone(),
                    );
                }
                (TaskOutput::Replacing, TaskCacheOutput::Stdout(line)) => {
                    if !line.trim().is_empty() {
                        self.get_or_init_task_pr(task).set_message(line.clone());
                    }
                }
                (TaskOutput::Replacing, TaskCacheOutput::Stderr(line)) => {
                    if !line.trim().is_empty() {
                        self.get_or_init_task_pr(task).println(line.clone());
                    }
                }
                (TaskOutput::Timed, TaskCacheOutput::Stdout(_)) => {}
                (TaskOutput::Interleave | TaskOutput::Quiet, TaskCacheOutput::Stdout(line)) => {
                    if console::colors_enabled() {
                        println!("{line}\x1b[0m");
                    } else {
                        println!("{line}");
                    }
                }
                (TaskOutput::Interleave | TaskOutput::Quiet, TaskCacheOutput::Stderr(line)) => {
                    if console::colors_enabled_stderr() {
                        eprintln!("{line}\x1b[0m");
                    } else {
                        eprintln!("{line}");
                    }
                }
            }
        }
    }

    fn silent_bool(&self) -> bool {
        self.silent
            || Settings::get().silent
            || self.output.is_some_and(|o| o.is_silent())
            || Settings::get().task.output.is_some_and(|o| o.is_silent())
    }

    pub(crate) fn silent(&self, task: Option<&Task>) -> bool {
        self.silent_bool()
            || task.is_some_and(|t| t.silent.is_silent())
            || task.is_some_and(|t| t.output.is_some_and(|o| o.is_silent()))
    }

    pub(crate) fn quiet(&self, task: Option<&Task>) -> bool {
        self.quiet
            || Settings::get().quiet
            || self.output.is_some_and(|o| o.is_quiet())
            || Settings::get().task.output.is_some_and(|o| o.is_quiet())
            || task.is_some_and(|t| t.quiet)
            || task.is_some_and(|t| t.output.is_some_and(|o| o.is_quiet()))
            || self.silent(task)
    }

    pub(crate) fn raw(&self, task: Option<&Task>) -> bool {
        // Interactive tasks are treated as raw for I/O (stdin/stdout/stderr inherit).
        // This means CmdLineRunner will also acquire its internal RAW_LOCK — that's
        // intentional and harmless since TASK_RUNTIME_LOCK already provides exclusivity.
        self.raw || Settings::get().raw || task.is_some_and(|t| t.raw || t.interactive)
    }

    pub(crate) fn jobs(&self) -> usize {
        if self.raw {
            1
        } else {
            crate::jobs::resolve(Settings::get().jobs, self.jobs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::RunEntry;

    #[test]
    fn colored_prefix_modes_require_a_rendered_style() {
        for output in [
            TaskOutput::Prefix,
            TaskOutput::KeepOrder,
            TaskOutput::Replacing,
            TaskOutput::Timed,
        ] {
            assert!(displays_colored_task_prefix(output));
        }
        for output in [
            TaskOutput::Interleave,
            TaskOutput::Quiet,
            TaskOutput::Silent,
        ] {
            assert!(!displays_colored_task_prefix(output));
        }
    }

    fn task_named(name: &str) -> Task {
        Task {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn buffered(state: &KeepOrderState, task: &Task) -> usize {
        state.buffers.get(task).map(Vec::len).unwrap_or(0)
    }

    fn keys(state: &KeepOrderState) -> Vec<String> {
        state.buffers.keys().map(|t| t.name.clone()).collect()
    }

    #[test]
    fn activating_a_task_flushes_what_it_buffered() {
        // A task started from a task reference is not in `buffers`, so its first
        // line is held; from the second on it is `buffers.first()` and streams
        // live. Those held lines came first and must go out with it, not be
        // stranded for `on_task_finished` to drop (#12238).
        let mut state = KeepOrderState::new();
        let task = task_named("one");

        state.on_stdout(&task, "one".into(), "first".into());
        assert_eq!(buffered(&state, &task), 1, "first line should be held");

        state.on_stdout(&task, "one".into(), "second".into());
        assert_eq!(
            buffered(&state, &task),
            0,
            "becoming active must flush what was held"
        );
    }

    #[test]
    fn activating_through_stderr_flushes_too() {
        // stderr carries the command echo and timing lines, and reaches the same
        // branch, so it has to flush on the way in as well.
        let mut state = KeepOrderState::new();
        let task = task_named("one");

        state.on_stderr(&task, "one".into(), "first".into());
        assert_eq!(buffered(&state, &task), 1);

        state.on_stderr(&task, "one".into(), "second".into());
        assert_eq!(buffered(&state, &task), 0);
    }

    #[test]
    fn cached_timed_output_preserves_all_stdout_lines() {
        let handler = OutputHandler::new(OutputHandlerConfig {
            output: Some(TaskOutput::Timed),
            silent: false,
            quiet: false,
            raw: false,
            is_linear: true,
            jobs: None,
        });
        let task = Task::default();

        handler.replay_cached_output(
            &task,
            "build",
            &[
                TaskCacheOutput::Stdout("first".into()),
                TaskCacheOutput::Stdout("second".into()),
            ],
        );

        let outputs = handler.timed_outputs.lock().unwrap();
        assert_eq!(outputs.get("build").unwrap().1, ["first", "second"]);
    }

    #[test]
    fn injected_tasks_take_the_parents_slot() {
        // A task started from a run entry is not in the up-front task graph, so
        // without an anchor its block lands wherever its first line happened to
        // arrive rather than where the parent was declared (#12238).
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let other = task_named("other");
        state.init_task(&launch);
        state.init_task(&other);

        state.insert_tasks_before(&launch, &[task_named("one"), task_named("two")]);

        assert_eq!(keys(&state), ["one", "two", "launch", "other"]);
    }

    #[test]
    fn a_task_declared_after_the_parent_cannot_stream_ahead_of_the_children() {
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let other = task_named("other");
        state.init_task(&launch);
        state.init_task(&other);
        state.insert_tasks_before(&launch, &[task_named("one"), task_named("two")]);

        state.on_stdout(&other, "other".into(), "first".into());
        state.on_stdout(&other, "other".into(), "second".into());

        assert_eq!(
            buffered(&state, &other),
            2,
            "a task behind the injected ones must still be held"
        );
        assert_eq!(keys(&state)[0], "one");
    }

    #[test]
    fn a_second_injection_reuses_the_parent_anchor() {
        // Consuming the anchor on the first injection would leave the second run
        // entry with nothing to anchor to, and it would append behind whatever
        // was declared after the parent.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        state.init_task(&launch);

        state.insert_tasks_before(&launch, &[task_named("a")]);
        state.insert_tasks_before(&launch, &[task_named("b")]);

        assert_eq!(keys(&state), ["a", "b", "launch"]);
    }

    #[test]
    fn a_task_that_already_has_a_slot_is_not_moved() {
        // `shift_insert` moves a key it already holds, which would drag a task
        // out of the position it was given earlier.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let a = task_named("a");
        state.init_task(&launch);
        state.init_task(&a);

        state.insert_tasks_before(&launch, &[a.clone(), task_named("b")]);

        assert_eq!(keys(&state), ["b", "launch", "a"]);
    }

    #[test]
    fn a_parent_with_output_of_its_own_is_not_used_as_an_anchor() {
        // The mixed case: keep-order cannot express "parent output, children,
        // more parent output", and anchoring here would move lines the parent
        // had already buffered behind its children's blocks.
        let mut state = KeepOrderState::new();
        let mixed = Task {
            name: "mixed".to_string(),
            run: vec![RunEntry::Script("echo A".to_string())],
            ..Default::default()
        };
        state.init_task(&mixed);

        state.insert_tasks_before(&mixed, &[task_named("one"), task_named("two")]);

        assert_eq!(keys(&state), ["mixed", "one", "two"]);
    }

    #[test]
    fn retiring_a_task_that_never_ran_releases_the_ones_behind_it() {
        // The scheduler drops tasks on teardown without ever spawning them, so
        // nothing else reports them as finished. An anchor left behind that way
        // would hold the front of the map and keep everything after it buffered
        // until the final flush.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let other = task_named("other");
        state.init_task(&launch);
        state.init_task(&other);

        state.on_stdout(&other, "other".into(), "held".into());
        assert_eq!(
            buffered(&state, &other),
            1,
            "held while the anchor is ahead"
        );

        state.on_task_finished(&launch);

        assert_eq!(keys(&state), ["other"]);
        assert!(
            state.is_active(&other),
            "and streams once the anchor is gone"
        );
    }

    #[test]
    fn a_task_held_behind_an_anchor_flushes_when_the_anchor_finishes() {
        // `promote_next` leaves `active` unset when the front slot is an empty
        // anchor, so nothing streams while the anchor is there. The anchor's own
        // completion has to release what queued up behind it rather than leaving
        // it for `flush_all`: with `active` unset, `is_active` is true for the
        // front entry, so the anchor takes the active path and flushes.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let other = task_named("other");
        state.init_task(&launch);
        state.init_task(&other);

        state.on_stdout(&other, "other".into(), "held".into());
        assert_eq!(buffered(&state, &other), 1);
        assert!(
            state.active.is_none(),
            "an empty anchor must not be promoted"
        );

        state.on_task_finished(&launch);

        assert_eq!(keys(&state), ["other"], "the anchor is gone");
        assert_eq!(
            buffered(&state, &other),
            0,
            "and what queued behind it has been printed"
        );
    }

    #[test]
    fn retiring_a_slot_never_touches_the_live_task() {
        // An active task holds an empty buffer, so a retirement that keyed only
        // off emptiness would take the stream away from a task that is still
        // writing to it.
        let mut state = KeepOrderState::new();
        let a = task_named("a");
        let other = task_named("other");
        state.init_task(&a);
        state.init_task(&other);

        state.on_stdout(&a, "a".into(), "line".into());
        assert_eq!(buffered(&state, &a), 0, "the live task buffers nothing");

        state.retire_unused_slot(&a);

        assert_eq!(keys(&state), ["a", "other"], "its slot must survive");
        assert!(state.is_active(&a), "and it keeps the stream");
    }

    #[test]
    fn an_unanchored_parent_falls_back_to_appending() {
        let mut state = KeepOrderState::new();
        let other = task_named("other");
        state.init_task(&other);

        state.insert_tasks_before(&task_named("unregistered"), &[task_named("one")]);

        assert_eq!(keys(&state), ["other", "one"]);
    }

    #[test]
    fn an_empty_anchor_does_not_pin_the_live_stream() {
        // An anchor holds no lines and never produces any. Promoting it would
        // pin the live slot until the parent finishes — which is after every
        // task it injects — so the next child would buffer to the end of the run
        // instead of streaming.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let a = task_named("a");
        state.init_task(&a);
        state.init_task(&launch);

        state.on_stdout(&a, "a".into(), "line".into());
        state.on_task_finished(&a);
        assert!(
            state.active.is_none(),
            "the anchor must not have claimed the stream"
        );

        let b = task_named("b");
        state.insert_tasks_before(&launch, std::slice::from_ref(&b));
        assert!(state.is_active(&b), "the next child must stream live");
    }
}
