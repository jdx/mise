use comfy_table::Cell;
use eyre::Result;
use itertools::Itertools;

use crate::config::Config;
use crate::task::task_history::{TaskHistory, TaskRun, TaskSummary};
use crate::ui::table::MiseTable;

/// Show recent task runs and how long they took
///
/// `mise run` records each task's start time, duration, arguments and result
/// for this project: whether it succeeded, failed, was restored from the
/// artifact cache, skipped, or cancelled. mise keeps the last
/// `task.history_limit` runs of each task (default 20); set `task.history` to
/// `false` to stop recording.
///
/// `--summary` shows one line per task with its median duration over the runs
/// that executed and succeeded, which is the typical cost of running it.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise tasks history"###,
        help = r###"List this project's recent task runs"###
    ),
    example(
        r###"mise tasks history test"###,
        help = r###"List the recent runs of the test task"###
    ),
    example(
        r###"mise tasks history --summary"###,
        help = r###"Show each task's median and last duration"###
    ),
    example(
        r###"mise tasks history --summary --json"###,
        help = r###"Get typical durations for scripts and agents"###
    )
)]
pub(super) struct TasksHistory {
    /// Only show runs of this task
    task: Option<String>,
    /// Show one line per task: runs, failures, median and last duration
    #[usage(short, long)]
    summary: bool,
    /// Output in JSON format
    #[usage(short = 'J', long)]
    json: bool,
}

impl TasksHistory {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        // Runs are recorded under the task's full name, so an alias or the `:task` shorthand
        // finds them; a task that no longer exists still shows its old runs.
        let task = match &self.task {
            Some(name) => Some(
                super::info::find_task(&config, name)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| t.name)
                    .unwrap_or_else(|| name.clone()),
            ),
            None => None,
        };
        let history = TaskHistory::for_config(&config);
        if self.summary {
            let summaries = history.summaries(task.as_deref())?;
            if self.json {
                miseprintln!("{}", serde_json::to_string_pretty(&summaries)?);
            } else {
                print_summaries(&summaries)?;
            }
        } else {
            let runs = history.runs(task.as_deref())?;
            if self.json {
                miseprintln!("{}", serde_json::to_string_pretty(&runs)?);
            } else {
                print_runs(&runs)?;
            }
        }
        Ok(())
    }
}

fn print_runs(runs: &[TaskRun]) -> Result<()> {
    let mut table = MiseTable::new(false, &["Task", "Started", "Duration", "Status", "Args"]);
    for run in runs {
        table.add_row(vec![
            Cell::new(&run.task),
            Cell::new(local_time(&run.started_at)),
            Cell::new(format_ms(run.duration_ms)),
            Cell::new(status(run.status.as_str(), run.exit_code)),
            Cell::new(run.args.iter().join(" ")),
        ]);
    }
    table.print()
}

fn print_summaries(summaries: &[TaskSummary]) -> Result<()> {
    let mut table = MiseTable::new(
        false,
        &["Task", "Runs", "Failed", "Median", "Last", "Last status"],
    );
    for s in summaries {
        table.add_row(vec![
            Cell::new(&s.task),
            Cell::new(s.runs),
            Cell::new(s.failures),
            Cell::new(s.median_ms.map(format_ms).unwrap_or_else(|| "-".into())),
            Cell::new(format_ms(s.last_ms)),
            Cell::new(s.last_status.as_str()),
        ]);
    }
    table.print()
}

fn status(status: &str, exit_code: Option<i32>) -> String {
    match exit_code {
        Some(code) if code != 0 => format!("{status} ({code})"),
        _ => status.to_string(),
    }
}

fn format_ms(ms: u64) -> String {
    crate::ui::time::format_duration(std::time::Duration::from_millis(ms))
}

fn local_time(started_at: &str) -> String {
    started_at
        .parse::<jiff::Timestamp>()
        .map(|ts| {
            ts.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| started_at.to_string())
}
