use crate::cli::args::ToolArg;
use crate::config::{Config, Settings};
use crate::task::Deps;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_helpers::canonicalize_path;
use crate::toolset::{InstallOptions, ToolSource, Toolset};
use eyre::Result;
use std::sync::Arc;

/// Handles collection and installation of tools required by tasks
pub struct TaskToolInstaller<'a> {
    context_builder: &'a TaskContextBuilder,
    cli_tools: &'a [ToolArg],
}

impl<'a> TaskToolInstaller<'a> {
    pub fn new(context_builder: &'a TaskContextBuilder, cli_tools: &'a [ToolArg]) -> Self {
        Self {
            context_builder,
            cli_tools,
        }
    }

    /// Collect and install all tools needed by tasks
    pub async fn install_tools(&self, config: &mut Arc<Config>, tasks: &Deps) -> Result<()> {
        let mut all_tools = self.cli_tools.to_vec();
        let mut all_tool_requests = vec![];
        let all_tasks: Vec<_> = tasks.all().collect();

        trace!("Collecting tools from {} tasks", all_tasks.len());

        // Collect tools from tasks
        for t in &all_tasks {
            // Collect tools from task.tools (task-level tool overrides)
            for (k, v) in &t.tools {
                all_tools.push(format!("{k}@{v}").parse()?);
            }

            // Collect tools from monorepo task config files
            if let Some(task_cf) = t.cf(config) {
                let tool_requests = self
                    .collect_tools_from_config_file(config, task_cf.clone(), &t.name)
                    .await?;
                all_tool_requests.extend(tool_requests);
            }
        }

        // Build and install toolset
        let toolset = self
            .build_toolset(config, all_tools, all_tool_requests)
            .await?;
        self.install_toolset(config, toolset).await?;

        Ok(())
    }

    /// Collect tools from a task's config file with caching
    async fn collect_tools_from_config_file(
        &self,
        _config: &Arc<Config>,
        task_cf: Arc<dyn crate::config::config_file::ConfigFile>,
        task_name: &str,
    ) -> Result<Vec<crate::toolset::ToolRequest>> {
        let config_path = canonicalize_path(task_cf.get_path());

        // Check cache first
        let cache = self
            .context_builder
            .tool_request_set_cache()
            .read()
            .expect("tool_request_set_cache RwLock poisoned");

        let tool_request_set = if let Some(cached) = cache.get(&config_path) {
            trace!(
                "Using cached tool request set from {}",
                config_path.display()
            );
            Arc::clone(cached)
        } else {
            drop(cache); // Release read lock before write
            match task_cf.to_tool_request_set() {
                Ok(trs) => {
                    let trs = Arc::new(trs);
                    let mut cache = self
                        .context_builder
                        .tool_request_set_cache()
                        .write()
                        .expect("tool_request_set_cache RwLock poisoned");
                    cache.entry(config_path.clone()).or_insert_with(|| {
                        trace!("Cached tool request set to {}", config_path.display());
                        Arc::clone(&trs)
                    });
                    trs
                }
                Err(e) => {
                    warn!(
                        "Failed to parse tools from {} for task {}: {}",
                        task_cf.get_path().display(),
                        task_name,
                        e
                    );
                    return Ok(vec![]);
                }
            }
        };

        trace!(
            "Found {} tools in config file for task {}",
            tool_request_set.tools.len(),
            task_name
        );

        // Extract all tool requests from the tool request set
        let mut tool_requests = vec![];
        for (_, reqs) in tool_request_set.tools.iter() {
            tool_requests.extend(reqs.iter().cloned());
        }

        Ok(tool_requests)
    }

    /// Build a toolset from CLI tools and collected tool requests
    async fn build_toolset(
        &self,
        config: &Arc<Config>,
        all_tools: Vec<ToolArg>,
        all_tool_requests: Vec<crate::toolset::ToolRequest>,
    ) -> Result<Toolset> {
        let source = ToolSource::Argument;
        let mut ts = Toolset::new(source.clone());

        // Add tools from CLI args and task.tools
        for tool_arg in all_tools {
            if let Some(tvr) = tool_arg.tvr {
                ts.add_version(tvr);
            }
        }

        // Add tools from config files
        for tr in all_tool_requests {
            trace!("Adding tool from config: {}", tr);
            ts.add_version(tr);
        }

        ts.resolve(config).await?;

        Ok(ts)
    }

    /// Install missing versions from the toolset
    async fn install_toolset(&self, config: &mut Arc<Config>, mut ts: Toolset) -> Result<()> {
        ts.install_missing_versions(
            config,
            &InstallOptions {
                missing_args_only: !Settings::get().task_run_auto_install,
                skip_auto_install: !Settings::get().task_run_auto_install
                    || !Settings::get().auto_install,
                ..Default::default()
            },
        )
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_tool_installer_new() {
        let context_builder = TaskContextBuilder::new();
        let cli_tools: Vec<ToolArg> = vec![];
        let installer = TaskToolInstaller::new(&context_builder, &cli_tools);
        assert_eq!(installer.cli_tools.len(), 0);
    }
}
