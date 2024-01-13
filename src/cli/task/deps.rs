use crate::{
    config::{Config, Settings},
    task::{Deps, Task},
    ui::{style, tree::print_tree},
};
use console::style;
use eyre::Result;
use itertools::Itertools;
use petgraph::dot::Dot;

/// [experimental] Display a tree visualization of a dependency graph
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDeps {
    /// Tasks to show dependencies for
    /// Can specify multiple tasks by separating with spaces
    /// e.g.: mise task deps lint test check
    #[clap(verbatim_doc_comment)]
    pub tasks: Option<Vec<String>>,

    /// Display dependencies in DOT format
    #[clap(long, alias = "dot", verbatim_doc_comment)]
    pub dot: bool,
}

impl TaskDeps {
    pub fn run(self) -> Result<()> {
        let config = Config::try_get()?;
        let settings = Settings::try_get()?;
        settings.ensure_experimental()?;

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
        config
            .tasks()
            .iter()
            .map(|(_, t)| t)
            .sorted()
            .filter(|t| !t.hide)
            .map(|t| Ok(t.to_owned()))
            .collect()
    }

    fn get_task_lists(&self, config: &Config) -> Result<Vec<Task>> {
        let tasks = self.tasks.as_ref().map(|t| {
            t.iter()
                .sorted()
                .map(|tn| match config.tasks().get(tn) {
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
            print_tree(&(&deps.graph, idx));
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
        let tasks = config.tasks();
        let task_names = tasks.keys().sorted().map(style::ecyan).join(", ");
        let t = style(&t).yellow().for_stderr();
        eyre!("no task named `{t}` found. Available tasks: {task_names}")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise task deps</bold>
  Shows dependencies for all tasks

  $ <bold>mise task deps lint test check</bold>
  Shows dependencies for the "lint", "test" and "check" tasks

  $ <bold>mise task deps --dot</bold>
  Shows dependencies in DOT format
"#
);

#[cfg(test)]
mod tests {
    #[test]
    fn test_tasks_deps_tree() {
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
