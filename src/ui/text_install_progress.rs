//! Append-only install progress for captured stderr and CI terminals, and the
//! progress model it shares with the interactive renderer.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::install_progress::{InstallProgress, ToolProgress};
use super::multi_progress_report::MultiProgressReport;
use super::progress_report::{ProgressIcon, SingleReport};
use super::style;

const INTERVAL: Duration = Duration::from_secs(3);
pub(super) const BAR_WIDTH: usize = 16;

#[derive(Debug)]
pub(super) struct Tool {
    pub(super) key: String,
    pub(super) prefix: String,
    pub(super) started: Option<Instant>,
    pub(super) message: String,
    pub(super) outcome: Option<Outcome>,
    skipped: bool,
    /// Relative cost of each install operation, in the order they run, as the
    /// backend estimated it. Empty until the backend declares its plan.
    pub(super) weights: Vec<f64>,
    pub(super) completed_ops: usize,
    pub(super) transfer: Option<Transfer>,
    /// Secondary progress from work that is not a byte transfer — an embedded
    /// package manager's `32/48 pkgs`. Shown where transfer bytes would be.
    pub(super) detail: Option<String>,
    /// The file the backend chose to download, from its `download <name>`
    /// status. Kept for the completion line: "which artifact did it pick?" is a
    /// question people ask the log after the fact, and a fast install never
    /// lives long enough to appear in a snapshot.
    pub(super) artifact: Option<String>,
    /// The backend found the artifact already downloaded and skipped the fetch.
    /// Without saying so, a 0.3s install of an 80 MB tool looks like a lie.
    pub(super) reused: bool,
    /// Tools this one is held behind. Drained as they finish, so the row
    /// always names what is actually still in the way.
    pub(super) waiting_on: Vec<String>,
    /// Never decreases. A backend that restarts a download (a range request
    /// that the server refuses, a retry) would otherwise walk the bar
    /// backwards, which reads as the install having gone wrong.
    pub(super) fraction: f64,
}

/// Progress through the operation currently running: bytes of a download, or
/// whole items when a package manager counts packages instead.
#[derive(Debug, Clone, Copy)]
pub(super) struct Transfer {
    pub(super) done: u64,
    pub(super) total: u64,
    /// Bytes get sizes and a rate; items get neither.
    pub(super) bytes: bool,
    started: Instant,
    /// Bytes already on disk when this transfer began, from a resumed
    /// download. Excluded from the rate so a resume does not report a
    /// throughput the network never achieved.
    pub(super) resumed_at: u64,
}

impl Transfer {
    fn new(total: u64) -> Self {
        Self {
            done: 0,
            total,
            bytes: true,
            started: Instant::now(),
            resumed_at: 0,
        }
    }

    fn items(done: u64, total: u64) -> Self {
        Self {
            done,
            total,
            bytes: false,
            started: Instant::now(),
            resumed_at: 0,
        }
    }

    pub(super) fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64).clamp(0.0, 1.0))
    }

    /// Average over this transfer rather than an instantaneous rate: the
    /// snapshot only lands every few seconds, so a sampled rate would report
    /// whichever moment it happened to catch.
    fn rate(&self, now: Instant) -> Option<f64> {
        if !self.bytes {
            return None;
        }
        let seconds = now.saturating_duration_since(self.started).as_secs_f64();
        let moved = self.done.saturating_sub(self.resumed_at);
        (seconds > 0.5 && moved > 0).then(|| moved as f64 / seconds)
    }
}

impl Tool {
    pub(super) fn new(key: String, prefix: String) -> Self {
        Self {
            key,
            prefix,
            started: None,
            message: "queued".into(),
            outcome: None,
            skipped: false,
            weights: vec![],
            completed_ops: 0,
            transfer: None,
            detail: None,
            artifact: None,
            reused: false,
            waiting_on: vec![],
            fraction: 0.0,
        }
    }

    /// How far through its own install this tool is, in [0, 1].
    ///
    /// Operations are weighted by the backend's estimate of their cost, and the
    /// one in flight is filled by its byte progress when it has any. This paces
    /// the bar; it is not a time estimate, and nothing about the install depends
    /// on it being accurate.
    fn compute_fraction(&self) -> f64 {
        if self.outcome.is_some() {
            return 1.0;
        }
        let total: f64 = self.weights.iter().sum();
        if total <= 0.0 {
            return 0.0;
        }
        let done: f64 = self.weights.iter().take(self.completed_ops).sum();
        let current = self
            .transfer
            .and_then(|t| t.fraction())
            .and_then(|f| self.weights.get(self.completed_ops).map(|w| w * f))
            .unwrap_or(0.0);
        // Reserve the tail: a tool is only 1.0 once its worker returns, since
        // postinstall hooks and cleanup run after the last declared operation.
        ((done + current) / total).clamp(0.0, 0.99)
    }

    pub(super) fn advance(&mut self) {
        self.fraction = self.compute_fraction().max(self.fraction);
    }

    pub(super) fn is_active(&self) -> bool {
        self.started.is_some() && self.outcome.is_none()
    }

    pub(super) fn is_queued(&self) -> bool {
        self.started.is_none() && self.outcome.is_none()
    }

    /// What the row says while nothing else does: which tools it is behind.
    pub(super) fn waiting_message(&self) -> Option<String> {
        (self.is_queued() && !self.waiting_on.is_empty())
            .then(|| format!("waiting for {}", self.waiting_on.join(", ")))
    }

