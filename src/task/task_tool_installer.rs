use crate::cli::args::ToolArg;
use crate::config::{Config, Settings};
use crate::task::Deps;
use crate::task::task_context_builder::TaskContextBuilder;
use crate::task::task_helpers::canonicalize_path;
use crate::toolset::{InstallOptions, ToolSource, Toolset};
use eyre::Result;
use std::path::Path;
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
                    .collect_tools_from_config_file(task_cf.clone(), &t.name)
                    .await?;
                all_tool_requests.extend(tool_requests);
            } else if let Some(config_root) = &t.config_root {
                // For file tasks without a config file (e.g. scripts in .mise-tasks/),
                // fall back to loading tools from the project's config hierarchy
                let tool_requests = self.collect_tools_from_dir(config_root, &t.name).await?;
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

    /// Collect tools from a task's config file hierarchy
    async fn collect_tools_from_config_file(
        &self,
        task_cf: Arc<dyn crate::config::config_file::ConfigFile>,
        task_name: &str,
    ) -> Result<Vec<crate::toolset::ToolRequest>> {
        let task_dir = task_cf.config_root();
        self.collect_tools_from_dir(&task_dir, task_name).await
    }

    /// Collect tools from config files found in a directory hierarchy
    async fn collect_tools_from_dir(
        &self,
        dir: &Path,
        task_name: &str,
    ) -> Result<Vec<crate::toolset::ToolRequest>> {
        let config_paths = crate::config::load_config_hierarchy_from_dir(dir)?;
        let task_config_files = crate::config::load_config_files_from_paths(&config_paths).await?;

        let mut tool_requests: Vec<crate::toolset::ToolRequest> = vec![];
        let mut seen_tools: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (source, cf) in task_config_files.iter() {
            let config_path = canonicalize_path(source);

            // Check cache first for this config file's tool request set
            let trs = {
                let cache = self
                    .context_builder
                    .tool_request_set_cache()
                    .read()
                    .expect("tool_request_set_cache RwLock poisoned");
                cache.get(&config_path).cloned()
            };

            let trs = if let Some(cached) = trs {
                trace!(
                    "Using cached tool request set from {}",
                    config_path.display()
                );
                cached
            } else {
                match cf.to_tool_request_set() {
                    Ok(trs) => {
                        let trs = Arc::new(trs);
                        let mut cache = self
                            .context_builder
                            .tool_request_set_cache()
                            .write()
                            .expect("tool_request_set_cache RwLock poisoned");
                        cache.insert(config_path.clone(), Arc::clone(&trs));
                        trace!("Cached tool request set from {}", config_path.display());
                        trs
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse tools from {} for task {}: {}",
                            source.display(),
                            task_name,
                            e
                        );
                        continue;
                    }
                }
            };

            for (ba, reqs) in trs.tools.iter() {
                let tool_key = ba.to_string();
                if !seen_tools.contains(&tool_key) {
                    trace!(
                        "Adding tool {} from {} for task {}",
                        ba,
                        source.display(),
                        task_name
                    );
                    tool_requests.extend(reqs.iter().cloned());
                    seen_tools.insert(tool_key);
                }
            }
        }

        trace!(
            "Found {} tool requests in config hierarchy for task {}",
            tool_requests.len(),
            task_name
        );

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
        let _ = ts
            .install_missing_versions(
                config,
                &InstallOptions {
                    missing_args_only: !Settings::get().task.run_auto_install,
                    skip_auto_install: !Settings::get().task.run_auto_install
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
