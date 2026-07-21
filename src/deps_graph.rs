use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

use eyre::{Result, bail};
use indexmap::IndexSet;
use petgraph::Direction;
use petgraph::algo::is_cyclic_directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use tokio::sync::mpsc;

/// Generic dependency graph scheduler using Kahn's algorithm.
///
/// Emits nodes that are ready to process (all dependencies satisfied)
/// via an mpsc channel. Supports success/failure completion with
/// transitive dependency blocking and cycle detection.
///
/// Type parameters:
/// - `K`: Key type for identifying nodes
/// - `N`: Node value type stored in the graph
#[derive(Debug)]
pub struct DepsGraph<K, N>
where
    K: Hash + Eq + Clone + fmt::Display,
    N: Clone + fmt::Debug,
{
    graph: StableGraph<N, ()>,
    node_indices: HashMap<K, NodeIndex>,
    sent: HashSet<K>,
    blocked: HashSet<K>,
    tx: mpsc::UnboundedSender<Option<N>>,
    key_fn: fn(&N) -> K,
}

impl<K, N> DepsGraph<K, N>
where
    K: Hash + Eq + Clone + fmt::Display,
    N: Clone + fmt::Debug,
{
    /// Create a new DepsGraph.
    ///
    /// - `nodes`: Iterator of (key, node) pairs to add to the graph
    /// - `edges`: Iterator of (from_key, to_key) pairs meaning "from depends on to"
    /// - `key_fn`: Function to extract a key from a node value
    pub fn new(
        nodes: impl IntoIterator<Item = (K, N)>,
        edges: impl IntoIterator<Item = (K, K)>,
        key_fn: fn(&N) -> K,
    ) -> Result<Self> {
        let mut graph = StableGraph::new();
        let mut node_indices = HashMap::new();

        for (key, node) in nodes {
            if node_indices.contains_key(&key) {
                continue;
            }
            let idx = graph.add_node(node);
            node_indices.insert(key, idx);
        }

        for (from_key, to_key) in edges {
            let Some(&from_idx) = node_indices.get(&from_key) else {
                continue;
            };
            let Some(&to_idx) = node_indices.get(&to_key) else {
                bail!("'{}' depends on unknown '{}'", from_key, to_key);
            };
            if from_key != to_key {
                graph.update_edge(from_idx, to_idx, ());
            }
        }

        let (tx, _) = mpsc::unbounded_channel();

        let mut deps = Self {
            graph,
            node_indices,
            sent: HashSet::new(),
            blocked: HashSet::new(),
            tx,
            key_fn,
        };

        deps.detect_and_block_cycles();

        Ok(deps)
    }

    /// Subscribe to receive nodes that are ready to process.
    /// Returns a receiver that emits `Some(node)` for each ready node,
    /// followed by `None` when all nodes have been processed.
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<N>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    /// Mark a node as successfully completed and emit any newly-ready nodes.
    pub fn complete_success(&mut self, key: &K) {
        self.remove_node(key);
        self.emit_leaves();
    }

    /// Mark a node as failed and block all transitive dependents.
    pub fn complete_failure(&mut self, key: &K) {
        if let Some(&idx) = self.node_indices.get(key) {
            let dependents = self.get_transitive_dependents(idx);
            for dep_idx in dependents {
                if let Some(dep_node) = self.graph.node_weight(dep_idx) {
                    let dep_key = (self.key_fn)(dep_node);
                    self.blocked.insert(dep_key);
                }
            }
        }

        self.remove_node(key);
        self.emit_leaves();
    }

    /// Returns whether all nodes have been processed.
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Returns the keys of all blocked nodes (dependency failures or cycles).
    pub fn blocked_keys(&self) -> Vec<K> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = self.graph.node_weight(idx)?;
                let key = (self.key_fn)(node);
                if self.blocked.contains(&key) {
                    Some(key)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns the node values that are blocked.
    pub fn blocked_nodes(&self) -> Vec<N> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = self.graph.node_weight(idx)?;
                let key = (self.key_fn)(node);
                if self.blocked.contains(&key) {
                    Some(node.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Detect cycles and mark all nodes in cycles as blocked.
    fn detect_and_block_cycles(&mut self) {
        if !is_cyclic_directed(&self.graph) {
            return;
        }

        let mut can_reach_leaf: HashSet<NodeIndex> = HashSet::new();

        // Start with all leaf nodes (no outgoing edges = no dependencies)
        for idx in self.graph.node_indices() {
            if self
                .graph
                .neighbors_directed(idx, Direction::Outgoing)
                .next()
                .is_none()
            {
                can_reach_leaf.insert(idx);
            }
        }

        // Propagate backwards: if all dependencies of a node can reach a leaf,
        // then it can also reach a leaf
        let mut changed = true;
        while changed {
            changed = false;
            for idx in self.graph.node_indices() {
                if can_reach_leaf.contains(&idx) {
                    continue;
                }
                let deps_can_reach = self
                    .graph
                    .neighbors_directed(idx, Direction::Outgoing)
                    .all(|dep_idx| can_reach_leaf.contains(&dep_idx));
                if deps_can_reach
                    && self
                        .graph
                        .neighbors_directed(idx, Direction::Outgoing)
                        .next()
                        .is_some()
                {
                    can_reach_leaf.insert(idx);
                    changed = true;
                }
            }
        }

        // Any node that cannot reach a leaf is in a cycle
        for idx in self.graph.node_indices() {
            if !can_reach_leaf.contains(&idx)
                && let Some(node) = self.graph.node_weight(idx)
            {
                let key = (self.key_fn)(node);
                self.blocked.insert(key);
            }
        }
    }

    /// Emit all nodes that have no remaining dependencies (leaf nodes).
    fn emit_leaves(&mut self) {
        let leaves = self.find_leaves();

        for (key, node) in leaves {
            if self.sent.contains(&key) || self.blocked.contains(&key) {
                continue;
            }

            if self.sent.insert(key.clone()) {
                trace!("Scheduling: {}", key);
                if let Err(e) = self.tx.send(Some(node)) {
                    trace!("Error sending node: {e:?}");
                }
            }
        }

        if self.is_all_done() {
            trace!("All nodes finished");
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing stream: {e:?}");
            }
        }
    }

    /// Find all leaf nodes (no unsatisfied dependencies).
    fn find_leaves(&self) -> Vec<(K, N)> {
        self.graph
            .externals(Direction::Outgoing)
            .filter_map(|idx| {
                let node = self.graph.node_weight(idx)?;
                Some(((self.key_fn)(node), node.clone()))
            })
            .collect()
    }

    /// Check if all nodes have been processed (sent, completed, or blocked).
    fn is_all_done(&self) -> bool {
        if self.is_empty() {
            return true;
        }

        self.graph.node_indices().all(|idx| {
            self.graph
                .node_weight(idx)
                .map(|node| self.blocked.contains(&(self.key_fn)(node)))
                .unwrap_or(true)
        })
    }

    /// Remove a node from the graph by its key.
    fn remove_node(&mut self, key: &K) {
        if let Some(&idx) = self.node_indices.get(key) {
            self.graph.remove_node(idx);
            self.node_indices.remove(key);
        }
    }

    /// Get all transitive dependents of a node (nodes that depend on this one).
    fn get_transitive_dependents(&self, start_idx: NodeIndex) -> IndexSet<NodeIndex> {
        let mut dependents = IndexSet::new();
        let mut stack = vec![start_idx];

        while let Some(idx) = stack.pop() {
            for neighbor in self.graph.neighbors_directed(idx, Direction::Incoming) {
                if dependents.insert(neighbor) {
                    stack.push(neighbor);
                }
            }
        }

        dependents
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::ptr_arg)]
    fn string_key(s: &String) -> String {
        s.clone()
    }

    #[test]
    fn test_empty_graph() {
        let deps: DepsGraph<String, String> =
            DepsGraph::new(vec![], Vec::<(String, String)>::new(), string_key).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_no_deps_all_ready() {
        let nodes = vec![
            ("a".into(), "a".into()),
            ("b".into(), "b".into()),
            ("c".into(), "c".into()),
        ];
        let mut deps: DepsGraph<String, String> =
            DepsGraph::new(nodes, Vec::<(String, String)>::new(), string_key).unwrap();
        let mut rx = deps.subscribe();

        let mut emitted = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            emitted.push(id);
        }
        assert_eq!(emitted.len(), 3);
    }

    #[test]
    fn test_linear_ordering() {
        let nodes: Vec<(String, String)> = vec![
            ("a".into(), "a".into()),
            ("b".into(), "b".into()),
            ("c".into(), "c".into()),
        ];
        let edges: Vec<(String, String)> = vec![("b".into(), "a".into()), ("c".into(), "b".into())];
        let mut deps = DepsGraph::new(nodes, edges, string_key).unwrap();
        let mut rx = deps.subscribe();

        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "a");
        assert!(rx.try_recv().is_err());

        deps.complete_success(&"a".into());
        let second = rx.try_recv().unwrap().unwrap();
        assert_eq!(second, "b");

        deps.complete_success(&"b".into());
        let third = rx.try_recv().unwrap().unwrap();
        assert_eq!(third, "c");

        deps.complete_success(&"c".into());
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_failure_blocks_dependents() {
        let nodes: Vec<(String, String)> = vec![
            ("a".into(), "a".into()),
            ("b".into(), "b".into()),
            ("c".into(), "c".into()),
            ("d".into(), "d".into()),
        ];
        let edges: Vec<(String, String)> = vec![("b".into(), "a".into()), ("c".into(), "b".into())];
        let mut deps = DepsGraph::new(nodes, edges, string_key).unwrap();
        let mut rx = deps.subscribe();

        let mut initial = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            initial.push(id);
        }
        assert_eq!(initial.len(), 2);
        assert!(initial.contains(&"a".to_string()));
        assert!(initial.contains(&"d".to_string()));

        deps.complete_failure(&"a".into());
        let blocked = deps.blocked_keys();
        assert!(blocked.contains(&"b".to_string()));
        assert!(blocked.contains(&"c".to_string()));

        deps.complete_success(&"d".into());
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_cycle_detection() {
        let nodes: Vec<(String, String)> = vec![
            ("a".into(), "a".into()),
            ("b".into(), "b".into()),
            ("c".into(), "c".into()),
        ];
        let edges: Vec<(String, String)> = vec![("a".into(), "b".into()), ("b".into(), "a".into())];
        let mut deps = DepsGraph::new(nodes, edges, string_key).unwrap();

        let blocked = deps.blocked_keys();
        assert!(blocked.contains(&"a".to_string()));
        assert!(blocked.contains(&"b".to_string()));

        let mut rx = deps.subscribe();
        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "c");

        deps.complete_success(&"c".into());
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_unknown_dep_error() {
        let nodes: Vec<(String, String)> = vec![("a".into(), "a".into())];
        let edges: Vec<(String, String)> = vec![("a".into(), "nonexistent".into())];
        let result = DepsGraph::new(nodes, edges, string_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown"));
    }
}
