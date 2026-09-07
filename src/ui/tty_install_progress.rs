//! Live install progress for an interactive terminal.
//!
//! The live region is small on purpose: a header with the install-wide bar and
//! one row per tool that is actually doing something. Finished tools leave the
//! region as permanent lines above it — the same lines CI gets — so scrollback
//! is the record and the screen never fills with rows that are done.
//!
//! Everything shown here comes from the shared [`State`]; clx is only asked to
//! paint prop strings and animate a spinner.
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use clx::progress::{ProgressJob, ProgressJobBuilder, ProgressJobDoneBehavior, ProgressStatus};

use super::install_progress::{InstallProgress, ToolProgress};
use super::progress_report::{ProgressIcon, SingleReport};
use super::style;
use super::text_install_progress::{State, Tool, elapsed, filled_cells, first_line, format_bytes};
use crate::cli::version::VERSION_PLAIN;

/// Redraw cadence for elapsed times and rates. clx animates the spinner on its
/// own; this only needs to keep the numbers from looking stuck.
const REFRESH: Duration = Duration::from_millis(250);
const HEADER_BAR_WIDTH: usize = 24;
const MIN_HEADER_BAR_WIDTH: usize = 10;
const ROW_BAR_WIDTH: usize = 10;
/// Fine enough that a 24-cell bar never jumps more than one cell per update,
/// and the terminal's OSC progress indicator moves in percent.
const OSC_SCALE: usize = 10_000;

/// What fits on one line at this terminal width. Decided from the two widths
/// that hold still during an install — the terminal and the prefix column — so
/// a row does not gain and lose its bar as its message changes. Each cell goes
/// in order of how much it says: the artifact name first, then the transfer
/// figures, then the row bar; the header gives up its version, then bar cells.
#[derive(Debug, PartialEq)]
struct Layout {
    version: bool,
    byline: bool,
    /// The header's aggregate transfer rate.
    rate: bool,
    header_bar: usize,
    row_bar: bool,
    bytes: bool,
    artifact: bool,
}

impl Layout {
    fn fit(columns: usize, prefix_width: usize, version_width: usize) -> Self {
        // `mise`, the gaps, and a status as long as "5/7 · 12.3 MB/s · 2 queued · 3.0s".
        const HEADER_FIXED: usize = 4 + 2 + 2 + 36;
        const BYLINE_WIDTH: usize = " by @jdx".len();
        // The byline is the last thing the header gives up: it goes only when
        // even a ten-cell bar would not fit beside it. Below that the bar stays
        // at ten — cells freed by a dropped byline or rate are not handed back,
        // so a shrinking terminal never sees the bar grow — and the aggregate
        // rate leaves the status once the ten cells no longer fit with it.
        let byline = columns >= HEADER_FIXED + BYLINE_WIDTH + MIN_HEADER_BAR_WIDTH;
        let rate = columns >= HEADER_FIXED + MIN_HEADER_BAR_WIDTH;
        let version = columns >= HEADER_FIXED + BYLINE_WIDTH + HEADER_BAR_WIDTH + 1 + version_width;
        let header_bar = if byline {
            (columns - HEADER_FIXED - BYLINE_WIDTH).clamp(MIN_HEADER_BAR_WIDTH, HEADER_BAR_WIDTH)
        } else {
            MIN_HEADER_BAR_WIDTH
        };
        // What a row has after its prefix: the phase, then the optional cells,
        // then the elapsed time and the spinner.
        let free = columns.saturating_sub(prefix_width);
        Self {
            version,
            byline,
            rate,
            header_bar,
            row_bar: free >= 46,
            bytes: free >= 70,
            artifact: free >= 100,
        }
    }
}

struct Rows {
    /// One entry per tool, present while the tool has a row on screen.
    jobs: Vec<Option<Arc<ProgressJob>>>,
    /// Sub-progress rows (an embedded package manager's counter), keyed the same way.
    children: Vec<Option<Arc<ProgressJob>>>,
}

pub(crate) struct TtyInstallProgress {
    state: Arc<Mutex<State>>,
    header: Arc<ProgressJob>,
    rows: Arc<Mutex<Rows>>,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
    finished: bool,
}

