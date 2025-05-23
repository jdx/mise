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
    pub fn run(self) -> Result<()> {
        let tasks = if self.tasks.is_none() {
            self.get_all_tasks()?
        } else {
            self.get_task_lists()?
        };

        if self.dot {
            self.print_deps_dot(tasks)?;
        } else {
            self.print_deps_tree(tasks)?;
        }

        Ok(())
    }

    fn get_all_tasks(&self) -> Result<Vec<Task>> {
        Ok(Config::get()
            .tasks()?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .cloned()
            .collect())
    }

    fn get_task_lists(&self) -> Result<Vec<Task>> {
        let config = Config::get();
        let tasks = config.tasks()?;
        let tasks = self.tasks.as_ref().map(|t| {
            t.iter()
                .map(|tn| {
                    tasks
                        .get(tn)
                        .cloned()
                        .or_else(|| {
                            tasks
                                .values()
                                .find(|task| task.display_name().as_str() == tn.as_str())
                                .cloned()
                        })
                        .ok_or_else(|| self.err_no_task(tn.as_str()))
                })
                .collect::<Result<Vec<Task>>>()
        });
        match tasks {
            Some(Ok(tasks)) => Ok(tasks),
            Some(Err(e)) => Err(e),
            None => Ok(vec![]),
        }
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
    fn print_deps_tree(&self, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new(tasks.clone())?;
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
    fn print_deps_dot(&self, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new(tasks)?;
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

    fn err_no_task(&self, t: &str) -> eyre::Report {
        let config = Config::get();
        let tasks = config
            .tasks()
            .map(|t| t.values().map(|v| v.display_name()).collect::<Vec<_>>())
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
