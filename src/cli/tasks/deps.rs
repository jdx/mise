use std::sync::Arc;

use crate::config::Config;
use crate::task::task_fetcher::TaskFetcher;
use crate::task::{Deps, GetMatchingExt, Task, build_task_ref_map};
use crate::ui::style::{self};
use crate::ui::tree::print_tree;
use console::style;
use eyre::{Result, eyre};
use itertools::Itertools;
use petgraph::dot::Dot;

/// Display a tree visualization of a dependency graph
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TasksDeps {
    /// Tasks to show dependencies for
    /// Can specify multiple tasks by separating with spaces
    /// e.g.: mise tasks deps lint test check
    #[clap(verbatim_doc_comment)]
    pub tasks: Option<Vec<String>>,

    /// Display dependencies in DOT format
    #[clap(long, alias = "dot", verbatim_doc_comment)]
    pub dot: bool,

    /// Show hidden tasks
    #[clap(long, verbatim_doc_comment)]
    pub hidden: bool,
}

impl TasksDeps {
    pub async fn run(self) -> Result<()> {
        let config = Config::get().await?;
        let tasks = if self.tasks.is_none() {
            self.get_all_tasks(&config).await?
        } else {
            self.get_task_lists(&config).await?
        };

        if self.dot {
            self.print_deps_dot(&config, tasks).await?;
        } else {
            self.print_deps_tree(&config, tasks).await?;
        }

        Ok(())
    }

    async fn get_all_tasks(&self, config: &Arc<Config>) -> Result<Vec<Task>> {
        // Use TaskLoadContext::all() to load tasks from entire monorepo
        let ctx = crate::task::TaskLoadContext::all();
        let tasks = config.tasks_with_context(Some(&ctx)).await?;
        let visible_tasks = tasks
            .iter()
            // A local/overlay hide=true survives remote metadata merging, so
            // do not fetch tasks that cannot appear in ordinary output.
            .filter(|(_, task)| self.hidden || !task.hide)
            .map(|(name, task)| (name.clone(), task.clone()))
            .collect();
        let resolved = TaskFetcher::new(false)
            .require_trust_before_fetch()
            .fetch_task_map(config, &visible_tasks)
            .await?;
        Ok(resolved
            .values()
            .filter(|t| self.hidden || !t.hide)
            .cloned()
            .collect())
    }

    async fn get_task_lists(&self, config: &Arc<Config>) -> Result<Vec<Task>> {
        // Expand all task names first
        let task_names: Vec<String> = self
            .tasks
            .as_ref()
            .unwrap_or(&vec![])
            .iter()
            .map(|t| crate::task::expand_colon_task_syntax(t, config))
            .collect::<Result<Vec<_>>>()?;

        // Load monorepo tasks once with combined context for all monorepo patterns
        let monorepo_patterns: Vec<&str> = task_names
            .iter()
            .filter(|t| t.starts_with("//"))
            .map(|s| s.as_str())
            .collect();
        let mut monorepo_tasks = if !monorepo_patterns.is_empty() {
            let ctx = crate::task::TaskLoadContext::from_patterns(monorepo_patterns.into_iter());
            Some(
                config
                    .tasks_with_context(Some(&ctx))
                    .await?
                    .as_ref()
                    .clone(),
            )
        } else {
            None
        };

        // Load non-monorepo tasks once (only if needed)
        let has_regular = task_names.iter().any(|t| !t.starts_with("//"));
        let mut regular_tasks = if has_regular {
            Some(config.tasks().await?.as_ref().clone())
        } else {
            None
        };

        let monorepo_needs_remote = if let Some(tasks) = &monorepo_tasks {
            let refs = build_task_ref_map(tasks.iter());
            task_names
                .iter()
                .filter(|name| name.starts_with("//"))
                .map(|name| refs.get_matching(name))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(Vec::is_empty)
        } else {
            false
        };
        if monorepo_needs_remote && let Some(tasks) = &monorepo_tasks {
            monorepo_tasks = Some(
                TaskFetcher::new(false)
                    .require_trust_before_fetch()
                    .fetch_task_map(config, tasks)
                    .await?,
            );
        }

        let regular_needs_remote = if let Some(tasks) = &regular_tasks {
            let refs = build_task_ref_map(tasks.iter());
            task_names
                .iter()
                .filter(|name| !name.starts_with("//"))
                .map(|name| refs.get_matching(name))
                .collect::<Result<Vec<_>>>()?
                .iter()
                .any(Vec::is_empty)
        } else {
            false
        };
        if regular_needs_remote && let Some(tasks) = &regular_tasks {
            regular_tasks = Some(
                TaskFetcher::new(false)
                    .require_trust_before_fetch()
                    .fetch_task_map(config, tasks)
                    .await?,
            );
        }

        // Build task ref maps once (not per-task), after any remote-alias retry.
        let monorepo_ref_map = monorepo_tasks
            .as_ref()
            .map(|tasks| build_task_ref_map(tasks.iter()));
        let regular_ref_map = regular_tasks
            .as_ref()
            .map(|tasks| build_task_ref_map(tasks.iter()));

        // Look up each task from the appropriate cache
        let mut tasks = vec![];
        for task_name in &task_names {
            let (all_tasks, ref_map) = if task_name.starts_with("//") {
                (
                    monorepo_tasks.as_ref().unwrap(),
                    monorepo_ref_map.as_ref().unwrap(),
                )
            } else {
                (
                    regular_tasks.as_ref().unwrap(),
                    regular_ref_map.as_ref().unwrap(),
                )
            };

            let matching = ref_map.get_matching(task_name).ok();
            let task = matching.and_then(|m| m.first().cloned().cloned());

            match task {
                Some(task) => {
                    tasks.push(task.clone());
                }
                None => {
                    return Err(self.err_no_task(task_name, all_tasks));
                }
            }
        }
        Ok(tasks)
    }

