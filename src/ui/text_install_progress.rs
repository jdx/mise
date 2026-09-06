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

    fn bar(&self) -> String {
        let total = self.tools.len();
        let complete = self.tools.iter().filter(|t| t.outcome.is_some()).count();
        let filled = (complete * BAR_WIDTH)
            .checked_div(total)
            .unwrap_or(BAR_WIDTH);
        format!(
            "{}{} {complete}/{total}",
            style::ecyan("█".repeat(filled)),
            style::edim("░".repeat(BAR_WIDTH - filled))
        )
    }

    fn snapshot(&self, now: Instant) -> String {
        let mut lines = vec![format!("{} · {}", self.bar(), elapsed(self.started, now))];
        let width = self.width();
        for tool in self.tools.iter().filter(|t| t.outcome.is_none()) {
            if let Some(started) = tool.started {
                let prefix = console::pad_str(&tool.prefix, width, console::Alignment::Left, None);
                let message =
                    console::pad_str(&tool.message, 30, console::Alignment::Left, Some("…"));
                lines.push(format!("  {prefix}  {message}  {}", elapsed(started, now)));
            }
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
        let prefix = console::pad_str(&tool.prefix, width, console::Alignment::Left, None);
        let duration = tool
            .started
            .map(|started| format!("  {}", elapsed(started, now)))
            .unwrap_or_default();
        let (icon, detail) = match outcome {
            Outcome::Installed => (ProgressIcon::Success, String::new()),
            Outcome::Skipped => (ProgressIcon::Skipped, " · already installed".into()),
            Outcome::Failed => (ProgressIcon::Error, format!(" · failed: {}", tool.message)),
        };
        Some(format!("{icon} {prefix}{duration}{detail}"))
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
        let message = match message.split_whitespace().next() {
            Some("download") => "downloading",
            Some("checksum") => "verifying checksum",
            Some("extract") => "extracting",
            Some("install") => "installing",
            Some("running") if message == "running custom postinstall hook" => {
                "running postinstall hook"
            }
            _ => &message,
        };
        self.state.lock().unwrap().tools[self.index].message = message.into();
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
        assert!(!snapshot.contains("tool0"));
        assert!(snapshot.contains("tool1@1  extracting                      1.0s"));
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
    fn an_unknown_request_falls_back_instead_of_panicking() {
        let progress = TextInstallProgress::new(std::iter::once(("tool".into(), "tool@1".into())));
        assert!(progress.start_tool("tool").is_some());
        assert!(progress.start_tool("other").is_none());
    }
}