    /// A backend's status message. The phase is normalized so the bar can
    /// recognize it; the artifact and a skipped fetch are kept as facts.
    pub(super) fn apply_message(&mut self, message: String) {
        let message = message.replace(['\r', '\n'], " ");
        let mut words = message.split_whitespace();
        let mut reused = false;
        let mut operations_done = false;
        let (phase, artifact) = match words.next() {
            Some("download") => ("downloading", words.next()),
            Some("cached") => {
                reused = true;
                ("reusing download", words.next())
            }
            Some("checksum") => ("verifying checksum", None),
            Some("verify") if words.clone().next() == Some("size") => ("verifying size", None),
            Some("verify") => ("verifying", None),
            Some("extract") => ("extracting", None),
            Some("install") => ("installing", None),
            Some("uninstall") | Some("remove") => ("removing", None),
            // The postinstall hook means every declared operation is behind us.
            // Plugin backends (asdf, vfox) never call next_operation, so
            // without this their bar sits at zero until the worker returns.
            Some("running") if message == "running custom postinstall hook" => {
                operations_done = true;
                ("running postinstall hook", None)
            }
            _ => (message.as_str(), None),
        };
        self.message = phase.to_string();
        self.reused |= reused;
        if let Some(artifact) = artifact {
            self.artifact = Some(artifact.to_string());
        }
        if operations_done {
            self.completed_ops = self.weights.len();
            self.transfer = None;
        }
        self.advance();
    }

    pub(super) fn start_operations(&mut self, weights: &[f64]) {
        let weights: Vec<f64> = weights.iter().copied().filter(|w| *w > 0.0).collect();
        self.weights = if weights.is_empty() {
            vec![1.0]
        } else {
            weights
        };
        self.advance();
    }

    pub(super) fn next_operation(&mut self) {
        self.completed_ops = (self.completed_ops + 1).min(self.weights.len());
        // Byte progress belongs to the operation that just ended.
        self.transfer = None;
        self.advance();
    }

    /// A length always opens a new transfer, even when it equals the last one:
    /// the downloader announces it once per attempt, and a retry that kept the
    /// previous state would fold the failed attempt's bytes and time into this
    /// one's rate.
    pub(super) fn set_length(&mut self, length: u64) {
        self.transfer = Some(Transfer::new(length));
        self.advance();
    }

    pub(super) fn set_position(&mut self, position: u64) {
        // A server that sends no content-length never calls set_length, so the
        // first bytes open a transfer with an unknown total.
        let transfer = self.transfer.get_or_insert_with(|| Transfer::new(0));
        // A resumed download reports its starting offset here.
        if transfer.done == 0 && position > 0 {
            transfer.resumed_at = position;
        }
        transfer.done = position;
        self.advance();
    }

    pub(super) fn inc(&mut self, delta: u64) {
        let transfer = self.transfer.get_or_insert_with(|| Transfer::new(0));
        transfer.done = transfer.done.saturating_add(delta);
        self.advance();
    }

    pub(super) fn set_detail(&mut self, detail: String) {
        self.detail = (!detail.trim().is_empty()).then_some(detail);
    }

    /// Whole-item progress through the current operation. Replaces whatever
    /// transfer was there: a package manager that switches from resolving to
    /// fetching reports a new tally against a new total.
    pub(super) fn set_items(&mut self, done: u64, total: u64) {
        self.transfer = Some(Transfer::items(done, total));
        self.advance();
    }

    pub(super) fn set_skipped(&mut self, skipped: bool) {
        self.skipped = skipped;
    }

    /// The worker's outcome, given its error if it had one.
    pub(super) fn outcome_for(&self, error: Option<&str>) -> Outcome {
        match error {
            Some(_) => Outcome::Failed,
            None if self.skipped => Outcome::Skipped,
            None => Outcome::Installed,
        }
    }

    /// `42.1/78.3 MB · 12.4 MB/s`, dropping whichever half is unknown. A server
    /// that sends no content-length still gets a running byte count.
    ///
    /// Explicit detail wins over the transfer: a package manager reports its
    /// tally through `set_detail` and drives the bar with counts, and those
    /// counts must not be printed as bytes.
    pub(super) fn transfer_detail(&self, now: Instant) -> String {
        if let Some(detail) = &self.detail {
            return detail.clone();
        }
        if let Some(bytes) = self.transfer_bytes(now) {
            return bytes;
        }
        match self.transfer {
            Some(transfer) if !transfer.bytes && (transfer.done > 0 || transfer.total > 0) => {
                format!("{}/{}", transfer.done, transfer.total)
            }
            _ => String::new(),
        }
    }

    /// Just the byte transfer — `42.1/78.3 MB · 12.4 MB/s` — for a renderer
    /// that shows detail and item counts somewhere else and must not print
    /// them twice.
    pub(super) fn transfer_bytes(&self, now: Instant) -> Option<String> {
        let transfer = self.transfer?;
        if !transfer.bytes || (transfer.done == 0 && transfer.total == 0) {
            return None;
        }
        let bytes = if transfer.total > 0 {
            format!(
                "{}/{}",
                format_bytes_in(transfer.done, transfer.total),
                format_bytes(transfer.total)
            )
        } else {
            format_bytes(transfer.done)
        };
        Some(match transfer.rate(now) {
            Some(rate) => format!("{bytes} · {}/s", format_bytes(rate.round() as u64)),
            None => bytes,
        })
    }

    /// What the interactive display's indented child row shows: an explicit
    /// detail, or the item tally a package manager drives the bar with. A byte
    /// transfer belongs to the parent row and is not repeated underneath it.
    pub(super) fn child_detail(&self, now: Instant) -> Option<String> {
        match self.transfer {
            Some(transfer) if transfer.bytes && self.detail.is_none() => None,
            _ => Some(self.transfer_detail(now)).filter(|detail| !detail.is_empty()),
        }
    }

