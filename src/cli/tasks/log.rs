use std::io::Write;

use eyre::{Result, bail};

use crate::config::Config;
use crate::file::display_path;
use crate::task::task_run_log;

use super::info::find_task;

/// Print the output of a task's last run
///
/// mise keeps the combined stdout and stderr of each task's most recent run,
/// after redaction, so output that `replacing`, `timed`, `silent` or a
/// truncated terminal hid can be read without running the task again. Each run
/// replaces the task's previous log, and a log keeps at most its last 4 MiB.
///
/// mise only logs output it reads: a `raw` or `interactive` task, or an
/// `interleave` task whose output goes straight to the terminal (when nothing
/// needs redacting and neither the artifact cache nor OpenTelemetry reads it),
/// has no log.
#[derive(Debug, usage_rs::Args)]
#[usage(
    example(
        r###"mise tasks log test"###,
        help = r###"Print the output of the last run of the test task"###
    ),
    example(
        r###"mise tasks log test --path"###,
        help = r###"Print where that log is kept"###
    )
)]
pub(super) struct TasksLog {
    /// Name of the task
    pub task: String,
    /// Print the path of the log file instead of its contents
    ///
    /// The path is printed even when the task has no log yet.
    #[usage(long, verbatim_doc_comment)]
    pub path: bool,
}

impl TasksLog {
    pub(super) async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let Some(task) = find_task(&config, &self.task).await? else {
            bail!(
                "Task not found: {}, use `mise tasks ls --all --hidden` to list all tasks",
                self.task
            );
        };
        let path = task_run_log::log_path(&task, &config);
        if self.path {
            miseprintln!("{}", path.display());
            return Ok(());
        }
        let contents = match std::fs::read(&path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => bail!(
                "no log for task {}: it has not run since mise started keeping logs, or its last run went straight to the terminal (raw, interactive, or interleave output that mise did not read)",
                task.name
            ),
            Err(err) => bail!("failed to read {}: {err}", display_path(&path)),
        };
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&contents)?;
        stdout.flush()?;
        Ok(())
    }
}
