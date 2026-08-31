use std::collections::{HashMap, HashSet};

use crate::config::Settings;
use crate::task::task_helpers::{task_gets_keep_order_slot, task_needs_permit};
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
    /// Which task injected each task that a run entry placed in the group.
    ///
    /// Usually that is a task given its slot at runtime, but a run entry can
    /// also name a task that already holds one — one being run at top level as
    /// well. It keeps that slot, and the slot joins the group all the same, so
    /// the parent's next entry lands past it rather than in front of it.
    ///
    /// A printing parent's children go *after* it, so a second run entry has to
    /// land after the first group rather than at the parent's heels — otherwise
    /// the later group ends up in front and the order reverses. Finding that
    /// boundary means walking forward past everything belonging to the parent,
    /// and neither a count of its direct children nor "was this injected at
    /// all?" describes that: a printing child injects slots that belong inside
    /// its parent's group, while a printing *sibling* injects slots that do not.
    /// Telling those apart needs the ancestry, so it is recorded.
    ///
    /// The anchored path does not need the boundary — its children go *before*
    /// the parent, which pushes the parent along, so re-reading its index
    /// already lands past the previous group — but it records ancestry too, so
    /// that a printing child of an anchored parent can find its own.
    ///
    /// A task can have **more than one** parent here. Nothing stops two run
    /// entries from naming the same task, and a slot that already exists is
    /// adopted rather than moved, so it genuinely belongs to both groups. Keeping
    /// only the first parent left the second one unable to see it, and that
    /// parent's later entry landed in front of a task its earlier entry had
    /// named.
    injected_by: HashMap<Task, Vec<Task>>,
    /// Set after flush_all — further output prints directly
    done: bool,
}

impl KeepOrderState {
    pub(crate) fn new() -> Self {
        Self {
            active: None,
            buffers: IndexMap::new(),
            finished: Vec::new(),
            injected_by: HashMap::new(),
            done: false,
        }
    }

    pub(crate) fn init_task(&mut self, task: &Task) {
        self.buffers.entry(task.clone()).or_default();
    }

    /// Give `tasks` slots at `parent`'s position, keeping their relative order,
    /// so tasks a parent injects at runtime occupy the place the parent itself
    /// was declared in rather than landing wherever their first line happened to
    /// arrive.
    ///
    /// `parent` keeps its own slot afterwards: a later run entry, or a nested
    /// injection by one of these tasks, has to find the same anchor.
    /// `on_task_finished` reaps it when it stayed empty.
    ///
    /// Which side of the parent depends on whether the parent prints. One that
    /// prints nothing is only an anchor, so its children take its position. One
    /// that prints keeps its own block ahead of them: moving it behind its
    /// children would reorder lines it had already buffered, which is why an
    /// earlier version of this gave such a parent no anchor at all and appended
    /// instead. Appending was not enough — the position then came from whichever
    /// parent injected first, so with two printing parents the blocks came out in
    /// a different order from one run to the next. Inserting after the parent
    /// keeps its buffered lines where they were *and* takes the scheduler back
    /// out of the answer.
    ///
    /// What is still not expressible is "parent output, children, more parent
    /// output": keep-order is one contiguous block per task, so a printing
    /// parent's later lines land in its own block, ahead of the children.
    pub(crate) fn insert_injected_tasks(&mut self, parent: &Task, tasks: &[Task]) {
        let Some(parent_idx) = self.buffers.get_index_of(parent) else {
            for task in tasks {
                self.init_task(task);
            }
            return;
        };
        let after = task_needs_permit(parent);
        let mut idx = if after {
            self.group_end(parent, parent_idx)
        } else {
            parent_idx
        };
        for task in tasks {
            // `shift_insert` *moves* a key it already holds, which would drag a
            // task out of the position it was given earlier — from a previous
            // injection, or from the up-front registration of a task that is
            // also being run at top level. It keeps that slot; two things have
            // to happen around it instead.
            //
            // `idx` moves past it, so the entries written *after* it stay after
            // it — leaving `idx` alone put them in front, which reversed the
            // order the run entry was written in. Only on the side that inserts
            // *after* the parent: the anchored path's insertion point is the
            // parent's own slot, and a task holding a slot behind the parent
            // would push `idx` past it, sending the rest of the group out of the
            // anchor and behind the parent it was supposed to precede.
            //
            // And the slot joins the parent's subtree, because `group_end` stops
            // at the first slot that is not the parent's. Without that, a task
            // that already had a slot ended the walk where it stood, and the
            // parent's *next* run entry landed in front of the group this one
            // left: `[p, d, c1, c2]` for entries written `[c1, c2]` then `[d]`.
            if let Some(existing) = self.buffers.get_index_of(task) {
                if after && existing >= idx {
                    idx = existing + 1;
                }
                // Recorded on both sides, unlike the `idx` move above. Which
                // side the parent inserts on says nothing about whose subtree
                // the slot belongs to, and an anchored parent's child can go on
                // to inject a subtree of its own — one that an *outer* printing
                // parent then has to walk past. Gating this on `after` left that
                // edge missing, and the outer `group_end` stopped at the child.
                //
                // Skipped when `parent` already descends from `task`: that edge
                // would close the chain `descends_from` walks into a loop. This
                // is the bound that gating on `after` was standing in for.
                if !self.descends_from(parent, task) {
                    let parents = self.injected_by.entry(task.clone()).or_default();
                    if !parents.contains(parent) {
                        parents.push(parent.clone());
                    }
                }
                continue;
            }
            self.buffers.shift_insert(idx, task.clone(), Vec::new());
            self.injected_by.insert(task.clone(), vec![parent.clone()]);
            idx += 1;
        }
    }

