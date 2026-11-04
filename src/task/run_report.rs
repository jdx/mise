//! The JSON report `mise run --report-file` writes when the run ends.
//!
//! Every task the run schedules is listed, in the order the run created it, with how it
//! ended. A task the run never reached stays `not_run`, so a reader can tell "skipped
//! because up to date" from "never started because something else failed".

use std::path::Path;
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use indexmap::IndexMap;
use itertools::Itertools;
use serde::Serialize;

use crate::Result;
use crate::task::{Task, TaskKey, task_key};

/// The env var that sets `mise run --report-file`. mise removes it from the environment of
/// the tasks it runs, so a nested `mise run` does not overwrite the outer run's report.
pub const REPORT_FILE_ENV: &str = "MISE_TASK_REPORT_FILE";

/// Bumped when a field changes meaning or goes away; new fields do not bump it.
const REPORT_VERSION: u32 = 1;

/// How one task ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskReportStatus {
    /// Ran and exited 0.
    Passed,
    /// Ran, or failed to start, and did not succeed.
    Failed,
    /// Stopped by an interrupt, by `--timeout`, or by mise after another task failed.
    Cancelled,
    /// Not run: sources up to date, `skip_if` met, or listed in `task.skip`.
    Skipped,
    /// Its result was restored from the artifact cache.
    Cached,
    /// Scheduled, but the run ended before it started.
    NotRun,
}

/// How the whole run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunReportStatus {
    Passed,
    Failed,
    Cancelled,
    TimedOut,
}

struct Entry {
    name: String,
    args: Vec<String>,
    status: TaskReportStatus,
    exit_code: Option<i32>,
    started: Option<(SystemTime, Instant)>,
    ended: Option<(SystemTime, Instant)>,
}

#[derive(Default)]
struct State {
    tasks: IndexMap<TaskKey, Entry>,
    timed_out: bool,
}

/// Collects what happened to each task of a run. Shared by the run and its task jobs, and
/// kept outside the run itself so a `--timeout` that drops the run can still write it.
pub struct RunReport {
    started: (SystemTime, Instant),
    state: Mutex<State>,
}

#[derive(Serialize)]
struct TaskJson<'a> {
    name: &'a str,
    args: &'a [String],
    status: TaskReportStatus,
    exit_code: Option<i32>,
    duration_ms: Option<u128>,
    started_at: Option<String>,
    ended_at: Option<String>,
}

#[derive(Serialize)]
struct RunJson<'a> {
    version: u32,
    status: RunReportStatus,
    started_at: String,
    ended_at: String,
    duration_ms: u128,
    tasks: Vec<TaskJson<'a>>,
}

