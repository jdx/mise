use std::{
    collections::{HashMap, HashSet},
    sync::mpsc,
};

use crate::{
    config::{Config, Settings},
    task::Task,
    ui::style,
};
use console::style;
use itertools::Itertools;
use miette::Result;
use petgraph::{dot::Dot, graph::DiGraph, Direction};
use ptree::graph::print_graph;

/// [experimental] Display a tree visualization of a dependency graph
#[derive(Debug, clap::Args)]
#[clap(verbatim_doc_comment, after_long_help = AFTER_LONG_HELP)]
pub struct TaskDeps {
    /// Tasks to get dependencies for
    /// Can specify multiple tasks by separating with spaces
    /// e.g.: mise task deps task1 task2
    #[clap(verbatim_doc_comment)]
    pub tasks: Option<Vec<String>>,

    /// Print dependencies in DOT format
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

        // TODO remove this once printing works properlybr
        // let task_names = tasks.iter().map(|t| t.name.clone()).join(", ");
        // miseprint!("Dependencies for task(s): {}\n", task_names);

        if self.dot {
            let _ = self.print_deps_dot(&config, tasks);
        } else {
            let _ = self.print_deps_tree(&config, tasks);
        }

        Ok(())
    }

    fn get_all_tasks(&self, config: &Config) -> Result<Vec<Task>> {
        config
            .tasks()
            .iter()
            .filter(|(n, t)| *n == &t.name) // filter out aliases
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
            let _ = print_graph(&deps.graph, idx);
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

    fn err_no_task(&self, config: &Config, t: &str) -> miette::Report {
        let tasks = config.tasks();
        let task_names = tasks.keys().sorted().map(style::ecyan).join(", ");
        let t = style(&t).yellow().for_stderr();
        miette!("no task named `{t}` found. Available tasks: {task_names}")
    }
}

static AFTER_LONG_HELP: &str = color_print::cstr!(
    r#"<bold><underline>Examples:</underline></bold>
  $ <bold>mise task deps</bold>
  Shows dependencies for all tasks

  $ <bold>mise task deps task1</bold>
  Shows dependencies for task1

  $ <bold>mise task deps --dot</bold>
  Shows dependencies in DOT format
"#
);

#[derive(Debug)]
pub struct Deps {
    graph: DiGraph<Task, ()>,
    sent: HashSet<String>,
    tx: mpsc::Sender<Option<Task>>,
}

impl Deps {
    pub fn new(config: &Config, tasks: Vec<Task>) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];
        for t in tasks {
            stack.push(t.clone());
            indexes
                .entry(t.name.clone())
                .or_insert_with(|| graph.add_node(t));
        }
        while let Some(a) = stack.pop() {
            let a_idx = *indexes
                .entry(a.name.clone())
                .or_insert_with(|| graph.add_node(a.clone()));
            for b in a.resolve_depends(config)? {
                let b_idx = *indexes
                    .entry(b.name.clone())
                    .or_insert_with(|| graph.add_node(b.clone()));
                if !graph.contains_edge(a_idx, b_idx) {
                    graph.add_edge(a_idx, b_idx, ());
                }
                stack.push(b.clone());
            }
        }
        let (tx, _) = mpsc::channel();
        let sent = HashSet::new();
        Ok(Self { graph, tx, sent })
    }

    fn leaves(&self) -> Vec<Task> {
        self.graph
            .externals(Direction::Outgoing)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    fn emit_leaves(&mut self) {
        let leaves = self.leaves().into_iter().collect_vec();
        for task in leaves {
            if self.sent.contains(&task.name) {
                continue;
            }
            self.sent.insert(task.name.clone());
            self.tx.send(Some(task)).unwrap();
        }
        if self.graph.node_count() == 0 {
            self.tx.send(None).unwrap();
        }
    }

    pub fn subscribe(&mut self) -> mpsc::Receiver<Option<Task>> {
        let (tx, rx) = mpsc::channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    // #[requires(self.graph.node_count() > 0)]
    // #[ensures(self.graph.node_count() == old(self.graph.node_count()) - 1)]
    pub fn remove(&mut self, task: &Task) {
        if let Some(idx) = self
            .graph
            .node_indices()
            .find(|&idx| &self.graph[idx] == task)
        {
            self.graph.remove_node(idx);
            self.emit_leaves();
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &Task> {
        self.graph.node_indices().map(|idx| &self.graph[idx])
    }

    pub fn is_linear(&self) -> bool {
        !self.graph.node_indices().any(|idx| {
            self.graph
                .neighbors_directed(idx, Direction::Outgoing)
                .count()
                > 1
        })
    }

    // fn pop(&'a mut self) -> Option<&'a Task> {
    //     if let Some(leaf) = self.leaves().first() {
    //         self.remove(&leaf.clone())
    //     } else {
    //         None
    //     }
    // }
}