    /// The slot just past `parent`'s own subtree.
    ///
    /// What a parent inserts sits contiguously after it, so the group is
    /// normally an unbroken run. It is not always: a task that already held a
    /// slot keeps it, and that slot can sit beyond an unrelated one — a
    /// top-level task registered between them, say. Adopting it into the subtree
    /// without reaching it would put the parent's next entry in front of a task
    /// its run entry named *first*.
    ///
    /// So this looks for the **last** slot descended from `parent` rather than
    /// stopping at the first that is not. An unrelated slot caught inside that
    /// span was already there before the parent injected anything, and leaving
    /// it where it is costs nothing; ending the group short of a task the parent
    /// owns reverses the order the run entries were written in.
    fn group_end(&self, parent: &Task, parent_idx: usize) -> usize {
        let mut end = parent_idx + 1;
        for idx in parent_idx + 1..self.buffers.len() {
            if self
                .buffers
                .get_index(idx)
                .is_some_and(|(task, _)| self.descends_from(task, parent))
            {
                end = idx + 1;
            }
        }
        end
    }

    /// Whether `task` was injected by `ancestor`, directly or through a chain of
    /// injections.
    ///
    /// The walk is bounded by the number of recorded edges: a chain longer than
    /// that has returned to a slot it already passed. Injection alone cannot
    /// build one — a task is recorded under the parent that placed it, which was
    /// already there — but a task that also holds a slot of its own can be
    /// recorded on either side of a pair whose run entries name each other, and
    /// a hung run is a far worse answer than one misordered group.
    fn descends_from(&self, task: &Task, ancestor: &Task) -> bool {
        // A walk over a graph rather than a chain, since a task can be named by
        // several run entries. `seen` is what keeps it terminating: two entries
        // naming each other can be recorded from both ends, and the ordering may
        // then be imperfect, but the run must not hang.
        let mut seen: HashSet<&Task> = HashSet::new();
        let mut stack = vec![task];
        while let Some(current) = stack.pop() {
            if !seen.insert(current) {
                continue;
            }
            let Some(parents) = self.injected_by.get(current) else {
                continue;
            };
            for parent in parents {
                if parent == ancestor {
                    return true;
                }
                stack.push(parent);
            }
        }
        false
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
            // See `task_gets_keep_order_slot` for which tasks get a slot and why.
            // The executor applies the same rule to the tasks a run entry
            // injects, so both paths agree on who is in the buffer map.
            TaskOutput::KeepOrder if task_gets_keep_order_slot(task) => {
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

        state.insert_injected_tasks(&launch, &[task_named("one"), task_named("two")]);

        assert_eq!(keys(&state), ["one", "two", "launch", "other"]);
    }

    #[test]
    fn a_task_declared_after_the_parent_cannot_stream_ahead_of_the_children() {
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let other = task_named("other");
        state.init_task(&launch);
        state.init_task(&other);
        state.insert_injected_tasks(&launch, &[task_named("one"), task_named("two")]);

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

        state.insert_injected_tasks(&launch, &[task_named("a")]);
        state.insert_injected_tasks(&launch, &[task_named("b")]);

        assert_eq!(keys(&state), ["a", "b", "launch"]);
    }

    #[test]
    fn a_task_that_already_has_a_slot_is_not_moved() {
        // `shift_insert` moves a key it already holds, which would drag a task
        // out of the position it was given earlier.
        //
        // The anchored path does not step past that slot the way the printing
        // one does: its insertion point *is* the parent's slot, so stepping past
        // a task sitting behind the parent would send `b` out of the anchor and
        // behind the parent it is meant to precede.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        let a = task_named("a");
        state.init_task(&launch);
        state.init_task(&a);

        state.insert_injected_tasks(&launch, &[a.clone(), task_named("b")]);

        assert_eq!(keys(&state), ["b", "launch", "a"]);
    }

    #[test]
    fn a_parent_that_prints_keeps_its_block_ahead_of_its_children() {
        // The mixed case: keep-order cannot express "parent output, children,
        // more parent output", so the parent's own block stays in front. Putting
        // the children *before* it would move lines it had already buffered.
        let mut state = KeepOrderState::new();
        // `head` goes first so the parent is not the one streaming live —
        // otherwise `is_active` is true for it and its line prints straight out
        // instead of landing in the block this is about.
        let head = task_named("head");
        let mixed = printing_task("mixed");
        state.init_task(&head);
        state.init_task(&mixed);
        state.on_stdout(&mixed, "mixed".into(), "start".into());

        state.insert_injected_tasks(&mixed, &[task_named("one"), task_named("two")]);

        assert_eq!(keys(&state), ["head", "mixed", "one", "two"]);
        assert_eq!(
            buffered(&state, &mixed),
            1,
            "the parent's line must still be in the parent's block"
        );
    }

    /// A printing parent's children go *after* it, so a second run entry must
    /// land after the first group. Anchoring each call at the parent's heels
    /// would put the later group in front and reverse what was written.
    ///
    /// The anchored path gets this for free — its children go before the parent,
    /// which pushes the parent along — which is why only this side needs
    /// `group_end`.
    #[test]
    fn a_second_injection_by_a_printing_parent_follows_the_first() {
        let mut state = KeepOrderState::new();
        let mixed = printing_task("mixed");
        state.init_task(&mixed);

        state.insert_injected_tasks(&mixed, &[task_named("first")]);
        state.insert_injected_tasks(&mixed, &[task_named("second")]);

        assert_eq!(keys(&state), ["mixed", "first", "second"]);
    }

    /// A printing child can inject in turn, and those slots belong inside its
    /// parent's group. Counting the parent's direct children misses them, so a
    /// later run entry from the parent lands in front of them: `p, c, d, x`
    /// instead of `p, c, x, d`.
    #[test]
    fn a_nested_injection_stays_inside_the_group_it_belongs_to() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let c = printing_task("c");
        state.init_task(&p);

        state.insert_injected_tasks(&p, std::slice::from_ref(&c));
        state.insert_injected_tasks(&c, &[task_named("x")]);
        state.insert_injected_tasks(&p, &[task_named("d")]);

        assert_eq!(keys(&state), ["p", "c", "x", "d"]);
    }