    ///
    /// Print dependencies as a tree
    ///
    /// Example:
    /// ```
    /// task1
    /// ├─ task2
    /// │  └─ task3
    /// └─ task4
    /// task5
    /// ```
    ///
    async fn print_deps_tree(&self, config: &Arc<Config>, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new_for_display(config, tasks.clone()).await?;
        // filter out nodes that are not selected
        let start_indexes = deps.graph.node_indices().filter(|&idx| {
            let task = &deps.graph[idx];
            tasks.iter().any(|t| t.name == task.name)
        });
        // iterate over selected graph nodes and print tree
        for idx in start_indexes {
            print_tree(&(&deps.graph, idx))?;
        }
        Ok(())
    }

    ///
    /// Print dependencies in DOT format
    ///
    /// Example:
    /// ```
    /// digraph {
    ///  1 [label = "task1"]
    ///  2 [label = "task2"]
    ///  3 [label = "task3"]
    ///  4 [label = "task4"]
    ///  5 [label = "task5"]
    ///  1 -> 2 [ ]
    ///  2 -> 3 [ ]
    ///  1 -> 4 [ ]
    /// }
    /// ```
    //
    async fn print_deps_dot(&self, config: &Arc<Config>, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new_for_display(config, tasks).await?;
        miseprintln!(
            "{:?}",
            Dot::with_attr_getters(
                &deps.graph,
                &[
                    petgraph::dot::Config::NodeNoLabel,
                    petgraph::dot::Config::EdgeNoLabel
                ],
                &|_, _| String::new(),
                &|_, nr| format!("label = \"{}\"", nr.1.name),
            ),
        );
        Ok(())
    }

    fn err_no_task(
        &self,
        t: &str,
        all_tasks: &std::collections::BTreeMap<String, Task>,
    ) -> eyre::Report {
        let task_names = all_tasks
            .values()
            .map(|v| v.display_name.clone())
            .map(style::ecyan)
            .join(", ");
        let t = style(&t).yellow().for_stderr();
        eyre!("no tasks named `{t}` found. Available tasks: {task_names}")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>

    # Show dependencies for all tasks
    $ <bold>mise tasks deps</bold>

    # Show dependencies for the "lint", "test" and "check" tasks
    $ <bold>mise tasks deps lint test check</bold>

    # Show dependencies in DOT format
    $ <bold>mise tasks deps --dot</bold>
"#
);
