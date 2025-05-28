use std::sync::Arc;

use crate::config::Config;
use crate::task::{Deps, Task};
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

    /// Show hidden tasks
    #[clap(long, verbatim_doc_comment)]
    pub hidden: bool,

    /// Display dependencies in DOT format
    #[clap(long, alias = "dot", verbatim_doc_comment)]
    pub dot: bool,
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
        Ok(config
            .tasks()
            .await?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .cloned()
            .collect())
    }

    async fn get_task_lists(&self, config: &Arc<Config>) -> Result<Vec<Task>> {
        let all_tasks = config.tasks().await?;
        let mut tasks = vec![];
        for task in self.tasks.as_ref().unwrap_or(&vec![]) {
            match all_tasks
                .get(task)
                .or_else(|| all_tasks.values().find(|t| &t.display_name == task))
                .cloned()
            {
                Some(task) => {
                    tasks.push(task);
                }
                None => {
                    return Err(self.err_no_task(config, task).await);
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
        let deps = Deps::new(config, tasks.clone()).await?;
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
        let deps = Deps::new(config, tasks).await?;
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

    async fn err_no_task(&self, config: &Arc<Config>, t: &str) -> eyre::Report {
        let tasks = config
            .tasks()
            .await
            .map(|t| t.values().map(|v| &v.display_name).collect::<Vec<_>>())
            .unwrap_or_default();
        let task_names = tasks.into_iter().map(style::ecyan).join(", ");
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
