use crate::config::CONFIG;
use crate::task::Task;
use crossbeam_channel as channel;
use itertools::Itertools;
use petgraph::graph::DiGraph;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<String>, // tasks that have already started so should not run again
    tx: channel::Sender<Option<Task>>,
}

/// manages a dependency graph of tasks so `mise run` knows what to run next
impl Deps {
    pub fn new(tasks: Vec<Task>) -> eyre::Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];

        // first we add all tasks to the graph, create a stack of work for this function, and
        // store the index of each task in the graph
        for t in &tasks {
            stack.push(t.clone());
            indexes
                .entry(t.name.clone())
                .or_insert_with(|| graph.add_node(t.clone()));
        }
        while let Some(a) = stack.pop() {
            let a_idx = *indexes
                .entry(a.name.clone())
                .or_insert_with(|| graph.add_node(a.clone()));
            for b in a.resolve_depends(&CONFIG, &tasks)? {
                let b_idx = *indexes
                    .entry(b.name.clone())
                    .or_insert_with(|| graph.add_node(b.clone()));
                if !graph.contains_edge(a_idx, b_idx) {
                    graph.add_edge(a_idx, b_idx, ());
                }
                stack.push(b.clone());
            }
        }
        let (tx, _) = channel::unbounded();
        let sent = HashSet::new();
        Ok(Self { graph, tx, sent })
    }

    fn leaves(&self) -> Vec<Task> {
        self.graph
            .externals(Direction::Outgoing)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// main method to emit tasks that no longer have dependencies being waited on
    fn emit_leaves(&mut self) {
        let leaves = self.leaves().into_iter().collect_vec();
        for task in leaves {
            if self.sent.contains(&task.name) {
                continue;
            }
            self.sent.insert(task.name.clone());
            if let Err(e) = self.tx.send(Some(task)) {
                trace!("Error sending task: {e:?}");
            }
        }
        if self.graph.node_count() == 0 {
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing task stream: {e:?}");
            }
        }
    }

    /// listened to by `mise run` which gets a stream of tasks to run
    pub fn subscribe(&mut self) -> channel::Receiver<Option<Task>> {
        let (tx, rx) = channel::unbounded();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    // use contracts::{ensures, requires};
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

    // fn pop(&'a mut self) -> Option<&'a Tasks> {
    //     if let Some(leaf) = self.leaves().first() {
    //         self.remove(&leaf.clone())
    //     } else {
    //         None
    //     }
    // }
}
