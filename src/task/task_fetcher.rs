use crate::config::Settings;
use crate::task::Task;
use crate::task::task_file_providers::TaskFileProvidersBuilder;
use eyre::{Result, bail};

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
        let no_cache = self.no_cache || Settings::get().task.remote_no_cache.unwrap_or(false);
        let task_file_providers = TaskFileProvidersBuilder::new()
            .with_cache(!no_cache)
            .build();

        for t in tasks {
            if let Some(file) = &t.file {
                let source = file.to_string_lossy().to_string();

                // Skip local files - they don't need provider resolution
                if !Self::is_remote_source(&source) {
                    continue;
                }

                let provider = task_file_providers.get_provider(&source);

                if provider.is_none() {
                    bail!("No provider found for file: {}", source);
                }

                let local_path = provider.unwrap().get_local_path(&source).await?;

                // Store the original remote source before replacing with local path
                // This is used to determine if the task should use monorepo config file context
                t.remote_file_source = Some(source);
                t.file = Some(local_path);
            }
        }

        Ok(())
    }

    /// Check if a source path is a remote task file (git or http/https)
    fn is_remote_source(source: &str) -> bool {
        source.starts_with("git::")
            || source.starts_with("http://")
            || source.starts_with("https://")
    }
}