    /// The permanent line for a finished tool, padded to `width`. Given the
    /// terminal's `columns`, the artifact name is dropped rather than wrapped:
    /// the line is a record, and a wrapped record reads as two.
    pub(super) fn completion_line(
        &self,
        width: usize,
        now: Instant,
        columns: Option<usize>,
    ) -> String {
        let outcome = self
            .outcome
            .expect("only finished tools have a completion line");
        let prefix = console::pad_str(&self.prefix, width, console::Alignment::Left, None);
        let duration = self
            .started
            .map(|started| format!("  {}", elapsed(started, now)))
            .unwrap_or_default();
        let (icon, detail) = match outcome {
            Outcome::Installed if self.reused => (ProgressIcon::Success, " · cached".into()),
            Outcome::Installed => (ProgressIcon::Success, String::new()),
            Outcome::Skipped => (ProgressIcon::Skipped, " · already installed".into()),
            Outcome::Failed => (ProgressIcon::Error, format!(" · failed: {}", self.message)),
        };
        let line = format!("{icon} {prefix}{duration}{detail}");
        match (&self.artifact, outcome) {
            (Some(artifact), Outcome::Installed) => {
                let with_artifact = format!("{line}  {}", style::edim(artifact));
                match columns {
                    Some(columns) if console::measure_text_width(&with_artifact) > columns => line,
                    _ => with_artifact,
                }
            }
            _ => line,
        }
    }
}

pub(super) fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1_000.0;
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} kB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

/// The running half of `42.1/78.3 MB`: scaled to the total's unit and printed
/// without it, so the pair reads as one measurement instead of two.
fn format_bytes_in(bytes: u64, total: u64) -> String {
    const KB: f64 = 1_000.0;
    const MB: f64 = 1_000_000.0;
    const GB: f64 = 1_000_000_000.0;
    let (unit, decimals) = if total as f64 >= GB {
        (GB, 1)
    } else if total as f64 >= MB {
        (MB, 1)
    } else if total as f64 >= 10.0 * KB {
        (KB, 0)
    } else if total as f64 >= KB {
        // Small enough that whole kilobytes would round a half-done transfer
        // to zero.
        (KB, 1)
    } else {
        (1.0, 0)
    };
    format!("{:.*}", decimals, bytes as f64 / unit)
}

/// What a session is doing to its tools. The same model and renderers serve
/// both; only the verbs differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Install,
    Remove,
}

impl Action {
    pub(super) fn present(self) -> &'static str {
        match self {
            Action::Install => "installing",
            Action::Remove => "removing",
        }
    }

    pub(super) fn past(self) -> &'static str {
        match self {
            Action::Install => "installed",
            Action::Remove => "removed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Installed,
    Skipped,
    Failed,
}

#[derive(Debug)]
pub(super) struct State {
    pub(super) action: Action,
    pub(super) tools: Vec<Tool>,
    pub(super) started: Instant,
}

impl State {
    #[cfg(test)]
    pub(super) fn new(tools: impl Iterator<Item = (String, String)>) -> Self {
        Self::for_action(Action::Install, tools)
    }