impl TtyInstallProgress {
    pub(super) fn new(state: State) -> Self {
        let count = state.tools.len();
        let mise_text = format!("{}", style::emagenta("mise").bold());
        // The first frame is drawn before the first refresh; it fits too.
        let layout = Layout::fit(
            columns(),
            state.width(),
            console::measure_text_width(&VERSION_PLAIN),
        );
        let header = ProgressJobBuilder::new()
            .body("{{ mise }}{{ version }}{{ byline }}  {{ bar }}  {{ status }}")
            .prop("mise", &mise_text)
            .prop("version", &version_text(layout.version))
            .prop("byline", &byline_text(layout.byline))
            .prop("bar", &state.bar_only(layout.header_bar))
            .prop("status", &format!("0/{count}"))
            .progress_total(OSC_SCALE)
            .progress_current(0)
            // Finishing removes the header; the summary line above it is the
            // record, and a second copy of the bar would only repeat it.
            .on_done(ProgressJobDoneBehavior::Hide)
            .start();
        let rows = Arc::new(Mutex::new(Rows {
            jobs: vec![None; count],
            children: vec![None; count],
        }));
        let state = Arc::new(Mutex::new(state));
        let (stop, rx) = mpsc::channel();
        let thread = {
            let state = state.clone();
            let header = header.clone();
            let rows = rows.clone();
            thread::spawn(move || {
                while rx.recv_timeout(REFRESH) == Err(mpsc::RecvTimeoutError::Timeout) {
                    let state = state.lock().unwrap();
                    let rows = rows.lock().unwrap();
                    refresh(&state, &header, &rows, Instant::now());
                }
            })
        };
        Self {
            state,
            header,
            rows,
            stop,
            thread: Some(thread),
            finished: false,
        }
    }

    fn stop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    fn row(&self, index: usize, tool: &Tool, status: ProgressStatus) -> Arc<ProgressJob> {
        let mut rows = self.rows.lock().unwrap();
        if let Some(job) = &rows.jobs[index] {
            job.set_status(status);
            return job.clone();
        }
        let job = self.header.add(
            ProgressJobBuilder::new()
                .body("{{ prefix }} {{ left | flex_fill }} {{ right }} {{ spinner(name=\"arc\") }}")
                .prop("prefix", &tool.prefix)
                .prop("left", "")
                .prop("right", "")
                .status(status)
                .on_done(ProgressJobDoneBehavior::Hide)
                .build(),
        );
        rows.jobs[index] = Some(job.clone());
        job
    }
}

fn columns() -> usize {
    console::Term::stderr().size().1 as usize
}

/// The version with its separating space, or nothing: the header body has no
/// gap of its own to leave behind when the version goes.
fn version_text(shown: bool) -> String {
    if shown {
        format!(" {}", style::edim(&*VERSION_PLAIN))
    } else {
        String::new()
    }
}

/// `mise VERSION by @jdx`, as the header always read.
fn byline_text(shown: bool) -> String {
    if shown {
        format!(" {}", style::edim("by @jdx"))
    } else {
        String::new()
    }
}

/// Repaint every prop from the model. Cheap enough to run several times a
/// second: it formats a handful of short strings and clx coalesces redraws.
fn refresh(state: &State, header: &Arc<ProgressJob>, rows: &Rows, now: Instant) {
    let width = state.width();
    let layout = Layout::fit(
        columns(),
        width,
        console::measure_text_width(&VERSION_PLAIN),
    );
    let (progress, _) = state.progress();
    header.progress_current(((progress * OSC_SCALE as f64).round() as usize).min(OSC_SCALE));
    header.prop("version", &version_text(layout.version));
    header.prop("byline", &byline_text(layout.byline));
    header.prop("bar", &state.bar_only(layout.header_bar));
    let mut status = state.count_label();
    if layout.rate
        && let Some(rate) = state.aggregate_rate(now)
    {
        status.push_str(&format!(" · {}/s", format_bytes(rate.round() as u64)));
    }
    let queued = state.queued_count();
    if queued > 0 {
        status.push_str(&format!(" · {queued} queued"));
    }
    status.push_str(&format!(" · {}", elapsed(state.started, now)));
    header.prop("status", &status);

    for (index, tool) in state.tools.iter().enumerate() {
        let Some(job) = rows.jobs.get(index).and_then(|j| j.as_ref()) else {
            continue;
        };
        job.prop(
            "prefix",
            &console::pad_str(&tool.prefix, width, console::Alignment::Left, None).to_string(),
        );
        let Some(started) = tool.started else {
            // A held tool names what it is behind; once that drains it is only
            // waiting for a slot, and the row must not keep the stale reason.
            let waiting = tool
                .waiting_message()
                .unwrap_or_else(|| "queued".to_string());
            job.prop("left", &style::edim(waiting).to_string());
            job.prop("right", "");
            continue;
        };
        let mut left = tool.message.clone();
        // Bytes only: detail and item counts have their own child row.
        if let Some(bytes) = tool.transfer_bytes(now)
            && layout.bytes
        {
            left.push_str(&format!("  {bytes}"));
        }
        job.prop("left", &left);
        let mut right = String::new();
        if !tool.weights.is_empty() && layout.row_bar {
            let filled = filled_cells(tool.fraction, ROW_BAR_WIDTH, tool.outcome.is_some());
            right.push_str(&format!(
                "{}{}  ",
                style::ecyan("█".repeat(filled)),
                style::edim("░".repeat(ROW_BAR_WIDTH - filled))
            ));
        }
        right.push_str(&elapsed(started, now));
        if let Some(artifact) = &tool.artifact
            && layout.artifact
        {
            right.push_str(&format!("  {}", style::edim(artifact)));
        }
        job.prop("right", &right);
    }
}

