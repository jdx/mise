use crate::task::task_file_providers::get_local_path;
use crate::task::Task;
use eyre::Result;

/// Handles fetching remote task files and converting them to local paths
pub struct TaskFetcher {
    no_cache: bool,
}

impl TaskFetcher {
    pub fn new(no_cache: bool) -> Self {
        Self { no_cache }
    }

    /// Fetch remote task files, converting remote paths to local cached paths
    pub async fn fetch_tasks(&self, tasks: &mut Vec<Task>) -> Result<()> {
        for t in tasks {
            if let Some(file) = &t.file {
                let source = file.to_string_lossy().to_string();
                let local_path = get_local_path(&source, self.no_cache).await?;

                // Store the original remote source before replacing with local path
                // This is used to determine if the task should use monorepo config file context
                t.remote_file_source = Some(source);
                t.file = Some(local_path);
            }
        }

        Ok(())
    }
}
