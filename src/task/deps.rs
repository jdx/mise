use crate::config::env_directive::EnvDirective;
use crate::task::Task;
use crate::{config::Config, task::task_list::resolve_depends};
use itertools::Itertools;
use petgraph::Direction;
use petgraph::graph::DiGraph;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::mpsc;

/// Unique key for a task instance, including name, args, and env vars
pub type TaskKey = (String, Vec<String>, Vec<(String, String)>);

#[derive(Debug, Clone)]
pub struct Deps {
    pub graph: DiGraph<Task, ()>,
    sent: HashSet<TaskKey>, // tasks that have already started so should not run again
    removed: HashSet<TaskKey>, // tasks that have already finished to track if we are in an infinitve loop
    post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>>, // maps each post-dep to its parent tasks
    tx: mpsc::UnboundedSender<Option<Task>>,
    // not clone, notify waiters via tx None
}

/// Extract a hashable key from a task, including env vars set via dependencies
pub fn task_key(task: &Task) -> TaskKey {
    // Extract simple key-value env vars for deduplication
    // This ensures tasks with same name/args but different env are treated as distinct
    let env_key: Vec<(String, String)> = task
        .env
        .0
        .iter()
        .filter_map(|d| match d {
            EnvDirective::Val(k, v, _) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .sorted()
        .collect();
    (task.name.clone(), task.args.clone(), env_key)
}

/// manages a dependency graph of tasks so `mise run` knows what to run next
impl Deps {
    pub async fn new(config: &Arc<Config>, tasks: Vec<Task>) -> eyre::Result<Self> {
        let mut graph = DiGraph::new();
        let mut indexes = HashMap::new();
        let mut stack = vec![];
        let mut seen = HashSet::new();
        let mut post_dep_parents: HashMap<TaskKey, HashSet<TaskKey>> = HashMap::new();

        let mut add_idx = |task: &Task, graph: &mut DiGraph<Task, ()>| {
            *indexes
                .entry(task_key(task))
                .or_insert_with(|| graph.add_node(task.clone()))
        };

        // first we add all tasks to the graph, create a stack of work for this function, and
        // store the index of each task in the graph
        for t in &tasks {
            stack.push(t.clone());
            add_idx(t, &mut graph);
        }
        let all_tasks_to_run = resolve_depends(config, tasks).await?;
        while let Some(a) = stack.pop() {
            if seen.contains(&a) {
                // prevent infinite loop
                continue;
            }
            let a_idx = add_idx(&a, &mut graph);
            let (pre, post) = a.resolve_depends(config, &all_tasks_to_run).await?;
            for b in pre {
                let b_idx = add_idx(&b, &mut graph);
                graph.update_edge(a_idx, b_idx, ());
                stack.push(b.clone());
            }
            for b in post {
                let b_idx = add_idx(&b, &mut graph);
                graph.update_edge(b_idx, a_idx, ());
                post_dep_parents
                    .entry(task_key(&b))
                    .or_default()
                    .insert(task_key(&a));
                stack.push(b.clone());
            }
            seen.insert(a);
        }
        let (tx, _) = mpsc::unbounded_channel();
        let sent = HashSet::new();
        let removed = HashSet::new();
        Ok(Self {
            graph,
            tx,
            sent,
            removed,
            post_dep_parents,
        })
    }

    /// main method to emit tasks that no longer have dependencies being waited on
    fn emit_leaves(&mut self) {
        let leaves = leaves(&self.graph);
        let leaves_is_empty = leaves.is_empty();

        for task in leaves {
            let key = task_key(&task);

            if self.sent.insert(key) {
                trace!("Scheduling task {0}", task.name);
                if let Err(e) = self.tx.send(Some(task)) {
                    trace!("Error sending task: {e:?}");
                }
            }
        }

        if self.is_empty() {
            trace!("All tasks finished");
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing task stream: {e:?}");
            }
        } else if leaves_is_empty && self.sent.len() == self.removed.len() {
            panic!(
                "Infinitive loop detected, all tasks are finished but the graph isn't empty {0} {1:#?}",
                self.all().map(|t| t.name.clone()).join(", "),
                self.graph
            )
        }
    }

    /// listened to by `mise run` which gets a stream of tasks to run
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<Task>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Check if a post-dep task should actually run: it must be a post-dependency
    /// AND its parent must have been scheduled.
    /// Returns false for non-post-dep tasks or post-deps whose parent was never started.
    pub fn is_runnable_post_dep(&self, task: &Task) -> bool {
        let key = task_key(task);
        match self.post_dep_parents.get(&key) {
            Some(parent_keys) => parent_keys.iter().any(|pk| self.sent.contains(pk)),
            None => false,
        }
    }

    /// Remove multiple tasks from the graph in a batch, emitting leaves only once at the end.
    /// This prevents intermediate emit_leaves from scheduling tasks that will be removed later.
    pub fn remove_batch(&mut self, tasks: &[Task]) {
        for task in tasks {
            if let Some(idx) = self.node_idx(task) {
                self.graph.remove_node(idx);
                let key = task_key(task);
                self.removed.insert(key);
            }
        }
        self.emit_leaves();
    }

    // use contracts::{ensures, requires};
    // #[requires(self.graph.node_count() > 0)]
    // #[ensures(self.graph.node_count() == old(self.graph.node_count()) - 1)]
    pub fn remove(&mut self, task: &Task) {
        if let Some(idx) = self.node_idx(task) {
            self.graph.remove_node(idx);
            let key = task_key(task);
            self.removed.insert(key);
            self.emit_leaves();
        }
    }

    fn node_idx(&self, task: &Task) -> Option<petgraph::graph::NodeIndex> {
        self.graph
            .node_indices()
            .find(|&idx| &self.graph[idx] == task)
    }

    pub fn all(&self) -> impl Iterator<Item = &Task> {
        self.graph.node_indices().map(|idx| &self.graph[idx])
    }

    pub fn is_linear(&self) -> bool {
        let mut graph = self.graph.clone();
        // pop dependencies off, if we get multiple dependencies at once it's not linear
        loop {
            let leaves = leaves(&graph);
            if leaves.is_empty() {
                return true;
            } else if leaves.len() > 1 {
                return false;
            } else {
                let idx = self
                    .graph
                    .node_indices()
                    .find(|&idx| graph[idx] == leaves[0])
                    .unwrap();
                graph.remove_node(idx);
            }
        }
    }
}

fn leaves(graph: &DiGraph<Task, ()>) -> Vec<Task> {
    graph
        .externals(Direction::Outgoing)
        .map(|idx| graph[idx].clone())
        .collect()
}
