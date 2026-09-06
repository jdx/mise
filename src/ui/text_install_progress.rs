//! Append-only install progress for captured stderr and CI terminals.
use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::multi_progress_report::MultiProgressReport;
use super::progress_report::{ProgressIcon, SingleReport};
use super::style;

const INTERVAL: Duration = Duration::from_secs(3);
const BAR_WIDTH: usize = 16;

#[derive(Debug)]
struct Tool {
    key: String,
    prefix: String,
    started: Option<Instant>,
    message: String,
    outcome: Option<Outcome>,
    skipped: bool,
    /// Relative cost of each install operation, in the order they run, as the
    /// backend estimated it. Empty until the backend declares its plan.
    weights: Vec<f64>,
    completed_ops: usize,
    transfer: Option<Transfer>,
    /// The file the backend chose to download, from its `download <name>`
    /// status. Kept for the completion line: "which artifact did it pick?" is a
    /// question people ask the log after the fact, and a fast install never
    /// lives long enough to appear in a snapshot.
    artifact: Option<String>,
    /// The backend found the artifact already downloaded and skipped the fetch.
    /// Without saying so, a 0.3s install of an 80 MB tool looks like a lie.
    reused: bool,
    /// Never decreases. A backend that restarts a download (a range request
    /// that the server refuses, a retry) would otherwise walk the bar
    /// backwards, which reads as the install having gone wrong.
    fraction: f64,
}

/// Byte progress for the operation currently running.
#[derive(Debug, Clone, Copy)]
struct Transfer {
    done: u64,
    total: u64,
    started: Instant,
    /// Bytes already on disk when this transfer began, from a resumed
    /// download. Excluded from the rate so a resume does not report a
    /// throughput the network never achieved.
    resumed_at: u64,
}

impl Transfer {
    fn new(total: u64) -> Self {
        Self {
            done: 0,
            total,
            started: Instant::now(),
            resumed_at: 0,
        }
    }

    fn fraction(&self) -> Option<f64> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64).clamp(0.0, 1.0))
    }

    /// Average over this transfer rather than an instantaneous rate: the
    /// snapshot only lands every few seconds, so a sampled rate would report
    /// whichever moment it happened to catch.
    fn rate(&self, now: Instant) -> Option<f64> {
        let seconds = now.saturating_duration_since(self.started).as_secs_f64();
        let moved = self.done.saturating_sub(self.resumed_at);
        (seconds > 0.5 && moved > 0).then(|| moved as f64 / seconds)
    }
}

impl Tool {
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

    fn advance(&mut self) {
        self.fraction = self.compute_fraction().max(self.fraction);
    }

    /// `42.1/78.3 MB · 12.4 MB/s`, dropping whichever half is unknown. A server
    /// that sends no content-length still gets a running byte count.
    fn transfer_detail(&self, now: Instant) -> String {
        let Some(transfer) = self.transfer else {
            return String::new();
        };
        if transfer.done == 0 && transfer.total == 0 {
            return String::new();
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
        match transfer.rate(now) {
            Some(rate) => format!("{bytes} · {}/s", format_bytes(rate.round() as u64)),
            None => bytes,
        }
    }
}

fn format_bytes(bytes: u64) -> String {
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
    } else if total as f64 >= KB {
        (KB, 0)
    } else {
        (1.0, 0)
    };
    format!("{:.*}", decimals, bytes as f64 / unit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Installed,
    Skipped,
    Failed,
}

#[derive(Debug)]
struct State {
    tools: Vec<Tool>,
    started: Instant,
}

impl State {
    fn width(&self) -> usize {
        self.tools
            .iter()
            .map(|t| console::measure_text_width(&t.prefix))
            .max()
            .unwrap_or(0)
    }

    /// The bar fills fractionally — a tool halfway through its own install
    /// occupies half the width a finished one would — while the count beside it
    /// stays whole tools, which is the number that can be verified against the
    /// completion lines above.
    fn bar(&self) -> String {
        let total = self.tools.len();
        let complete = self.tools.iter().filter(|t| t.outcome.is_some()).count();
        let (progress, failed_share) = if total == 0 {
            (1.0, 0.0)
        } else {
            let failed = self
                .tools
                .iter()
                .filter(|t| t.outcome == Some(Outcome::Failed))
                .count();
            (
                self.tools.iter().map(|t| t.fraction).sum::<f64>() / total as f64,
                failed as f64 / total as f64,
            )
        };
        let filled = ((progress * BAR_WIDTH as f64).round() as usize).min(BAR_WIDTH);
        // A failed tool still counts as finished work, but a fully cyan bar over
        // "installed 0 tools · 1 failed" reads as success. Its share is red.
        let failed = ((failed_share * BAR_WIDTH as f64).round() as usize).min(filled);
        format!(
            "{}{}{} {complete}/{total}",
            style::ecyan("█".repeat(filled - failed)),
            style::ered("█".repeat(failed)),
            style::edim("░".repeat(BAR_WIDTH - filled))
        )
    }