impl InstallProgress for TtyInstallProgress {
    fn start_tool(&self, key: &str) -> Option<Box<dyn ToolProgress>> {
        let index = self.state.lock().unwrap().start_tool(key)?;
        {
            let state = self.state.lock().unwrap();
            self.row(index, &state.tools[index], ProgressStatus::Running);
        }
        Some(Box::new(TtyToolProgress {
            state: self.state.clone(),
            header: self.header.clone(),
            rows: self.rows.clone(),
            index,
        }))
    }

    fn queue_tool(&self, key: &str) {
        let mut state = self.state.lock().unwrap();
        state.queue_tool(key);
        if let Some(index) = state.index_of(key) {
            self.row(index, &state.tools[index], ProgressStatus::Pending);
        }
    }

    fn set_waiting(&self, key: &str, dependencies: Vec<String>) {
        let mut state = self.state.lock().unwrap();
        state.set_waiting(key, dependencies);
        if let Some(index) = state.index_of(key)
            && state.tools[index].waiting_message().is_some()
        {
            // A held tool gets a row so the wait is visible, paused rather than
            // spinning: nothing is happening to it yet.
            self.row(index, &state.tools[index], ProgressStatus::Pending);
        }
    }

    fn finish(&mut self, failures: Vec<(String, String)>) {
        self.finished = true;
        self.stop();
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let lines = state.fail_unstarted(failures, now, Some(columns()));
        let mut rows = self.rows.lock().unwrap();
        let Rows { jobs, children } = &mut *rows;
        for job in jobs.iter_mut().chain(children.iter_mut()) {
            if let Some(job) = job.take() {
                job.remove();
            }
        }
        for line in lines {
            self.header.println(&line);
        }
        // One last repaint so the header the summary lands under reads 3/3
        // with a full bar, not whatever the last tick happened to catch.
        refresh(&state, &self.header, &rows, now);
        let icon = if state.count(super::text_install_progress::Outcome::Failed) > 0 {
            ProgressIcon::Error
        } else {
            ProgressIcon::Success
        };
        self.header
            .println(&format!("{icon} {}", state.summary_text(now)));
        self.header.progress_current(OSC_SCALE);
        self.header.set_status(ProgressStatus::Done);
    }
}

impl Drop for TtyInstallProgress {
    fn drop(&mut self) {
        // A session dropped on an error path still closes its live region and
        // leaves its summary; otherwise the header sits there under the error.
        if !self.finished {
            InstallProgress::finish(self, vec![]);
        }
        self.stop();
    }
}

#[derive(Clone)]
pub(crate) struct TtyToolProgress {
    state: Arc<Mutex<State>>,
    header: Arc<ProgressJob>,
    rows: Arc<Mutex<Rows>>,
    index: usize,
}

impl std::fmt::Debug for TtyToolProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TtyToolProgress")
            .field("index", &self.index)
            .finish()
    }
}

impl TtyToolProgress {
    fn with_tool(&self, f: impl FnOnce(&mut Tool)) {
        f(&mut self.state.lock().unwrap().tools[self.index]);
    }

    fn job(&self) -> Option<Arc<ProgressJob>> {
        self.rows.lock().unwrap().jobs[self.index].clone()
    }

    /// Sub-progress gets its own indented row under the tool, so a package
    /// manager's count does not fight the tool's phase for the same cell.
    fn sync_child(&self) {
        let detail = self.state.lock().unwrap().tools[self.index].child_detail(Instant::now());
        let mut rows = self.rows.lock().unwrap();
        match (detail, rows.children[self.index].clone()) {
            (Some(detail), Some(child)) => child.prop("detail", &detail),
            (Some(detail), None) => {
                if let Some(parent) = rows.jobs[self.index].clone() {
                    let child = parent.add(
                        ProgressJobBuilder::new()
                            .body("  {{ arrow }} {{ detail }}")
                            .prop("arrow", &style::edim("↳").to_string())
                            .prop("detail", &detail)
                            .on_done(ProgressJobDoneBehavior::Hide)
                            .build(),
                    );
                    rows.children[self.index] = Some(child);
                }
            }
            (None, Some(child)) => {
                child.remove();
                rows.children[self.index] = None;
            }
            (None, None) => {}
        }
    }
}

impl ToolProgress for TtyToolProgress {
    fn set_prefix(&self, prefix: String) {
        self.with_tool(|tool| tool.prefix = prefix.clone());
        if let Some(job) = self.job() {
            job.prop("prefix", &prefix);
        }
    }

