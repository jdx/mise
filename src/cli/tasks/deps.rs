use console::style;
use eyre::{eyre, Result};
use itertools::Itertools;
use petgraph::dot::Dot;

use crate::config::{Config, Settings};
use crate::task::{Deps, Task};
use crate::ui::style::{self};
use crate::ui::tree::print_tree;

/// [experimental] Display a tree visualization of a dependency graph
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
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental("`mise tasks deps`")?;

        let tasks = if self.tasks.is_none() {
            self.get_all_tasks(&config)?
        } else {
            self.get_task_lists(&config)?
        };

        if self.dot {
            self.print_deps_dot(&config, tasks)?;
        } else {
            self.print_deps_tree(&config, tasks)?;
        }

        Ok(())
    }

    fn get_all_tasks(&self, config: &Config) -> Result<Vec<Task>> {
        Ok(config
            .tasks()?
            .values()
            .filter(|t| self.hidden || !t.hide)
            .cloned()
            .collect())
    }

    fn get_task_lists(&self, config: &Config) -> Result<Vec<Task>> {
        let tasks = config.tasks()?;
        let tasks = self.tasks.as_ref().map(|t| {
            t.iter()
                .map(|tn| match tasks.get(tn).cloned() {
                    Some(task) => Ok(task.clone()),
                    None => Err(self.err_no_task(config, tn.as_str())),
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
    fn print_deps_tree(&self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new(config, tasks.clone())?;
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
    fn print_deps_dot(&self, config: &Config, tasks: Vec<Task>) -> Result<()> {
        let deps = Deps::new(config, tasks)?;
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

    fn err_no_task(&self, config: &Config, t: &str) -> eyre::Report {
        let tasks = config
            .tasks()
            .map(|t| t.keys().collect::<Vec<_>>())
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

#[cfg(test)]
mod tests {
    use crate::test::reset;

    #[test]
    fn test_tasks_deps_tree() {
        reset();
        assert_cli_snapshot!("tasks", "deps", @r###"
        configtask
        filetask
        ├── test
        └── lint
        lint
        test
        "###
        );
    }

    #[test]
    fn test_tasks_deps_tree_args() {
        reset();
        assert_cli_snapshot!("tasks", "deps", "filetask", "lint", "test", @r###"
        filetask
        ├── test
        └── lint
        lint
        test
        "###
        );
    }

    #[test]
    fn test_tasks_deps_dot() {
        reset();
        assert_cli_snapshot!("tasks", "deps", "--dot", @r###"
        digraph {
            0 [ label = "configtask"]
            1 [ label = "filetask"]
            2 [ label = "lint"]
            3 [ label = "test"]
            1 -> 2 [ ]
            1 -> 3 [ ]
        }
        "###
        );
    }
}
