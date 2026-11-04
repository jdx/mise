//! A bounded record of recent task runs, for `mise tasks history`.
//!
//! Each project keeps its runs under `$MISE_STATE_DIR/task-history/<project>/`, one JSON file
//! per task holding that task's last `task.history_limit` runs, oldest first. The project is
//! the monorepo root, else the project root, else the current directory, so a monorepo's
//! subprojects share one history whichever directory `mise run` starts in.
//!
//! Recording is best-effort: a failure is logged at debug level and never fails the run. A
//! writer holds an OS file lock beside the task's file while it reads, appends and trims, so
//! parallel tasks and concurrent `mise run` processes never lose each other's runs, and it
//! replaces the file by an atomic rename, so a reader never needs the lock. The write is not
//! synced: a power loss can lose the latest run, which is not worth a journal commit per task.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use eyre::{Result, WrapErr};
use serde::{Deserialize, Serialize};

use crate::config::{Config, Settings};
use crate::dirs;
use crate::file;
use crate::hash::hash_sha256_to_str;
use crate::lock_file::LockFile;

const FORMAT_VERSION: u8 = 1;

/// How a recorded run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// The task's commands ran and succeeded.
    Ok,
    /// The task failed.
    Failed,
    /// The task's result was restored from the artifact cache instead of running.
    Cached,
    /// The task did not run: its sources were fresh, `skip_if` held, or `task.skip` named it.
    Skipped,
    /// The task was stopped by Ctrl-C or because another task failed.
    Cancelled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Cached => "cached",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
        }
    }
}

/// One recorded run of a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRun {
    pub task: String,
    /// The task's arguments, redacted like terminal output.
    pub args: Vec<String>,
    /// When the run started, RFC 3339 in UTC.
    pub started_at: String,
    pub duration_ms: u64,
    pub status: RunStatus,
    /// The exit code, when the task ran a command that reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl TaskRun {
    pub fn new(
        task: &str,
        args: Vec<String>,
        started: SystemTime,
        duration: Duration,
        status: RunStatus,
        exit_code: Option<i32>,
    ) -> Self {
        let millis = started
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .unwrap_or_default();
        let started_at = jiff::Timestamp::from_millisecond(millis)
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
            .to_string();
        Self {
            task: task.to_string(),
            args,
            started_at,
            duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
            status,
            exit_code,
        }
    }

    fn started(&self) -> jiff::Timestamp {
        self.started_at
            .parse()
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
    }
}

/// What one task's history says about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskSummary {
    pub task: String,
    /// Recorded runs, of any status.
    pub runs: usize,
    pub failures: usize,
    /// Median duration of the runs that executed and succeeded.
    pub median_ms: Option<u64>,
    pub last_ms: u64,
    pub last_status: RunStatus,
    pub last_started_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HistoryFile {
    version: u8,
    /// The project this history belongs to, for whoever reads the state directory.
    root: PathBuf,
    runs: Vec<TaskRun>,
}

/// The run history of one project.
#[derive(Debug, Clone)]
pub struct TaskHistory {
    root: PathBuf,
    dir: PathBuf,
    limit: usize,
}

impl TaskHistory {
    /// The history of the project `config` was loaded for.
    pub fn for_config(config: &Config) -> Self {
        let root = config
            .monorepo_root()
            .or_else(|| config.project_root.clone())
            .unwrap_or_else(|| dirs::CWD.clone().unwrap_or_default());
        let limit = Settings::get().task.history_limit;
        Self::at(&dirs::STATE.join("task-history"), root, limit)
    }

    /// The history to record this run's tasks in, or `None` when recording is off.
    pub fn recorder(config: &Config) -> Option<Self> {
        let settings = Settings::get();
        if !settings.task.history || settings.task.history_limit <= 0 {
            return None;
        }
        Some(Self::for_config(config))
    }