    pub(super) fn for_action(
        action: Action,
        tools: impl Iterator<Item = (String, String)>,
    ) -> Self {
        // Match the dependency scheduler's deduplication of repeated requests.
        let mut seen = HashSet::new();
        Self {
            action,
            started: Instant::now(),
            tools: tools
                .filter(|(key, _)| seen.insert(key.clone()))
                .map(|(key, prefix)| Tool::new(key, prefix))
                .collect(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub(super) fn width(&self) -> usize {
        self.tools
            .iter()
            .map(|t| console::measure_text_width(&t.prefix))
            .max()
            .unwrap_or(0)
    }

    pub(super) fn index_of(&self, key: &str) -> Option<usize> {
        self.tools.iter().position(|t| t.key == key)
    }

    pub(super) fn start_tool(&mut self, key: &str) -> Option<usize> {
        let index = self.index_of(key)?;
        let tool = &mut self.tools[index];
        tool.started = Some(Instant::now());
        tool.message = "resolving".into();
        Some(index)
    }

    pub(super) fn set_waiting(&mut self, key: &str, dependencies: Vec<String>) {
        if let Some(index) = self.index_of(key) {
            self.tools[index].waiting_on = dependencies;
        }
    }

    pub(super) fn count(&self, outcome: Outcome) -> usize {
        self.tools
            .iter()
            .filter(|t| t.outcome == Some(outcome))
            .count()
    }

    pub(super) fn complete_count(&self) -> usize {
        self.tools.iter().filter(|t| t.outcome.is_some()).count()
    }

    /// Tools waiting only for a job slot. Ones held behind a dependency have
    /// their own row saying which, and are not counted here as well.
    pub(super) fn queued_count(&self) -> usize {
        self.tools
            .iter()
            .filter(|t| t.is_queued() && t.waiting_message().is_none())
            .count()
    }

    pub(super) fn all_done(&self) -> bool {
        self.tools.iter().all(|t| t.outcome.is_some())
    }

    /// Overall fraction in [0, 1] and the share of it that is failed tools.
    pub(super) fn progress(&self) -> (f64, f64) {
        let total = self.tools.len();
        if total == 0 {
            return (1.0, 0.0);
        }
        (
            self.tools.iter().map(|t| t.fraction).sum::<f64>() / total as f64,
            self.count(Outcome::Failed) as f64 / total as f64,
        )
    }

    /// Bytes per second across every active transfer, when any is moving.
    pub(super) fn aggregate_rate(&self, now: Instant) -> Option<f64> {
        let rates: Vec<f64> = self
            .tools
            .iter()
            .filter(|t| t.is_active())
            .filter_map(|t| t.transfer.and_then(|x| x.rate(now)))
            .collect();
        (!rates.is_empty()).then(|| rates.iter().sum())
    }

    /// The bar fills fractionally — a tool halfway through its own install
    /// occupies half the width a finished one would — while the count beside it
    /// stays whole tools, which is the number that can be verified against the
    /// completion lines above.
    pub(super) fn bar(&self) -> String {
        format!("{} {}", self.bar_only(BAR_WIDTH), self.count_label())
    }

    pub(super) fn count_label(&self) -> String {
        format!("{}/{}", self.complete_count(), self.tools.len())
    }

    pub(super) fn bar_only(&self, width: usize) -> String {
        let (progress, failed_share) = self.progress();
        let filled = filled_cells(progress, width, self.all_done());
        // A failed tool still counts as finished work, but a fully cyan bar over
        // "installed 0 tools · 1 failed" reads as success. Its share is red.
        let failed = ((failed_share * width as f64).round() as usize).min(filled);
        format!(
            "{}{}{}",
            style::ecyan("█".repeat(filled - failed)),
            style::ered("█".repeat(failed)),
            style::edim("░".repeat(width - filled))
        )
    }

    pub(super) fn snapshot(&self, now: Instant) -> String {
        let mut lines = vec![format!("{} · {}", self.bar(), elapsed(self.started, now))];
        // Columns size to their content on every snapshot. A fixed width would
        // either truncate a status like "running postinstall hook" or reserve
        // blank space for a transfer column most rows never fill.
        // Rows are read in CI log viewers and narrow panes, so the fixed-shape
        // cells come first — prefix, phase, elapsed — and the one variable cell,
        // the transfer detail, is last and unpadded. The artifact name is not
        // repeated here: it wraps a long row every three seconds, and the
        // completion line records it once, in full, where it can be searched.
        let rows: Vec<_> = self
            .tools
            .iter()
            .filter(|t| t.is_active())
            .filter_map(|tool| {
                let started = tool.started?;
                Some((
                    tool.prefix.as_str(),
                    tool.message.clone(),
                    elapsed(started, now),
                    tool.transfer_detail(now),
                ))
            })
            .collect();
        let width = self.width();
        let message_width = column_width(rows.iter().map(|r| r.1.as_str()));
        let elapsed_width = column_width(rows.iter().map(|r| r.2.as_str()));
        for (prefix, message, elapsed, detail) in rows {
            let mut line = format!(
                "  {}  {}  {}",
                console::pad_str(prefix, width, console::Alignment::Left, None),
                console::pad_str(&message, message_width, console::Alignment::Left, None),
                console::pad_str(&elapsed, elapsed_width, console::Alignment::Right, None),
            );
            if !detail.is_empty() {
                line.push_str(&format!("  {detail}"));
            }
            lines.push(line);
        }
        // Tools held behind a dependency say which one; the rest are just
        // waiting for a slot, and a count is all there is to say about them.
        for tool in self.tools.iter().filter(|t| t.is_queued()) {
            if let Some(waiting) = tool.waiting_message() {
                lines.push(format!(
                    "  {}  {}",
                    console::pad_str(&tool.prefix, width, console::Alignment::Left, None),
                    style::edim(waiting)
                ));
            }
        }
        let queued = self.queued_count();
        if queued > 0 {
            lines.push(format!("  {queued} queued"));
        }
        lines.join("\n")
    }

    /// Record a tool's outcome and return its permanent line, or `None` when
    /// it had already finished so nothing is reported or counted twice.
    pub(super) fn finish_tool(
        &mut self,
        index: usize,
        outcome: Outcome,
        now: Instant,
        columns: Option<usize>,
    ) -> Option<String> {
        let width = self.width();
        if self.tools[index].outcome.is_some() {
            return None;
        }
        let key = self.tools[index].key.clone();
        {
            let tool = &mut self.tools[index];
            tool.outcome = Some(outcome);
            tool.fraction = 1.0;
            tool.transfer = None;
        }
        // Whatever was waiting on this tool is no longer waiting on it.
        for tool in &mut self.tools {
            tool.waiting_on.retain(|dep| dep != &key);
        }
        Some(self.tools[index].completion_line(width, now, columns))
    }

    /// Fail every listed tool that never reached a worker, returning their lines.
    pub(super) fn fail_unstarted(
        &mut self,
        failures: Vec<(String, String)>,
        now: Instant,
        columns: Option<usize>,
    ) -> Vec<String> {
        let mut lines = vec![];
        for (key, error) in failures {
            if let Some(index) = self.index_of(&key)
                && self.tools[index].outcome.is_none()
            {
                self.tools[index].message = first_line(&error);
                lines.extend(self.finish_tool(index, Outcome::Failed, now, columns));
            }
        }
        lines
    }

    pub(super) fn summary(&self, now: Instant) -> String {
        format!("{} · {}", self.bar(), self.summary_text(now))
    }

    pub(super) fn summary_text(&self, now: Instant) -> String {
        let installed = self.count(Outcome::Installed);
        let skipped = self.count(Outcome::Skipped);
        let failed = self.count(Outcome::Failed);
        let mut result = format!(
            "{} {installed} {}",
            self.action.past(),
            tool_noun(installed)
        );
        if skipped > 0 {
            result.push_str(&format!(" · {skipped} already installed"));
        }
        if failed > 0 {
            result.push_str(&format!(" · {failed} failed"));
        }
        result.push_str(&format!(" in {}", elapsed(self.started, now)));
        result
    }
}

/// Cells to fill for `progress` of `width`. While anything is unfinished the
/// last cell stays empty: a fraction capped at 0.99 still rounds to every cell
/// of a wide bar, and a full bar over "0/1" is a lie the eye reads first.
pub(super) fn filled_cells(progress: f64, width: usize, done: bool) -> usize {
    let cells = (progress * width as f64).floor() as usize;
    if done {
        cells.min(width)
    } else {
        cells.min(width.saturating_sub(1))
    }
}

pub(super) fn first_line(error: &str) -> String {
    error.lines().next().unwrap_or("installation failed").into()
}

fn column_width<'a>(cells: impl Iterator<Item = &'a str>) -> usize {
    cells.map(console::measure_text_width).max().unwrap_or(0)
}

pub(super) fn tool_noun(count: usize) -> &'static str {
    if count == 1 { "tool" } else { "tools" }
}

pub(super) fn elapsed(start: Instant, now: Instant) -> String {
    let duration = now.saturating_duration_since(start);
    if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

/// A session owns its heartbeat. Dropping it stops and joins the thread, including
/// on early errors, so no progress can appear after the command's final output.
pub(crate) struct TextInstallProgress {
    state: Arc<Mutex<State>>,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl TextInstallProgress {
    pub(super) fn new(state: State) -> Self {
        let total = state.tools.len();
        info!("{} {total} {}", state.action.present(), tool_noun(total));
        let state = Arc::new(Mutex::new(state));
        let (stop, rx) = mpsc::channel();
        let shared = state.clone();
        let thread = thread::spawn(move || {
            while rx.recv_timeout(INTERVAL) == Err(mpsc::RecvTimeoutError::Timeout) {
                let render = || {
                    let state = shared.lock().unwrap();
                    // Completed tools have already printed their individual results.
                    if !state.all_done() {
                        info!("{}", state.snapshot(Instant::now()));
                    }
                };
                if let Some(report) = MultiProgressReport::try_get() {
                    report.with_progress_unpaused(render);
                } else {
                    render();
                }
            }
        });
        Self {
            state,
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
}

impl InstallProgress for TextInstallProgress {
    fn start_tool(&self, key: &str) -> Option<Box<dyn ToolProgress>> {
        let index = self.state.lock().unwrap().start_tool(key)?;
        Some(Box::new(TextToolProgress {
            state: self.state.clone(),
            index,
        }))
    }

    fn set_waiting(&self, key: &str, dependencies: Vec<String>) {
        self.state.lock().unwrap().set_waiting(key, dependencies);
    }

    fn finish(&mut self, failures: Vec<(String, String)>) {
        self.stop();
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        for line in state.fail_unstarted(failures, now, None) {
            info!("{line}");
        }
        info!("{}", state.summary(now));
    }
}

impl Drop for TextInstallProgress {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextToolProgress {
    state: Arc<Mutex<State>>,
    index: usize,
}

impl TextToolProgress {
    fn with_tool(&self, f: impl FnOnce(&mut Tool)) {
        f(&mut self.state.lock().unwrap().tools[self.index]);
    }
}

impl ToolProgress for TextToolProgress {
    fn set_prefix(&self, prefix: String) {
        self.with_tool(|tool| tool.prefix = prefix);
    }

    /// `error` is the worker's own failure. Without it the line would report
    /// whatever phase the tool happened to be in when it died, which reads as a
    /// reason ("failed: ✓ Cosign verified") without being one.
    fn complete(&self, error: Option<&str>) {
        let mut state = self.state.lock().unwrap();
        let outcome = state.tools[self.index].outcome_for(error);
        if let Some(error) = error {
            state.tools[self.index].message = first_line(error);
        }
        if let Some(line) = state.finish_tool(self.index, outcome, Instant::now(), None) {
            info!("{line}");
        }
    }

    fn reporter(&self) -> Box<dyn SingleReport> {
        Box::new(self.clone())
    }
}

impl SingleReport for TextToolProgress {
    fn set_message(&self, message: String) {
        self.with_tool(|tool| tool.apply_message(message));
    }

    fn set_detail(&self, detail: String) {
        self.with_tool(|tool| tool.set_detail(detail));
    }

    fn set_items(&self, done: u64, total: u64) {
        self.with_tool(|tool| tool.set_items(done, total));
    }

    /// A child process's own output is the diagnostic value of an install log,
    /// so it stays immediate. Only the phase messages a backend generates are
    /// folded into the periodic snapshot.
    fn set_process_output(&self, message: String) {
        self.println(message);
    }

    fn shows_process_output(&self) -> bool {
        true
    }

    fn println(&self, message: String) {
        // Explicit output (including build scripts) remains immediate, through
        // the logger so that it is redacted like every other line mise prints.
        // The lock is released first: a child can emit a lot of these, and they
        // must not serialize behind another tool's status update.
        let prefix = self.state.lock().unwrap().tools[self.index].prefix.clone();
        for line in message.lines() {
            info!("{prefix} {line}");
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
    }

    fn set_length(&self, length: u64) {
        self.with_tool(|tool| tool.set_length(length));
    }

    fn set_position(&self, position: u64) {
        self.with_tool(|tool| tool.set_position(position));
    }

    fn inc(&self, delta: u64) {
        self.with_tool(|tool| tool.inc(delta));
    }

    fn finish_with_icon(&self, _message: String, icon: ProgressIcon) {
        self.with_tool(|tool| tool.set_skipped(matches!(icon, ProgressIcon::Skipped)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(started: Instant) -> State {
        let mut state = State::new((0..3).map(|i| (i.to_string(), format!("tool{i}@1"))));
        state.started = started;
        state
    }

    fn tool_progress(state: State) -> (Arc<Mutex<State>>, TextToolProgress) {
        let shared = Arc::new(Mutex::new(state));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
        (shared, progress)
    }

    fn transfer(done: u64, total: u64, started: Instant) -> Transfer {
        Transfer {
            done,
            total,
            bytes: true,
            started,
            resumed_at: 0,
        }
    }

    #[test]
    fn snapshots_separate_queue_time_from_work_time() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        state.finish_tool(0, Outcome::Installed, start + Duration::from_secs(1), None);
        state.tools[1].started = Some(start + Duration::from_secs(2));
        state.tools[1].message = "extracting".into();
        let snapshot =
            console::strip_ansi_codes(&state.snapshot(start + Duration::from_secs(3))).into_owned();
        assert!(snapshot.starts_with("█████░░░░░░░░░░░ 1/3 · 3.0s"));
        // A finished tool has already printed its own line.
        assert!(!snapshot.contains("tool0"));
        let row = snapshot
            .lines()
            .find(|l| l.contains("tool1@1"))
            .expect("the running tool has a row");
        assert!(row.contains("extracting"), "{row}");
        // Its own 1.0s of work, not the 3.0s since the install started.
        assert!(row.trim_end().ends_with("1.0s"), "{row}");
        // Nothing but the four cells: no artifact, no padded empty detail.
        assert_eq!(row.trim_end(), "  tool1@1  extracting  1.0s", "{row}");
        assert!(snapshot.ends_with("1 queued"));
    }

    #[test]
    fn snapshot_rows_put_detail_last_and_leave_no_gaps() {
        let start = Instant::now();
        let mut state = state(start);
        for tool in &mut state.tools {
            tool.started = Some(start);
        }
        state.tools[0].message = "extracting".into();
        state.tools[0].artifact = Some("node-v22.23.2-linux-x64.tar.gz".into());
        state.tools[1].message = "verifying checksum".into();
        state.tools[1].transfer = Some(transfer(1_100_000, 2_300_000, start));
        state.tools[2].apply_message("verify hk-x86_64-unknown-linux-gnu.tar.gz".into());
        let snapshot =
            console::strip_ansi_codes(&state.snapshot(start + Duration::from_secs(3))).into_owned();
        let rows: Vec<&str> = snapshot.lines().skip(1).map(str::trim_end).collect();
        assert_eq!(
            rows,
            [
                "  tool0@1  extracting          3.0s",
                "  tool1@1  verifying checksum  3.0s  1.1/2.3 MB · 367 kB/s",
                "  tool2@1  verifying           3.0s",
            ]
        );
    }

    #[test]
    fn a_removal_session_speaks_in_its_own_verbs() {
        let start = Instant::now();
        let mut state = State::for_action(
            Action::Remove,
            (0..2).map(|i| (i.to_string(), format!("tool{i}@1"))),
        );
        state.started = start;
        state.tools[0].started = Some(start);
        state.tools[0].apply_message("uninstall".into());
        assert_eq!(state.tools[0].message, "removing");
        state.tools[0].apply_message("remove ~/.local/share/mise/installs/tool0/1".into());
        assert_eq!(state.tools[0].message, "removing");
        state.finish_tool(
            0,
            Outcome::Installed,
            start + Duration::from_millis(40),
            None,
        );
        state.finish_tool(1, Outcome::Installed, start, None);
        let summary = console::strip_ansi_codes(&state.summary(start)).into_owned();
        assert_eq!(summary, "████████████████ 2/2 · removed 2 tools in 0ms");
    }

    #[test]
    fn a_tool_held_behind_a_dependency_says_which_one() {
        let start = Instant::now();
        let mut state = state(start);
        state.set_waiting("2", vec!["0".into(), "1".into()]);
        state.tools[0].started = Some(start);
        let snapshot = console::strip_ansi_codes(&state.snapshot(start)).into_owned();
        assert!(snapshot.contains("tool2@1  waiting for 0, 1"), "{snapshot}");
        // Only the slot-bound tool is counted as merely queued.
        assert!(snapshot.ends_with("1 queued"), "{snapshot}");
        // Finishing a dependency drops it from the wait.
        state.finish_tool(0, Outcome::Installed, start, None);
        assert_eq!(state.tools[2].waiting_on, vec!["1".to_string()]);
    }

    #[test]
    fn terminal_results_are_not_reported_or_counted_twice() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        let finished = state
            .finish_tool(
                0,
                Outcome::Installed,
                start + Duration::from_millis(250),
                None,
            )
            .unwrap();
        assert!(finished.contains("250ms"));
        assert!(state.finish_tool(0, Outcome::Failed, start, None).is_none());
        state.finish_tool(1, Outcome::Skipped, start, None);
        state.finish_tool(2, Outcome::Failed, start, None);
        let summary = console::strip_ansi_codes(&state.summary(start)).into_owned();
        assert_eq!(
            summary,
            "████████████████ 3/3 · installed 1 tool · 1 already installed · 1 failed in 0ms"
        );
    }

    #[test]
    fn backend_finish_does_not_complete_a_worker() {
        let (shared, progress) = tool_progress(state(Instant::now()));
        progress.finish_with_message("installed".into());
        assert!(shared.lock().unwrap().tools[0].outcome.is_none());
        progress.set_message("running postinstall hook".into());
        progress.complete(Some("hook exited with status 1"));
        assert_eq!(
            shared.lock().unwrap().tools[0].outcome,
            Some(Outcome::Failed)
        );
    }

    #[test]
    fn a_failure_reports_the_error_not_the_phase_it_died_in() {
        let (shared, progress) = tool_progress(state(Instant::now()));
        // A phase message can even read as a success on its own.
        progress.set_message("✓ Cosign verified".into());
        progress.complete(Some("checksum mismatch\nsecond line"));
        assert_eq!(shared.lock().unwrap().tools[0].message, "checksum mismatch");
    }

    #[test]
    fn a_skip_survives_a_successful_completion() {
        let (shared, progress) = tool_progress(state(Instant::now()));
        progress.finish_with_icon("already installed".into(), ProgressIcon::Skipped);
        progress.complete(None);
        assert_eq!(
            shared.lock().unwrap().tools[0].outcome,
            Some(Outcome::Skipped)
        );
    }

    #[test]
    fn a_half_done_tool_fills_half_the_width_of_a_finished_one() {
        let start = Instant::now();
        let mut state = state(start);
        // One finished, one exactly halfway through a single-operation install.
        state.tools[0].started = Some(start);
        state.finish_tool(0, Outcome::Installed, start, None);
        state.tools[1].started = Some(start);
        state.tools[1].weights = vec![1.0];
        state.tools[1].transfer = Some(transfer(50, 100, start));
        state.tools[1].advance();
        // 1.0 + ~0.5 + 0.0 over three tools is half the bar, while the count
        // beside it still reports whole tools.
        let bar = console::strip_ansi_codes(&state.bar()).into_owned();
        assert_eq!(bar, "████████░░░░░░░░ 1/3");
    }

    #[test]
    fn weights_pace_the_bar_by_the_backends_estimate() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.started = Some(start);
        // download, checksum, extract
        tool.start_operations(&[0.7, 0.15, 0.15]);
        assert_eq!(tool.fraction, 0.0);
        // Halfway through the download is 35% of the tool, not 1/6.
        tool.transfer = Some(transfer(1, 2, start));
        tool.advance();
        assert!((tool.fraction - 0.35).abs() < 1e-9, "{}", tool.fraction);
        // The tail stays reserved for the work that follows the last operation.
        tool.next_operation();
        tool.next_operation();
        tool.next_operation();
        assert_eq!(tool.fraction, 0.99);
    }

    #[test]
    fn progress_never_walks_backwards() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.weights = vec![1.0];
        tool.transfer = Some(transfer(90, 100, start));
        tool.advance();
        let high = tool.fraction;
        // A restarted download reports byte zero again.
        tool.set_length(100);
        assert_eq!(tool.fraction, high);
    }

    #[test]
    fn transfer_detail_reports_bytes_and_rate_in_one_unit() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.transfer = Some(transfer(42_100_000, 78_300_000, start));
        let detail = tool.transfer_detail(start + Duration::from_secs(4));
        assert_eq!(detail, "42.1/78.3 MB · 10.5 MB/s");

        // A server that sends no length still gets a running count, and a rate
        // needs enough of a sample to mean anything.
        tool.transfer = Some(transfer(1_500_000, 0, start));
        assert_eq!(
            tool.transfer_detail(start + Duration::from_millis(100)),
            "1.5 MB"
        );
    }

    #[test]
    fn dependency_waiting_tools_are_not_counted_as_queued() {
        let start = Instant::now();
        let mut state = state(start);
        state.set_waiting("2", vec!["0".into()]);
        // tool0 and tool1 wait for a slot; tool2 waits for tool0.
        assert_eq!(state.queued_count(), 2);
        state.tools[0].started = Some(start);
        state.finish_tool(0, Outcome::Installed, start, None);
        // tool2's wait drained: it is a plain queued tool now.
        assert!(state.tools[2].waiting_message().is_none());
        assert_eq!(state.queued_count(), 2);
    }

    #[test]
    fn transfer_bytes_is_only_ever_bytes() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.set_detail("32/48 pkgs".into());
        assert_eq!(tool.transfer_bytes(start), None);
        assert_eq!(tool.transfer_detail(start), "32/48 pkgs");
        // Item counts drive the bar but are not a byte transfer either.
        tool.detail = None;
        tool.set_items(32, 48);
        assert_eq!(tool.transfer_bytes(start), None);
        assert_eq!(tool.transfer_detail(start), "32/48");
    }

    #[test]
    fn non_transfer_detail_fills_the_same_column() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.set_detail("32/48 pkgs".into());
        assert_eq!(tool.transfer_detail(start), "32/48 pkgs");
        // Items driving the bar must not print as bytes, with or without
        // explicit detail, and never contribute a transfer rate.
        tool.set_items(32, 48);
        assert_eq!(tool.transfer_detail(start), "32/48 pkgs");
        assert_eq!(
            tool.transfer.unwrap().rate(start + Duration::from_secs(5)),
            None
        );
        tool.set_detail(String::new());
        assert_eq!(tool.transfer_detail(start), "32/48");
        // Items fill the current operation exactly as bytes would. (The
        // fraction is monotonic, so this tally must not fall below the 32/48
        // already reported above.)
        tool.start_operations(&[1.0]);
        tool.set_items(36, 48);
        assert!((tool.fraction - 0.75).abs() < 1e-9, "{}", tool.fraction);
        // Without explicit detail, the transfer is bytes.
        tool.set_detail(String::new());
        tool.transfer = Some(transfer(500, 1000, start));
        assert_eq!(tool.transfer_detail(start), "0.5/1 kB");
    }

    #[test]
    fn a_resumed_download_does_not_inflate_the_rate() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].transfer = Some(transfer(0, 100_000_000, start));
        let (shared, progress) = tool_progress(state);
        // 90 MB was already on disk; only the 10 MB since counts toward rate.
        progress.set_position(90_000_000);
        progress.inc(10_000_000);
        let shared = shared.lock().unwrap();
        let detail = shared.tools[0].transfer_detail(start + Duration::from_secs(2));
        assert_eq!(detail, "100.0/100.0 MB · 5.0 MB/s");
    }

    #[test]
    fn a_retried_attempt_starts_its_rate_from_zero() {
        let (shared, progress) = tool_progress(state(Instant::now()));
        progress.set_length(100);
        progress.inc(60);
        // Same total announced again: a new attempt, resuming at 60.
        progress.set_length(100);
        progress.set_position(60);
        progress.inc(10);
        let transfer = shared.lock().unwrap().tools[0].transfer.unwrap();
        assert_eq!((transfer.done, transfer.resumed_at), (70, 60));
    }

    #[test]
    fn bytes_without_a_length_still_show_a_running_count() {
        let start = Instant::now();
        let (shared, progress) = tool_progress(state(start));
        progress.inc(1_500_000);
        let state = shared.lock().unwrap();
        assert_eq!(state.tools[0].transfer_detail(start), "1.5 MB");
        // No total, so no fraction: the bar must not pretend to know.
        assert_eq!(state.tools[0].transfer.unwrap().fraction(), None);
    }

    #[test]
    fn completion_line_names_the_artifact_and_says_when_it_was_reused() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        let (shared, progress) = tool_progress(state);
        progress.set_message("cached node-v24.20.0-linux-x64.tar.xz".into());
        {
            let state = shared.lock().unwrap();
            assert_eq!(state.tools[0].message, "reusing download");
            assert!(state.tools[0].reused);
        }
        progress.set_message("extract node-v24.20.0-linux-x64.tar.xz".into());
        let line = shared
            .lock()
            .unwrap()
            .finish_tool(
                0,
                Outcome::Installed,
                start + Duration::from_millis(300),
                None,
            )
            .unwrap();
        let line = console::strip_ansi_codes(&line).into_owned();
        assert_eq!(
            line,
            "✓ tool0@1  300ms · cached  node-v24.20.0-linux-x64.tar.xz"
        );
    }

