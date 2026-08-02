use std::sync::Arc;
use std::time::Duration;

use bytesize::ByteSize;
use chrono::{DateTime, SecondsFormat, Utc};
use comfy_table::Row;
use eyre::{Result, bail};
use itertools::Itertools;
use serde::Serialize;

use crate::config::Config;
use crate::task::task_cache::{TaskCacheEntry, task_cache_entries};
use crate::task::task_fetcher::TaskFetcher;
use crate::task::task_source_checker::task_cwd;
use crate::task::{GetMatchingExt, Task};
use crate::ui::table::MiseTable;

/// Inspect output cache entries for a task
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment)]
pub struct CacheTask {
    /// Task name or pattern to inspect
    task: String,

    /// Output in JSON format
    #[clap(short = 'J', long)]
    json: bool,
}

impl CacheTask {
    pub async fn run(self) -> Result<()> {
        let (config, tasks) = resolve_tasks(&self.task).await?;
        let mut task_entries = Vec::with_capacity(tasks.len());
        for task in tasks {
            let root = task_cwd(&task, &config).await?;
            let entries = task_cache_entries(&task, &root)?;
            task_entries.push((task, root, entries));
        }
        if self.json {
            #[derive(Serialize)]
            struct Output<'a> {
                task: &'a str,
                root: &'a std::path::Path,
                entries: &'a [TaskCacheEntry],
            }
            let output = task_entries
                .iter()
                .map(|(task, root, entries)| Output {
                    task: &task.display_name,
                    root,
                    entries,
                })
                .collect_vec();
            miseprintln!("{}", serde_json::to_string_pretty(&output)?);
            return Ok(());
        }
        if task_entries
            .iter()
            .all(|(_, _, entries)| entries.is_empty())
        {
            miseprintln!(
                "No output cache entries for {}",
                task_entries
                    .iter()
                    .map(|(task, _, _)| &task.display_name)
                    .join(", ")
            );
            return Ok(());
        }
        let multiple_tasks = task_entries.len() > 1;
        let mut headings = vec![
            "Key",
            "Current",
            "Size",
            "Restored",
            "Time Saved",
            "Last Accessed",
            "Outputs",
        ];
        if multiple_tasks {
            headings.insert(0, "Task");
        }
        let mut table = MiseTable::new(false, &headings);
        for (task, _, entries) in task_entries {
            for entry in entries {
                let mut cells = vec![
                    entry.key,
                    if entry.current { "yes" } else { "" }.to_string(),
                    ByteSize::b(entry.size_bytes).display().iec().to_string(),
                    ByteSize::b(entry.restored_bytes)
                        .display()
                        .iec()
                        .to_string(),
                    crate::ui::time::format_duration(Duration::from_nanos(
                        entry.execution_duration_ns,
                    )),
                    format_timestamp(entry.last_accessed),
                    entry.outputs.iter().map(|path| path.display()).join(", "),
                ];
                if multiple_tasks {
                    cells.insert(0, task.display_name.clone());
                }
                table.add_row(Row::from(cells));
            }
        }
        table.print()
    }
}

pub(super) async fn resolve_tasks(task_spec: &str) -> Result<(Arc<Config>, Vec<Task>)> {
    let config = Config::get().await?;
    let task_name = crate::task::expand_colon_task_syntax(task_spec, &config)?;
    let tasks = if task_name.starts_with("//") {
        let ctx = crate::task::TaskLoadContext::from_pattern(&task_name);
        config.tasks_with_context(Some(&ctx)).await?
    } else if crate::task::is_workspace_project_task(&task_name) {
        let ctx = crate::task::TaskLoadContext::all();
        config.tasks_with_context(Some(&ctx)).await?
    } else {
        config.tasks().await?
    };
    let tasks_with_aliases = crate::task::build_task_ref_map(tasks.iter());
    let mut tasks = tasks_with_aliases
        .get_matching(&task_name)?
        .into_iter()
        .map(|task| (**task).clone())
        .collect_vec();
    if tasks.is_empty() {
        bail!("Task not found: {task_spec}, use `mise tasks ls --all --hidden` to list all tasks");
    }
    TaskFetcher::new(false)
        .fetch_tasks(&config, &mut tasks)
        .await?;
    Ok((config, tasks))
}

fn format_timestamp(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        .map(|timestamp| timestamp.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| "unknown".to_string())
}