    fn at(base: &Path, root: PathBuf, limit: i64) -> Self {
        let dir = base.join(short_hash(&root.to_string_lossy()));
        Self {
            root,
            dir,
            limit: usize::try_from(limit).unwrap_or(0),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Record a run, logging instead of failing.
    pub fn record(&self, run: TaskRun) {
        if let Err(err) = self.try_record(run) {
            debug!("task history: failed to record run: {err:#}");
        }
    }

    fn try_record(&self, run: TaskRun) -> Result<()> {
        let name = short_hash(&run.task);
        let path = self.dir.join(format!("{name}.json"));
        let _lock = LockFile::at(&self.dir.join(format!("{name}.lock"))).lock()?;
        let mut runs = read_file(&path).unwrap_or_default();
        runs.push(run);
        if runs.len() > self.limit {
            runs.drain(..runs.len() - self.limit);
        }
        let contents = serde_json::to_vec(&HistoryFile {
            version: FORMAT_VERSION,
            root: self.root.clone(),
            runs,
        })?;
        file::write_atomic_unsynced(&path, contents)
    }

    /// Every recorded run, or only `task`'s, oldest first.
    pub fn runs(&self, task: Option<&str>) -> Result<Vec<TaskRun>> {
        let mut runs = vec![];
        if let Some(task) = task {
            let path = self.dir.join(format!("{}.json", short_hash(task)));
            runs.extend(read_file(&path).unwrap_or_default());
        } else if self.dir.is_dir() {
            for entry in std::fs::read_dir(&self.dir)
                .wrap_err_with(|| format!("reading {}", file::display_path(&self.dir)))?
            {
                let path = entry?.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    runs.extend(read_file(&path).unwrap_or_default());
                }
            }
        }
        runs.sort_by_cached_key(|run| (run.started(), run.task.clone()));
        Ok(runs)
    }

    /// One summary per task, sorted by name.
    pub fn summaries(&self, task: Option<&str>) -> Result<Vec<TaskSummary>> {
        Ok(summarize(self.runs(task)?))
    }
}

fn short_hash(s: &str) -> String {
    hash_sha256_to_str(s)[..16].to_string()
}

/// A file that is missing, unreadable, or in another format reads as no runs, so the next
/// write starts it over instead of failing every run.
fn read_file(path: &Path) -> Option<Vec<TaskRun>> {
    let contents = std::fs::read(path).ok()?;
    match serde_json::from_slice::<HistoryFile>(&contents) {
        Ok(file) if file.version == FORMAT_VERSION => Some(file.runs),
        Ok(_) => None,
        Err(err) => {
            debug!("task history: ignoring {}: {err}", file::display_path(path));
            None
        }
    }
}

/// `runs` must be oldest first.
fn summarize(runs: Vec<TaskRun>) -> Vec<TaskSummary> {
    let mut by_task: std::collections::BTreeMap<String, Vec<TaskRun>> = Default::default();
    for run in runs {
        by_task.entry(run.task.clone()).or_default().push(run);
    }
    by_task
        .into_iter()
        .filter_map(|(task, runs)| {
            let last = runs.last()?.clone();
            let mut ok: Vec<u64> = runs
                .iter()
                .filter(|run| run.status == RunStatus::Ok)
                .map(|run| run.duration_ms)
                .collect();
            ok.sort_unstable();
            let median_ms = match ok.len() {
                0 => None,
                n if n % 2 == 1 => Some(ok[n / 2]),
                n => Some((ok[n / 2 - 1] + ok[n / 2]) / 2),
            };
            Some(TaskSummary {
                task,
                runs: runs.len(),
                failures: runs
                    .iter()
                    .filter(|run| run.status == RunStatus::Failed)
                    .count(),
                median_ms,
                last_ms: last.duration_ms,
                last_status: last.status,
                last_started_at: last.started_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(task: &str, secs: u64, ms: u64, status: RunStatus) -> TaskRun {
        TaskRun::new(
            task,
            vec![],
            SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            Duration::from_millis(ms),
            status,
            None,
        )
    }

    #[test]
    fn keeps_the_last_runs_of_each_task() {
        let tmp = tempfile::tempdir().unwrap();
        let history = TaskHistory::at(tmp.path(), PathBuf::from("/project"), 3);
        for i in 0..5 {
            history
                .try_record(run("build", i, i * 10, RunStatus::Ok))
                .unwrap();
        }
        history
            .try_record(run("test", 9, 7, RunStatus::Failed))
            .unwrap();
        let build = history.runs(Some("build")).unwrap();
        assert_eq!(
            build.iter().map(|r| r.duration_ms).collect::<Vec<_>>(),
            vec![20, 30, 40]
        );
        assert_eq!(history.runs(None).unwrap().len(), 4);
        assert_eq!(history.runs(None).unwrap().last().unwrap().task, "test");
    }

    #[test]
    fn concurrent_writers_keep_every_run() {
        let tmp = tempfile::tempdir().unwrap();
        let history = TaskHistory::at(tmp.path(), PathBuf::from("/project"), 100);
        std::thread::scope(|s| {
            for i in 0..8 {
                let history = &history;
                s.spawn(move || {
                    for j in 0..5 {
                        history
                            .try_record(run("build", i * 10 + j, 1, RunStatus::Ok))
                            .unwrap();
                    }
                });
            }
        });
        assert_eq!(history.runs(Some("build")).unwrap().len(), 40);
    }

    #[test]
    fn a_corrupt_file_starts_over() {
        let tmp = tempfile::tempdir().unwrap();
        let history = TaskHistory::at(tmp.path(), PathBuf::from("/project"), 3);
        std::fs::create_dir_all(&history.dir).unwrap();
        let path = history.dir.join(format!("{}.json", short_hash("build")));
        std::fs::write(&path, "not json").unwrap();
        history
            .try_record(run("build", 1, 5, RunStatus::Ok))
            .unwrap();
        assert_eq!(history.runs(Some("build")).unwrap().len(), 1);
    }

    #[test]
    fn median_counts_only_successful_executions() {
        let summaries = summarize(vec![
            run("build", 1, 100, RunStatus::Ok),
            run("build", 2, 300, RunStatus::Ok),
            run("build", 3, 1, RunStatus::Cached),
            run("build", 4, 2, RunStatus::Failed),
            run("lint", 5, 50, RunStatus::Skipped),
        ]);
        assert_eq!(summaries.len(), 2);
        let build = &summaries[0];
        assert_eq!(build.runs, 4);
        assert_eq!(build.failures, 1);
        assert_eq!(build.median_ms, Some(200));
        assert_eq!(build.last_status, RunStatus::Failed);
        assert_eq!(build.last_ms, 2);
        assert_eq!(summaries[1].median_ms, None);
    }

    #[test]
    fn started_at_is_rfc3339_utc() {
        let r = run("build", 90, 0, RunStatus::Ok);
        assert_eq!(r.started_at, "1970-01-01T00:01:30Z");
    }
}