    #[test]
    fn the_child_row_keeps_item_tallies_and_leaves_bytes_to_the_parent() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        let tool = &mut state.tools[0];
        // A package manager's count is the child's, with or without a detail.
        tool.set_items(3, 10);
        assert_eq!(tool.child_detail(start).as_deref(), Some("3/10"));
        tool.set_detail("3/10 pkgs · 1.2 MiB".into());
        assert_eq!(
            tool.child_detail(start).as_deref(),
            Some("3/10 pkgs · 1.2 MiB")
        );
        // A download is the parent row's figure; without a detail there is
        // nothing left for a child row to say.
        tool.set_detail(String::new());
        tool.set_length(1_000);
        tool.inc(500);
        assert_eq!(tool.child_detail(start), None);
        tool.set_detail("verifying checksum".into());
        assert_eq!(
            tool.child_detail(start).as_deref(),
            Some("verifying checksum")
        );
    }

    #[test]
    fn a_narrow_terminal_drops_the_artifact_rather_than_wrapping_the_line() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        state.tools[0].artifact = Some("node-v24.20.0-linux-x64.tar.xz".into());
        state.tools[0].outcome = Some(Outcome::Installed);
        let line = |columns| {
            let line = state.tools[0].completion_line(
                state.width(),
                start + Duration::from_millis(300),
                columns,
            );
            console::strip_ansi_codes(&line).into_owned()
        };
        // Unbounded (the append-only reporter) and roomy terminals keep it.
        assert_eq!(
            line(None),
            "✓ tool0@1  300ms  node-v24.20.0-linux-x64.tar.xz"
        );
        assert_eq!(line(Some(80)), line(None));
        // At 40 columns the full line would wrap; the artifact is the part
        // that goes, and the line is not padded out to the width it gave up.
        assert_eq!(line(Some(40)), "✓ tool0@1  300ms");
    }

    #[test]
    fn the_bar_keeps_a_cell_open_until_everything_is_done() {
        let start = Instant::now();
        let mut state = State::new(std::iter::once(("0".to_string(), "tool0@1".to_string())));
        state.started = start;
        state.tools[0].started = Some(start);
        state.tools[0].start_operations(&[1.0]);
        state.tools[0].apply_message("running custom postinstall hook".into());
        assert_eq!(state.tools[0].fraction, 0.99);
        let bar = console::strip_ansi_codes(&state.bar_only(24)).into_owned();
        assert_eq!(bar, "███████████████████████░");
        state.finish_tool(0, Outcome::Installed, start, None);
        let bar = console::strip_ansi_codes(&state.bar_only(24)).into_owned();
        assert_eq!(bar, "████████████████████████");
    }

    #[test]
    fn failures_take_their_share_of_the_bar_in_red() {
        let start = Instant::now();
        let mut state = state(start);
        state.finish_tool(0, Outcome::Installed, start, None);
        state.finish_tool(1, Outcome::Failed, start, None);
        state.finish_tool(2, Outcome::Failed, start, None);
        let bar = state.bar();
        let plain = console::strip_ansi_codes(&bar).into_owned();
        assert_eq!(plain, "████████████████ 3/3");
        // Two of three tools failed: eleven red cells follow five cyan ones.
        let red = format!("{}", style::ered("█".repeat(11)));
        let cyan = format!("{}", style::ecyan("█".repeat(5)));
        assert!(
            !console::colors_enabled_stderr() || (bar.contains(&red) && bar.contains(&cyan)),
            "{bar:?}"
        );
    }

    #[test]
    fn the_postinstall_hook_completes_every_declared_operation() {
        let (shared, progress) = tool_progress(state(Instant::now()));
        // A plugin backend that never calls next_operation.
        progress.start_operations(3);
        assert_eq!(shared.lock().unwrap().tools[0].fraction, 0.0);
        progress.set_message("running custom postinstall hook".into());
        let state = shared.lock().unwrap();
        assert_eq!(state.tools[0].completed_ops, 3);
        assert_eq!(state.tools[0].fraction, 0.99);
    }

    #[test]
    fn stopping_joins_the_heartbeat_without_waiting_for_a_tick() {
        let start = Instant::now();
        let mut progress = TextInstallProgress::new(State::new(std::iter::once((
            "tool".to_string(),
            "tool@1".to_string(),
        ))));
        progress.stop();
        assert!(progress.thread.is_none());
        assert!(start.elapsed() < INTERVAL);
    }

    #[test]
    fn an_unknown_request_falls_back_instead_of_panicking() {
        let progress = TextInstallProgress::new(State::new(std::iter::once((
            "tool".to_string(),
            "tool@1".to_string(),
        ))));
        assert!(progress.start_tool("tool").is_some());
        assert!(progress.start_tool("other").is_none());
    }
}