fn timestamp(time: SystemTime) -> String {
    jiff::Timestamp::try_from(time)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

impl Default for RunReport {
    fn default() -> Self {
        Self::new()
    }
}

impl RunReport {
    pub fn new() -> Self {
        Self {
            started: (SystemTime::now(), Instant::now()),
            state: Mutex::new(State::default()),
        }
    }

    fn entry<'a>(state: &'a mut State, task: &Task, args: &[String]) -> &'a mut Entry {
        state.tasks.entry(task_key(task)).or_insert_with(|| Entry {
            name: task.name.clone(),
            args: args.to_vec(),
            status: TaskReportStatus::NotRun,
            exit_code: None,
            started: None,
            ended: None,
        })
    }

    /// Lists a task the run will try to run. `args` are already redacted.
    pub fn schedule(&self, task: &Task, args: &[String]) {
        Self::entry(&mut self.state.lock().unwrap(), task, args);
    }

    /// Records that a task started. Also lists a task injected by a `run` entry.
    pub fn start(&self, task: &Task, args: &[String]) {
        let mut state = self.state.lock().unwrap();
        let entry = Self::entry(&mut state, task, args);
        entry.started = Some((SystemTime::now(), Instant::now()));
        entry.ended = None;
    }

    /// Records how a started task ended. `NotRun` takes back the start of a task that was
    /// abandoned before it ran anything.
    pub fn finish(&self, task: &Task, status: TaskReportStatus, exit_code: Option<i32>) {
        let mut state = self.state.lock().unwrap();
        let timed_out = state.timed_out;
        let entry = Self::entry(&mut state, task, &task.args);
        if status == TaskReportStatus::NotRun {
            entry.status = status;
            entry.exit_code = None;
            entry.started = None;
            entry.ended = None;
            return;
        }
        let now = (SystemTime::now(), Instant::now());
        entry.started.get_or_insert(now);
        entry.ended = Some(now);
        entry.exit_code = exit_code;
        // After `--timeout`, a task that fails was stopped by mise.
        entry.status = if timed_out && status == TaskReportStatus::Failed {
            TaskReportStatus::Cancelled
        } else {
            status
        };
    }

    pub fn mark_timed_out(&self) {
        self.state.lock().unwrap().timed_out = true;
    }

    /// Builds the report. A task still running when the run ended was cancelled.
    pub fn to_json(&self, run_failed: bool, interrupted: bool) -> Result<String> {
        let mut state = self.state.lock().unwrap();
        let now = (SystemTime::now(), Instant::now());
        for entry in state.tasks.values_mut() {
            if entry.started.is_some() && entry.ended.is_none() {
                entry.ended = Some(now);
                entry.status = TaskReportStatus::Cancelled;
            }
        }
        let any = |status| state.tasks.values().any(|e| e.status == status);
        let status = if state.timed_out {
            RunReportStatus::TimedOut
        } else if any(TaskReportStatus::Failed) {
            RunReportStatus::Failed
        } else if interrupted || any(TaskReportStatus::Cancelled) {
            RunReportStatus::Cancelled
        } else if run_failed {
            RunReportStatus::Failed
        } else {
            RunReportStatus::Passed
        };
        // In the order they started, then the tasks that never did, in schedule order.
        let tasks = state
            .tasks
            .values()
            .sorted_by_key(|e| (e.started.is_none(), e.started.map(|(_, at)| at)))
            .map(|e| TaskJson {
                name: &e.name,
                args: &e.args,
                status: e.status,
                exit_code: e.exit_code,
                duration_ms: match (e.started, e.ended) {
                    (Some((_, start)), Some((_, end))) => Some((end - start).as_millis()),
                    _ => None,
                },
                started_at: e.started.map(|(t, _)| timestamp(t)),
                ended_at: e.ended.map(|(t, _)| timestamp(t)),
            })
            .collect();
        let json = RunJson {
            version: REPORT_VERSION,
            status,
            started_at: timestamp(self.started.0),
            ended_at: timestamp(now.0),
            duration_ms: (now.1 - self.started.1).as_millis(),
            tasks,
        };
        Ok(serde_json::to_string_pretty(&json)? + "\n")
    }

    /// Writes the report to `path`, replacing it atomically so a reader never sees half.
    pub fn write(&self, path: &Path, run_failed: bool, interrupted: bool) -> Result<()> {
        let json = self.to_json(run_failed, interrupted)?;
        crate::file::write_atomic(path, json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(name: &str) -> Task {
        Task {
            name: name.to_string(),
            ..Default::default()
        }
    }

    fn report_json(report: &RunReport, run_failed: bool) -> serde_json::Value {
        serde_json::from_str(&report.to_json(run_failed, false).unwrap()).unwrap()
    }

    #[test]
    fn lists_tasks_in_schedule_order_with_their_status() {
        let report = RunReport::new();
        let (a, b, c) = (task("a"), task("b"), task("c"));
        report.schedule(&a, &[]);
        report.schedule(&b, &["x".to_string()]);
        report.schedule(&c, &[]);
        report.start(&b, &["x".to_string()]);
        report.finish(&b, TaskReportStatus::Failed, Some(3));
        // Two starts in the same tick would keep schedule order.
        std::thread::sleep(std::time::Duration::from_millis(2));
        report.start(&a, &[]);
        report.finish(&a, TaskReportStatus::Passed, Some(0));

        let json = report_json(&report, true);
        assert_eq!(json["status"], "failed");
        assert_eq!(json["version"], 1);
        let tasks = json["tasks"].as_array().unwrap();
        // Started tasks in start order, then the rest in schedule order.
        let names: Vec<_> = tasks.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["b", "a", "c"]);
        assert_eq!(tasks[0]["status"], "failed");
        assert_eq!(tasks[0]["exit_code"], 3);
        assert_eq!(tasks[0]["args"][0], "x");
        assert!(tasks[0]["duration_ms"].is_u64());
        assert_eq!(tasks[1]["status"], "passed");
        assert_eq!(tasks[2]["status"], "not_run");
        assert!(tasks[2]["started_at"].is_null());
        assert!(tasks[2]["duration_ms"].is_null());
    }

    #[test]
    fn a_task_still_running_at_a_timeout_is_cancelled() {
        let report = RunReport::new();
        let a = task("a");
        report.start(&a, &[]);
        report.mark_timed_out();

        let json = report_json(&report, true);
        assert_eq!(json["status"], "timed_out");
        assert_eq!(json["tasks"][0]["status"], "cancelled");
        assert!(json["tasks"][0]["ended_at"].is_string());
    }

    #[test]
    fn a_task_killed_by_the_timeout_is_cancelled() {
        let report = RunReport::new();
        let a = task("a");
        report.start(&a, &[]);
        report.mark_timed_out();
        report.finish(&a, TaskReportStatus::Failed, None);

        assert_eq!(
            report_json(&report, true)["tasks"][0]["status"],
            "cancelled"
        );
    }

    #[test]
    fn a_task_abandoned_before_it_started_is_not_run() {
        let report = RunReport::new();
        let a = task("a");
        report.start(&a, &[]);
        report.finish(&a, TaskReportStatus::NotRun, None);

        let json = report_json(&report, false);
        assert_eq!(json["tasks"][0]["status"], "not_run");
        assert!(json["tasks"][0]["started_at"].is_null());
    }

    #[test]
    fn skipped_and_cached_tasks_pass_the_run() {
        let report = RunReport::new();
        let (a, b) = (task("a"), task("b"));
        report.start(&a, &[]);
        report.finish(&a, TaskReportStatus::Skipped, None);
        report.start(&b, &[]);
        report.finish(&b, TaskReportStatus::Cached, None);

        let json = report_json(&report, false);
        assert_eq!(json["status"], "passed");
        assert_eq!(json["tasks"][0]["status"], "skipped");
        assert_eq!(json["tasks"][1]["status"], "cached");
    }

    #[test]
    fn a_run_that_fails_before_any_task_is_failed() {
        let json = report_json(&RunReport::new(), true);
        assert_eq!(json["status"], "failed");
        assert_eq!(json["tasks"].as_array().unwrap().len(), 0);
    }
}