    /// A parent's group ends where a *sibling's* subtree begins. Walking past
    /// everything merely "injected" cannot see that boundary: it steps over the
    /// sibling too and drops the nested task on the far side of it, giving
    /// `[p, p1, p2, q]` where `q` belongs between `p1` and `p2`.
    #[test]
    fn a_sibling_subtree_is_not_swallowed_by_the_one_before_it() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let p1 = printing_task("p1");
        let p2 = printing_task("p2");
        state.init_task(&p);

        state.insert_injected_tasks(&p, &[p1.clone(), p2.clone()]);
        state.insert_injected_tasks(&p1, &[task_named("q")]);

        assert_eq!(keys(&state), ["p", "p1", "q", "p2"]);

        // ...and the parent's own next entry still goes after both subtrees.
        state.insert_injected_tasks(&p, &[task_named("d")]);
        assert_eq!(keys(&state), ["p", "p1", "q", "p2", "d"]);
    }

    /// A run entry can name a task that already holds a slot — one that is being
    /// run at top level as well. That task keeps the slot it was given, but the
    /// entries written after it still have to land behind it. Skipping it
    /// without moving the insertion point put them in front instead, so the
    /// group came out in the reverse of what the entry said: `[p, c2, c1]`.
    #[test]
    fn a_child_that_already_has_a_slot_does_not_send_its_siblings_in_front() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let c1 = task_named("c1");
        let c2 = task_named("c2");
        state.init_task(&p);
        state.init_task(&c1);

        state.insert_injected_tasks(&p, &[c1.clone(), c2.clone()]);

        assert_eq!(keys(&state), ["p", "c1", "c2"]);
    }

    /// ...and the parent's *next* run entry has to clear it too. `group_end`
    /// stops at the first slot that is not the parent's, so a task that kept a
    /// slot of its own ended the walk where it stood and the later group landed
    /// in front of the earlier one: `[p, d, c1, c2]` for `[c1, c2]` then `[d]`.
    #[test]
    fn a_pre_slotted_child_still_belongs_to_the_group_it_joined() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let c1 = task_named("c1");
        state.init_task(&p);
        state.init_task(&c1);

        state.insert_injected_tasks(&p, &[c1.clone(), task_named("c2")]);
        state.insert_injected_tasks(&p, &[task_named("d")]);

        assert_eq!(keys(&state), ["p", "c1", "c2", "d"]);
    }

    /// The same slot, joined from the *anchored* side. A printing task injects a
    /// parent that prints nothing, that parent adopts a pre-slotted printing
    /// child, and the child then injects a subtree of its own. Which side the
    /// adopting parent inserts on says nothing about whose subtree the slot
    /// belongs to — but recording ancestry only on the `after` side left this
    /// edge missing, so the *outer* `group_end` stopped at the child and the
    /// outer parent's next entry landed in front of the child's subtree:
    /// `[p, q, d, c, x]` for entries written `[q]`, `[c]`, `[x]`, then `[d]`.
    #[test]
    fn a_slot_adopted_by_an_anchored_parent_still_joins_the_outer_group() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let q = task_named("q");
        let c = printing_task("c");
        state.init_task(&p);
        state.init_task(&c);

        state.insert_injected_tasks(&p, std::slice::from_ref(&q));
        state.insert_injected_tasks(&q, std::slice::from_ref(&c));
        state.insert_injected_tasks(&c, &[task_named("x")]);
        state.insert_injected_tasks(&p, &[task_named("d")]);

        assert_eq!(keys(&state), ["p", "q", "c", "x", "d"]);
    }

    /// A pre-slotted task's slot is wherever it already was, which need not be
    /// next to the parent adopting it. With an unrelated top-level task sitting
    /// between them, ending the group at the first slot that is not the
    /// parent's stopped short of a task the parent's *first* run entry named,
    /// and the second entry landed in front of it: `[p, d, s, c]` for entries
    /// written `[c]` then `[d]`.
    #[test]
    fn a_pre_slotted_child_beyond_an_unrelated_slot_still_precedes_the_next_entry() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let s = task_named("s");
        let c = task_named("c");
        state.init_task(&p);
        state.init_task(&s);
        state.init_task(&c);

        state.insert_injected_tasks(&p, std::slice::from_ref(&c));
        state.insert_injected_tasks(&p, &[task_named("d")]);

        // `s` keeps the slot it was registered in; what matters is that `c`,
        // which `p` named first, still comes before `d`.
        assert_eq!(keys(&state), ["p", "s", "c", "d"]);
    }

    /// Two run entries can name the same task, and the slot is adopted rather
    /// than moved — so it belongs to both groups at once. Keeping only the first
    /// parent left the second one blind to it: `group_end` stopped short and the
    /// second parent's later entry landed in front of a task its own earlier
    /// entry had named, giving `[p, q, d, c]` for `q`'s entries `[c]` then `[d]`.
    #[test]
    fn a_slot_named_by_two_parents_belongs_to_both_groups() {
        let mut state = KeepOrderState::new();
        let p = printing_task("p");
        let q = printing_task("q");
        let c = task_named("c");
        state.init_task(&p);
        state.init_task(&q);
        state.init_task(&c);

        state.insert_injected_tasks(&p, std::slice::from_ref(&c));
        state.insert_injected_tasks(&q, std::slice::from_ref(&c));
        state.insert_injected_tasks(&q, &[task_named("d")]);

        assert_eq!(keys(&state), ["p", "q", "c", "d"]);
    }

    /// Recording ancestry for a task that already had a slot is what makes a
    /// cycle reachable at all: two run entries naming each other can be recorded
    /// from both ends. `descends_from` walks that chain, so it is bounded — the
    /// group may come out misordered, but the run must not hang.
    #[test]
    fn run_entries_that_name_each_other_do_not_hang_the_walk() {
        let mut state = KeepOrderState::new();
        let a = printing_task("a");
        let b = printing_task("b");
        // What a pair of run entries naming each other records between them.
        state.injected_by.insert(a.clone(), vec![b.clone()]);
        state.injected_by.insert(b.clone(), vec![a.clone()]);

        assert!(state.descends_from(&a, &b));
        assert!(!state.descends_from(&a, &task_named("elsewhere")));
    }

    /// The defect this fixes: the position used to come from whichever parent
    /// injected first, so two printing parents produced a different order from
    /// one run to the next. Injecting in reverse settles it without needing any
    /// concurrency — before the fix this returned `[p1, p2, c2, c1]`.
    #[test]
    fn two_printing_parents_group_their_children_whatever_order_they_inject_in() {
        let forward = injected_order(false);
        let reversed = injected_order(true);
        assert_eq!(forward, ["p1", "c1", "p2", "c2"]);
        assert_eq!(
            forward, reversed,
            "the order must come from the parents' slots, not from who injected first"
        );
    }

    fn injected_order(reverse: bool) -> Vec<String> {
        let mut state = KeepOrderState::new();
        let p1 = printing_task("p1");
        let p2 = printing_task("p2");
        state.init_task(&p1);
        state.init_task(&p2);
        let injections = [(&p1, "c1"), (&p2, "c2")];
        let injections: Vec<_> = if reverse {
            injections.into_iter().rev().collect()
        } else {
            injections.into_iter().collect()
        };
        for (parent, child) in injections {
            state.insert_injected_tasks(parent, &[task_named(child)]);
        }
        keys(&state)
    }

    fn printing_task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            run: vec![RunEntry::Script(format!("echo {name}"))],
            ..Default::default()
        }
    }

    #[test]
    fn only_tasks_that_produce_or_inject_get_a_keep_order_slot() {
        // The gate both registration paths apply. A task that only aggregates
        // `depends` prints nothing and finishes after everything it waits on, so
        // a slot for it would sit at the front of the map -- where only the
        // front entry may stream -- for the whole run.
        assert!(
            !task_gets_keep_order_slot(&task_named("aggregator")),
            "nothing to print and nothing to anchor"
        );
        assert!(
            task_gets_keep_order_slot(&Task {
                name: "script".to_string(),
                run: vec![RunEntry::Script("echo A".to_string())],
                ..Default::default()
            }),
            "produces output of its own"
        );
        assert!(
            task_gets_keep_order_slot(&Task {
                name: "launch".to_string(),
                run: vec![RunEntry::TaskGroup {
                    tasks: vec!["one".to_string()],
                }],
                ..Default::default()
            }),
            "anchors what it injects"
        );
    }

    #[test]
    fn injected_tasks_keep_their_order_however_many_there_are() {
        // The executor now hands over a whole sub-graph -- the tasks named in the
        // run entry *and* their dependencies -- rather than two or three names,
        // and their relative order is the thing being carried across.
        let mut state = KeepOrderState::new();
        let launch = task_named("launch");
        state.init_task(&launch);

        let children = ["r1", "r2", "r3", "dep3", "dep2", "dep1"].map(task_named);
        state.insert_injected_tasks(&launch, &children);

        assert_eq!(
            keys(&state),
            ["r1", "r2", "r3", "dep3", "dep2", "dep1", "launch"]
        );
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

        state.insert_injected_tasks(&task_named("unregistered"), &[task_named("one")]);

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
        state.insert_injected_tasks(&launch, std::slice::from_ref(&b));
        assert!(state.is_active(&b), "the next child must stream live");
    }
}