    fn snapshot(&self, now: Instant) -> String {
        let mut lines = vec![format!("{} · {}", self.bar(), elapsed(self.started, now))];
        // Columns size to their content on every snapshot. A fixed width would
        // either truncate a status like "running postinstall hook" or reserve
        // blank space for a transfer column most rows never fill.
        let rows: Vec<_> = self
            .tools
            .iter()
            .filter(|t| t.outcome.is_none())
            .filter_map(|tool| {
                let started = tool.started?;
                Some((
                    tool.prefix.as_str(),
                    tool.message.as_str(),
                    tool.transfer_detail(now),
                    elapsed(started, now),
                    tool.artifact.as_deref(),
                ))
            })
            .collect();
        let width = self.width();
        let message_width = column_width(rows.iter().map(|r| r.1));
        let detail_width = column_width(rows.iter().map(|r| r.2.as_str()));
        for (prefix, message, detail, elapsed, artifact) in rows {
            let mut line = format!(
                "  {}  {}",
                console::pad_str(prefix, width, console::Alignment::Left, None),
                console::pad_str(message, message_width, console::Alignment::Left, None),
            );
            if detail_width > 0 {
                line.push_str("  ");
                line.push_str(&console::pad_str(
                    &detail,
                    detail_width,
                    console::Alignment::Left,
                    None,
                ));
            }
            line.push_str(&format!("  {elapsed}"));
            if let Some(artifact) = artifact {
                // Last, dimmed, so its length cannot disturb the columns.
                line.push_str(&format!("  {}", style::edim(artifact)));
            }
            lines.push(line);
        }
        let queued = self
            .tools
            .iter()
            .filter(|t| t.started.is_none() && t.outcome.is_none())
            .count();
        if queued > 0 {
            lines.push(format!("  {queued} queued"));
        }
        lines.join("\n")
    }

    fn finish_tool(&mut self, index: usize, outcome: Outcome, now: Instant) -> Option<String> {
        let width = self.width();
        let tool = &mut self.tools[index];
        if tool.outcome.is_some() {
            return None;
        }
        tool.outcome = Some(outcome);
        tool.fraction = 1.0;
        tool.transfer = None;
        let prefix = console::pad_str(&tool.prefix, width, console::Alignment::Left, None);
        let duration = tool
            .started
            .map(|started| format!("  {}", elapsed(started, now)))
            .unwrap_or_default();
        let (icon, detail) = match outcome {
            Outcome::Installed if tool.reused => (ProgressIcon::Success, " · cached".into()),
            Outcome::Installed => (ProgressIcon::Success, String::new()),
            Outcome::Skipped => (ProgressIcon::Skipped, " · already installed".into()),
            Outcome::Failed => (ProgressIcon::Error, format!(" · failed: {}", tool.message)),
        };
        let artifact = match (&tool.artifact, outcome) {
            (Some(artifact), Outcome::Installed) => format!("  {}", style::edim(artifact)),
            _ => String::new(),
        };
        Some(format!("{icon} {prefix}{duration}{detail}{artifact}"))
    }

