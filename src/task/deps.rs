use crate::config::Config;
use crate::task::Task;
use itertools::Itertools;
use petgraph::graph::DiGraph;
use petgraph::Direction;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

#[derive(Debug)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<String>,
    tx: mpsc::Sender<Option<Task>>,
}

impl Deps {
    pub fn new(config: &Config, tasks: Vec<Task>) -> eyre::Result<Self> {
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