    fn complete(&self, error: Option<&str>) {
        let line = {
            let mut state = self.state.lock().unwrap();
            let outcome = state.tools[self.index].outcome_for(error);
            if let Some(error) = error {
                state.tools[self.index].message = first_line(error);
            }
            state.finish_tool(self.index, outcome, Instant::now(), Some(columns()))
        };
        let Some(line) = line else {
            return;
        };
        // The permanent line goes above the live region; the row leaves it.
        let mut rows = self.rows.lock().unwrap();
        if let Some(child) = rows.children[self.index].take() {
            child.remove();
        }
        if let Some(job) = rows.jobs[self.index].take() {
            job.remove();
        }
        drop(rows);
        self.header.println(&line);
    }

    fn reporter(&self) -> Box<dyn SingleReport> {
        Box::new(self.clone())
    }
}

impl SingleReport for TtyToolProgress {
    fn set_message(&self, message: String) {
        self.with_tool(|tool| tool.apply_message(message));
    }

    fn set_detail(&self, detail: String) {
        self.with_tool(|tool| tool.set_detail(detail));
        self.sync_child();
    }

    fn set_items(&self, done: u64, total: u64) {
        self.with_tool(|tool| tool.set_items(done, total));
        self.sync_child();
    }

    fn println(&self, message: String) {
        let prefix = self.state.lock().unwrap().tools[self.index].prefix.clone();
        for line in message.lines() {
            self.header.println(&format!("{prefix} {line}"));
        }
    }

    fn start_operations(&self, count: usize) {
        self.with_tool(|tool| tool.start_operations(&vec![1.0; count.max(1)]));
    }

    fn start_operations_weighted(&self, weights: &[f64]) {
        self.with_tool(|tool| tool.start_operations(weights));
    }

    fn next_operation(&self) {
        self.with_tool(|tool| tool.next_operation());
        self.sync_child();
    }

    fn set_length(&self, length: u64) {
        self.with_tool(|tool| tool.set_length(length));
        self.sync_child();
    }

    fn set_position(&self, position: u64) {
        self.with_tool(|tool| tool.set_position(position));
    }

    fn inc(&self, delta: u64) {
        self.with_tool(|tool| tool.inc(delta));
    }

    fn abandon(&self) {
        if let Some(job) = self.job() {
            job.set_status(ProgressStatus::Hide);
        }
    }

    fn finish_with_icon(&self, _message: String, icon: ProgressIcon) {
        self.with_tool(|tool| tool.set_skipped(matches!(icon, ProgressIcon::Skipped)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_terminal_shows_everything() {
        let layout = Layout::fit(140, 27, 8);
        assert_eq!(
            layout,
            Layout {
                version: true,
                byline: true,
                rate: true,
                header_bar: HEADER_BAR_WIDTH,
                row_bar: true,
                bytes: true,
                artifact: true,
            }
        );
    }

    #[test]
    fn a_narrow_terminal_gives_up_the_cells_that_say_the_least_first() {
        // The artifact goes first, then the transfer figures, then the row bar.
        assert!(!Layout::fit(100, 12, 8).artifact);
        assert!(Layout::fit(100, 12, 8).bytes);
        assert!(!Layout::fit(80, 12, 8).bytes);
        assert!(Layout::fit(80, 12, 8).row_bar);
        assert!(!Layout::fit(52, 12, 8).row_bar);
        // The header drops the version first, then bar cells down to ten, and
        // only then the byline.
        let at = |columns| Layout::fit(columns, 12, 8);
        assert!(at(90).version && at(90).byline && at(90).header_bar == HEADER_BAR_WIDTH);
        assert!(!at(70).version && at(70).byline && at(70).header_bar == 18);
        assert!(!at(62).version && at(62).byline && at(62).header_bar == MIN_HEADER_BAR_WIDTH);
        assert!(!at(52).version && !at(52).byline && at(52).header_bar == MIN_HEADER_BAR_WIDTH);
        // Below that the rate leaves the status too, so the ten-cell bar and
        // the longest remaining status ("5/7 · 2 queued · 3.0s") fit in 40 —
        // and the bar never grows back as the terminal shrinks.
        assert!(at(54).rate && !at(52).rate);
        assert_eq!(at(40).header_bar, MIN_HEADER_BAR_WIDTH);
        let mut previous = HEADER_BAR_WIDTH;
        for columns in (30..140).rev() {
            assert!(at(columns).header_bar <= previous, "bar grew at {columns}");
            previous = at(columns).header_bar;
        }
    }

    #[test]
    fn a_long_prefix_column_costs_the_rows_their_optional_cells() {
        // The same terminal: a short prefix keeps the bytes, a long one loses them.
        assert!(Layout::fit(90, 12, 8).bytes);
        assert!(!Layout::fit(90, 27, 8).bytes);
    }
}
