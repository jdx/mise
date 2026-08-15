use crate::config::Config;
use crate::task::TaskLoadContext;
use crate::task::task_fetcher::TaskFetcher;
use eyre::{Result, eyre};

/// Resolve a file task's generated usage specification.
///
/// This is an internal target for usage mounts. Keeping the raw generator
/// command behind mise preserves the task's config root, environment, trust,
/// and sandbox behavior without eagerly running every generator in `tasks ls`.
#[derive(Debug, clap::Args)]
pub struct TasksUsage {
    /// Canonical task name to resolve
    task: String,
}

impl TasksUsage {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let context = TaskLoadContext::all();
        let tasks = config.tasks_with_context(Some(&context)).await?;
        let task = tasks
            .get(&self.task)
            .cloned()
            .ok_or_else(|| eyre!("task not found: {}", self.task))?;
        let requires_canonical_name = tasks
            .values()
            .filter(|candidate| candidate.display_name == task.display_name)
            .count()
            > 1;
        let mut tasks = vec![task];
        TaskFetcher::new(false)
            .fetch_tasks(&config, &mut tasks)
            .await?;
        let task = tasks
            .pop()
            .ok_or_else(|| eyre!("task disappeared while resolving usage: {}", self.task))?;
        if task.usage_command.is_none() {
            return Err(eyre!("task {} has no usage_command", task.display_name));
        }
        let mut spec = task.parse_usage_spec_for_display(&config).await?;
        if requires_canonical_name {
            spec.name = task.name.clone();
            spec.bin = task.name.clone();
            spec.cmd.name = task.name.clone();
            spec.cmd.usage = spec.cmd.usage();
        }
        miseprintln!("{spec}");
        Ok(())
    }
}
