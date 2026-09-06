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
const ROW_BAR_WIDTH: usize = 10;
/// Fine enough that a 24-cell bar never jumps more than one cell per update,
/// and the terminal's OSC progress indicator moves in percent.
const OSC_SCALE: usize = 10_000;

/// Columns below which the artifact name, then the transfer detail, are
/// dropped so the bar and the elapsed time keep their place.
const ARTIFACT_MIN_COLUMNS: usize = 110;
const DETAIL_MIN_COLUMNS: usize = 80;

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
}

impl TtyInstallProgress {
    pub(super) fn new(state: State) -> Self {
        let count = state.tools.len();
        let mise_text = format!("{}", style::emagenta("mise").bold());
        let version_text = format!("{}", style::edim(&*VERSION_PLAIN));
        let header = ProgressJobBuilder::new()
            .body("{{ mise }} {{ version }}  {{ bar }}  {{ status }}")
            .prop("mise", &mise_text)
            .prop("version", &version_text)
            .prop("bar", &state.bar_only(HEADER_BAR_WIDTH))
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

/// Repaint every prop from the model. Cheap enough to run several times a
/// second: it formats a handful of short strings and clx coalesces redraws.
fn refresh(state: &State, header: &Arc<ProgressJob>, rows: &Rows, now: Instant) {
    let columns = console::Term::stderr().size().1 as usize;
    let (progress, _) = state.progress();
    header.progress_current(((progress * OSC_SCALE as f64).round() as usize).min(OSC_SCALE));
    header.prop("bar", &state.bar_only(HEADER_BAR_WIDTH));
    let mut status = state.count_label();
    if let Some(rate) = state.aggregate_rate(now) {
        status.push_str(&format!(" · {}/s", format_bytes(rate.round() as u64)));
    }
    let queued = state.queued_count();
    if queued > 0 {
        status.push_str(&format!(" · {queued} queued"));
    }
    status.push_str(&format!(" · {}", elapsed(state.started, now)));
    header.prop("status", &status);

    let width = state.width();
    for (index, tool) in state.tools.iter().enumerate() {
        let Some(job) = rows.jobs.get(index).and_then(|j| j.as_ref()) else {
            continue;
        };
        job.prop(
            "prefix",
            &console::pad_str(&tool.prefix, width, console::Alignment::Left, None).to_string(),
        );
        if let Some(waiting) = tool.waiting_message() {
            job.prop("left", &style::edim(waiting).to_string());
            job.prop("right", "");
            continue;
        }
        let Some(started) = tool.started else {
            continue;
        };
        let mut left = tool.message.clone();
        let detail = tool.transfer_detail(now);
        if !detail.is_empty() && columns >= DETAIL_MIN_COLUMNS {
            left.push_str(&format!("  {detail}"));
        }
        job.prop("left", &left);
        let mut right = String::new();
        if !tool.weights.is_empty() {
            let filled = filled_cells(tool.fraction, ROW_BAR_WIDTH, tool.outcome.is_some());
            right.push_str(&format!(
                "{}{}  ",
                style::ecyan("█".repeat(filled)),
                style::edim("░".repeat(ROW_BAR_WIDTH - filled))
            ));
        }
        right.push_str(&elapsed(started, now));
        if let Some(artifact) = &tool.artifact
            && columns >= ARTIFACT_MIN_COLUMNS
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
        self.stop();
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        let lines = state.fail_unstarted(failures, now);
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
        let detail = {
            let state = self.state.lock().unwrap();
            let tool = &state.tools[self.index];
            tool.transfer
                .is_none()
                .then(|| tool.detail.clone())
                .flatten()
        };
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
            state.finish_tool(self.index, outcome, Instant::now())
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