    fn summary(&self, now: Instant) -> String {
        let installed = self
            .tools
            .iter()
            .filter(|t| t.outcome == Some(Outcome::Installed))
            .count();
        let skipped = self
            .tools
            .iter()
            .filter(|t| t.outcome == Some(Outcome::Skipped))
            .count();
        let failed = self
            .tools
            .iter()
            .filter(|t| t.outcome == Some(Outcome::Failed))
            .count();
        let mut result = format!(
            "{} · installed {installed} {}",
            self.bar(),
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

fn first_line(error: &str) -> String {
    error.lines().next().unwrap_or("installation failed").into()
}

fn column_width<'a>(cells: impl Iterator<Item = &'a str>) -> usize {
    cells.map(console::measure_text_width).max().unwrap_or(0)
}

fn tool_noun(count: usize) -> &'static str {
    if count == 1 { "tool" } else { "tools" }
}

fn elapsed(start: Instant, now: Instant) -> String {
    let duration = now.duration_since(start);
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
    pub(crate) fn new(tools: impl Iterator<Item = (String, String)>) -> Self {
        // Match the dependency scheduler's deduplication of repeated requests.
        let mut seen = HashSet::new();
        let state = Arc::new(Mutex::new(State {
            started: Instant::now(),
            tools: tools
                .filter(|(key, _)| seen.insert(key.clone()))
                .map(|(key, prefix)| Tool {
                    key,
                    prefix,
                    started: None,
                    message: "queued".into(),
                    outcome: None,
                    skipped: false,
                    weights: vec![],
                    completed_ops: 0,
                    transfer: None,
                    artifact: None,
                    reused: false,
                    fraction: 0.0,
                })
                .collect(),
        }));
        let total = state.lock().unwrap().tools.len();
        info!("installing {total} {}", tool_noun(total));
        let (stop, rx) = mpsc::channel();
        let shared = state.clone();
        let thread = thread::spawn(move || {
            while rx.recv_timeout(INTERVAL) == Err(mpsc::RecvTimeoutError::Timeout) {
                let render = || {
                    let state = shared.lock().unwrap();
                    // Completed tools have already printed their individual results.
                    if state.tools.iter().any(|t| t.outcome.is_none()) {
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

    /// Returns `None` for a request that was not part of this session, letting the
    /// caller fall back to the standard reporter instead of panicking mid-install.
    pub(crate) fn start_tool(&self, key: &str) -> Option<TextToolProgress> {
        let mut state = self.state.lock().unwrap();
        let index = state.tools.iter().position(|t| t.key == key)?;
        let tool = &mut state.tools[index];
        tool.started = Some(Instant::now());
        tool.message = "resolving".into();
        Some(TextToolProgress {
            state: self.state.clone(),
            index,
        })
    }

    pub(crate) fn finish(&mut self, failures: impl Iterator<Item = (String, String)>) {
        self.stop();
        let mut state = self.state.lock().unwrap();
        let now = Instant::now();
        for (key, error) in failures {
            if let Some(index) = state
                .tools
                .iter()
                .position(|t| t.key == key && t.outcome.is_none())
            {
                state.tools[index].message = first_line(&error);
                if let Some(line) = state.finish_tool(index, Outcome::Failed, now) {
                    info!("{line}");
                }
            }
        }
        info!("{}", state.summary(now));
    }

    fn stop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
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
    /// Mutate this tool's state and refresh its cached fraction. Every progress
    /// signal goes through here so the bar can never be stale relative to the
    /// rows printed beside it.
    fn with_tool(&self, f: impl FnOnce(&mut Tool)) {
        let mut state = self.state.lock().unwrap();
        let tool = &mut state.tools[self.index];
        f(tool);
        tool.advance();
    }

    pub(crate) fn set_prefix(&self, prefix: String) {
        self.state.lock().unwrap().tools[self.index].prefix = prefix;
    }

    /// The worker result is authoritative: backends sometimes finish their
    /// report before their postinstall work has actually returned.
    ///
    /// `error` is the worker's own failure. Without it the line would report
    /// whatever phase the tool happened to be in when it died, which reads as a
    /// reason ("failed: ✓ Cosign verified") without being one.
    pub(crate) fn complete(&self, error: Option<&str>) {
        let mut state = self.state.lock().unwrap();
        let outcome = match error {
            Some(error) => {
                state.tools[self.index].message = first_line(error);
                Outcome::Failed
            }
            None if state.tools[self.index].skipped => Outcome::Skipped,
            None => Outcome::Installed,
        };
        if let Some(line) = state.finish_tool(self.index, outcome, Instant::now()) {
            info!("{line}");
        }
    }
}

impl SingleReport for TextToolProgress {
    fn set_message(&self, message: String) {
        // Keep counters from embedded package managers, but omit artifact names
        // from the common download/checksum/extract status messages.
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
            Some("extract") => ("extracting", None),
            Some("install") => ("installing", None),
            Some("running") if message == "running custom postinstall hook" => {
                operations_done = true;
                ("running postinstall hook", None)
            }
            _ => (message.as_str(), None),
        };
        let phase = phase.to_string();
        let artifact = artifact.map(str::to_string);
        self.with_tool(|tool| {
            tool.message = phase;
            tool.reused |= reused;
            if operations_done {
                tool.completed_ops = tool.weights.len();
                tool.transfer = None;
            }
            if let Some(artifact) = artifact {
                tool.artifact = Some(artifact);
            }
        });
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
        self.with_tool(|tool| tool.weights = vec![1.0; count.max(1)]);
    }

    fn start_operations_weighted(&self, weights: &[f64]) {
        let weights: Vec<f64> = weights.iter().copied().filter(|w| *w > 0.0).collect();
        self.with_tool(|tool| {
            tool.weights = if weights.is_empty() {
                vec![1.0]
            } else {
                weights.clone()
            }
        });
    }

    fn next_operation(&self) {
        self.with_tool(|tool| {
            tool.completed_ops = (tool.completed_ops + 1).min(tool.weights.len());
            // Byte progress belongs to the operation that just ended.
            tool.transfer = None;
        });
    }

    fn set_length(&self, length: u64) {
        // A length always opens a new transfer, even when it equals the last
        // one: the downloader announces it once per attempt, and a retry that
        // kept the previous state would fold the failed attempt's bytes and
        // time into this one's rate.
        self.with_tool(|tool| tool.transfer = Some(Transfer::new(length)));
    }

    fn set_position(&self, position: u64) {
        self.with_tool(|tool| {
            // A server that sends no content-length never calls set_length, so
            // the first bytes open a transfer with an unknown total.
            let transfer = tool.transfer.get_or_insert_with(|| Transfer::new(0));
            // A resumed download reports its starting offset here.
            if transfer.done == 0 && position > 0 {
                transfer.resumed_at = position;
            }
            transfer.done = position;
        });
    }

    fn inc(&self, delta: u64) {
        self.with_tool(|tool| {
            let transfer = tool.transfer.get_or_insert_with(|| Transfer::new(0));
            transfer.done = transfer.done.saturating_add(delta);
        });
    }

    fn finish_with_icon(&self, _message: String, icon: ProgressIcon) {
        self.state.lock().unwrap().tools[self.index].skipped =
            matches!(icon, ProgressIcon::Skipped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(started: Instant) -> State {
        State {
            started,
            tools: (0..3)
                .map(|i| Tool {
                    key: i.to_string(),
                    prefix: format!("tool{i}@1"),
                    started: None,
                    message: "queued".into(),
                    outcome: None,
                    skipped: false,
                    weights: vec![],
                    completed_ops: 0,
                    transfer: None,
                    artifact: None,
                    reused: false,
                    fraction: 0.0,
                })
                .collect(),
        }
    }

    #[test]
    fn snapshots_separate_queue_time_from_work_time() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        state.finish_tool(0, Outcome::Installed, start + Duration::from_secs(1));
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
        assert!(snapshot.ends_with("1 queued"));
    }

    #[test]
    fn terminal_results_are_not_reported_or_counted_twice() {
        let start = Instant::now();
        let mut state = state(start);
        state.tools[0].started = Some(start);
        let finished = state
            .finish_tool(0, Outcome::Installed, start + Duration::from_millis(250))
            .unwrap();
        assert!(finished.contains("250ms"));
        assert!(state.finish_tool(0, Outcome::Failed, start).is_none());
        state.finish_tool(1, Outcome::Skipped, start);
        state.finish_tool(2, Outcome::Failed, start);
        let summary = console::strip_ansi_codes(&state.summary(start)).into_owned();
        assert_eq!(
            summary,
            "████████████████ 3/3 · installed 1 tool · 1 already installed · 1 failed in 0ms"
        );
    }

    #[test]
    fn backend_finish_does_not_complete_a_worker() {
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
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
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
        // A phase message can even read as a success on its own.
        progress.set_message("✓ Cosign verified".into());
        progress.complete(Some("checksum mismatch\nsecond line"));
        assert_eq!(shared.lock().unwrap().tools[0].message, "checksum mismatch");
    }

    #[test]
    fn a_skip_survives_a_successful_completion() {
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
        progress.finish_with_icon("already installed".into(), ProgressIcon::Skipped);
        progress.complete(None);
        assert_eq!(
            shared.lock().unwrap().tools[0].outcome,
            Some(Outcome::Skipped)
        );
    }

    #[test]
    fn stopping_joins_the_heartbeat_without_waiting_for_a_tick() {
        let start = Instant::now();
        let mut progress =
            TextInstallProgress::new(std::iter::once(("tool".into(), "tool@1".into())));
        progress.stop();
        assert!(progress.thread.is_none());
        assert!(start.elapsed() < INTERVAL);
    }

    #[test]
    fn a_half_done_tool_fills_half_the_width_of_a_finished_one() {
        let start = Instant::now();
        let mut state = state(start);
        // One finished, one exactly halfway through a single-operation install.
        state.tools[0].started = Some(start);
        state.finish_tool(0, Outcome::Installed, start);
        state.tools[1].started = Some(start);
        state.tools[1].weights = vec![1.0];
        state.tools[1].transfer = Some(Transfer {
            done: 50,
            total: 100,
            started: start,
            resumed_at: 0,
        });
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
        tool.weights = vec![0.7, 0.15, 0.15];
        tool.advance();
        assert_eq!(tool.fraction, 0.0);
        // Halfway through the download is 35% of the tool, not 1/6.
        tool.transfer = Some(Transfer {
            done: 1,
            total: 2,
            started: start,
            resumed_at: 0,
        });
        tool.advance();
        assert!((tool.fraction - 0.35).abs() < 1e-9, "{}", tool.fraction);
        // The tail stays reserved for the work that follows the last operation.
        tool.completed_ops = 3;
        tool.transfer = None;
        tool.advance();
        assert_eq!(tool.fraction, 0.99);
    }

    #[test]
    fn progress_never_walks_backwards() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.weights = vec![1.0];
        tool.transfer = Some(Transfer {
            done: 90,
            total: 100,
            started: start,
            resumed_at: 0,
        });
        tool.advance();
        let high = tool.fraction;
        // A restarted download reports byte zero again.
        tool.transfer = Some(Transfer {
            done: 0,
            total: 100,
            started: start,
            resumed_at: 0,
        });
        tool.advance();
        assert_eq!(tool.fraction, high);
    }

    #[test]
    fn transfer_detail_reports_bytes_and_rate_in_one_unit() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.transfer = Some(Transfer {
            done: 42_100_000,
            total: 78_300_000,
            started: start,
            resumed_at: 0,
        });
        let detail = tool.transfer_detail(start + Duration::from_secs(4));
        assert_eq!(detail, "42.1/78.3 MB · 10.5 MB/s");

        // A server that sends no length still gets a running count, and a rate
        // needs enough of a sample to mean anything.
        tool.transfer = Some(Transfer {
            done: 1_500_000,
            total: 0,
            started: start,
            resumed_at: 0,
        });
        assert_eq!(
            tool.transfer_detail(start + Duration::from_millis(100)),
            "1.5 MB"
        );
    }

    #[test]
    fn a_resumed_download_does_not_inflate_the_rate() {
        let start = Instant::now();
        let mut state = state(start);
        let tool = &mut state.tools[0];
        tool.transfer = Some(Transfer {
            done: 0,
            total: 100_000_000,
            started: start,
            resumed_at: 0,
        });
        // 90 MB was already on disk; only the 10 MB since counts toward rate.
        let progress = TextToolProgress {
            state: Arc::new(Mutex::new(state)),
            index: 0,
        };
        progress.set_position(90_000_000);
        progress.inc(10_000_000);
        let shared = progress.state.lock().unwrap();
        let detail = shared.tools[0].transfer_detail(start + Duration::from_secs(2));
        assert_eq!(detail, "100.0/100.0 MB · 5.0 MB/s");
    }

    #[test]
    fn completion_line_names_the_artifact_and_says_when_it_was_reused() {
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        shared.lock().unwrap().tools[0].started = Some(start);
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
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
            .finish_tool(0, Outcome::Installed, start + Duration::from_millis(300))
            .unwrap();
        let line = console::strip_ansi_codes(&line).into_owned();
        assert_eq!(
            line,
            "✓ tool0@1  300ms · cached  node-v24.20.0-linux-x64.tar.xz"
        );
    }

    #[test]
    fn failures_take_their_share_of_the_bar_in_red() {
        let start = Instant::now();
        let mut state = state(start);
        state.finish_tool(0, Outcome::Installed, start);
        state.finish_tool(1, Outcome::Failed, start);
        state.finish_tool(2, Outcome::Failed, start);
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
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
        // A plugin backend that never calls next_operation.
        progress.start_operations(3);
        assert_eq!(shared.lock().unwrap().tools[0].fraction, 0.0);
        progress.set_message("running custom postinstall hook".into());
        let state = shared.lock().unwrap();
        assert_eq!(state.tools[0].completed_ops, 3);
        assert_eq!(state.tools[0].fraction, 0.99);
    }

    #[test]
    fn a_retried_attempt_starts_its_rate_from_zero() {
        let start = Instant::now();
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
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
        let shared = Arc::new(Mutex::new(state(start)));
        let progress = TextToolProgress {
            state: shared.clone(),
            index: 0,
        };
        progress.inc(1_500_000);
        let state = shared.lock().unwrap();
        assert_eq!(state.tools[0].transfer_detail(start), "1.5 MB");
        // No total, so no fraction: the bar must not pretend to know.
        assert_eq!(state.tools[0].transfer.unwrap().fraction(), None);
    }

    #[test]
    fn an_unknown_request_falls_back_instead_of_panicking() {
        let progress = TextInstallProgress::new(std::iter::once(("tool".into(), "tool@1".into())));
        assert!(progress.start_tool("tool").is_some());
        assert!(progress.start_tool("other").is_none());
    }
}
