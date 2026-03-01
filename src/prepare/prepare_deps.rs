use std::collections::{HashMap, HashSet};

use eyre::{Result, bail};
use indexmap::IndexSet;
use petgraph::Direction;
use petgraph::algo::is_cyclic_directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use tokio::sync::mpsc;

/// Manages a dependency graph of prepare providers for execution scheduling.
/// Uses Kahn's algorithm to emit providers that are ready to run
/// (i.e., all their dependencies have completed).
#[derive(Debug)]
pub struct PrepareDeps {
    /// The dependency graph where edges point from a provider to its dependencies
    /// (i.e., edge A→B means "A depends on B", so B must run first).
    graph: StableGraph<String, ()>,
    /// Maps provider IDs to their node indices in the graph
    node_indices: HashMap<String, NodeIndex>,
    /// Providers that have already been sent for execution
    sent: HashSet<String>,
    /// Providers that are blocked due to dependency failures or cycles
    blocked: HashSet<String>,
    /// Channel sender for emitting ready providers (None signals completion).
    tx: mpsc::UnboundedSender<Option<String>>,
}

impl PrepareDeps {
    /// Creates a new PrepareDeps from a list of (provider_id, depends) tuples.
    /// Builds the dependency graph based on declared dependencies.
    pub fn new(providers: &[(String, Vec<String>)]) -> Result<Self> {
        let mut graph = StableGraph::new();
        let mut node_indices = HashMap::new();

        // Add all providers to the graph
        for (id, _) in providers {
            if node_indices.contains_key(id) {
                continue;
            }
            let idx = graph.add_node(id.clone());
            node_indices.insert(id.clone(), idx);
        }

        // Add edges for dependencies
        for (id, deps) in providers {
            let Some(&id_idx) = node_indices.get(id) else {
                continue;
            };
            for dep in deps {
                let Some(&dep_idx) = node_indices.get(dep) else {
                    bail!(
                        "prepare provider '{}' depends on unknown provider '{}'",
                        id,
                        dep
                    );
                };
                if id != dep {
                    // Edge from id to dep means "id depends on dep"
                    graph.update_edge(id_idx, dep_idx, ());
                }
            }
        }

        // Create a dummy channel - the real one is created in subscribe()
        let (tx, _) = mpsc::unbounded_channel();

        let mut deps = Self {
            graph,
            node_indices,
            sent: HashSet::new(),
            blocked: HashSet::new(),
            tx,
        };

        // Detect and block any cycles
        deps.detect_and_block_cycles();

        Ok(deps)
    }

    /// Subscribe to receive providers that are ready to run.
    /// Returns a receiver that will emit Some(id) for each ready provider,
    /// followed by None when all providers have been processed.
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<String>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    /// Mark a provider as successfully completed and emit any newly-ready providers.
    pub fn complete_success(&mut self, id: &str) {
        self.remove_node(id);
        self.emit_leaves();
    }

    /// Mark a provider as failed and block all transitive dependents.
    pub fn complete_failure(&mut self, id: &str) {
        // Find and block all transitive dependents before removing the node
        if let Some(&idx) = self.node_indices.get(id) {
            let dependents = self.get_transitive_dependents(idx);
            for dep_idx in dependents {
                if let Some(dep_id) = self.graph.node_weight(dep_idx) {
                    self.blocked.insert(dep_id.clone());
                }
            }
        }

        self.remove_node(id);
        self.emit_leaves();
    }

    /// Returns whether all providers have been processed
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Returns the list of blocked providers
    pub fn blocked_providers(&self) -> Vec<String> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let id = self.graph.node_weight(idx)?;
                if self.blocked.contains(id) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Detect cycles in the graph and mark all nodes in cycles as blocked
    fn detect_and_block_cycles(&mut self) {
        if !is_cyclic_directed(&self.graph) {
            return;
        }

        // Find all nodes that are part of cycles by checking which nodes
        // have no path to a leaf (a node with out-degree 0)
        let mut can_reach_leaf: HashSet<NodeIndex> = HashSet::new();

        // Start with all leaf nodes
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

        // Any node that cannot reach a leaf is in a cycle - block it
        for idx in self.graph.node_indices() {
            if !can_reach_leaf.contains(&idx) {
                if let Some(id) = self.graph.node_weight(idx) {
                    warn!(
                        "prepare provider '{}' is part of a dependency cycle, skipping",
                        id
                    );
                    self.blocked.insert(id.clone());
                }
            }
        }
    }

    /// Emit all providers that have no remaining dependencies (leaf nodes)
    fn emit_leaves(&mut self) {
        let leaves = self.find_leaves();

        for id in leaves {
            // Skip if already sent, blocked, or completed
            if self.sent.contains(&id) || self.blocked.contains(&id) {
                continue;
            }

            if self.sent.insert(id.clone()) {
                trace!("Scheduling prepare provider: {}", id);
                if let Err(e) = self.tx.send(Some(id)) {
                    trace!("Error sending provider: {e:?}");
                }
            }
        }

        // Check if we're done
        if self.is_all_done() {
            trace!("All prepare providers finished");
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing provider stream: {e:?}");
            }
        }
    }

    /// Find all leaf nodes (providers with no unsatisfied dependencies)
    fn find_leaves(&self) -> Vec<String> {
        self.graph
            .externals(Direction::Outgoing)
            .filter_map(|idx| self.graph.node_weight(idx).cloned())
            .collect()
    }

    /// Check if all providers have been processed (sent, completed, or blocked)
    fn is_all_done(&self) -> bool {
        if self.is_empty() {
            return true;
        }

        // Or if all remaining providers are blocked
        self.graph.node_indices().all(|idx| {
            self.graph
                .node_weight(idx)
                .map(|id| self.blocked.contains(id))
                .unwrap_or(true)
        })
    }

    /// Remove a node from the graph by its ID.
    fn remove_node(&mut self, id: &str) {
        if let Some(&idx) = self.node_indices.get(id) {
            self.graph.remove_node(idx);
            self.node_indices.remove(id);
        }
    }

    /// Get all transitive dependents of a node (providers that depend on this one)
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

    #[test]
    fn test_empty_graph() {
        let deps = PrepareDeps::new(&[]).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_no_deps_all_ready() {
        let providers = vec![
            ("npm".to_string(), vec![]),
            ("pip".to_string(), vec![]),
            ("go".to_string(), vec![]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();
        let mut rx = deps.subscribe();

        // All should be emitted immediately since none have dependencies
        let mut emitted = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            emitted.push(id);
        }
        assert_eq!(emitted.len(), 3);
        assert!(emitted.contains(&"npm".to_string()));
        assert!(emitted.contains(&"pip".to_string()));
        assert!(emitted.contains(&"go".to_string()));
    }

    #[test]
    fn test_linear_ordering() {
        let providers = vec![
            ("a".to_string(), vec![]),
            ("b".to_string(), vec!["a".to_string()]),
            ("c".to_string(), vec!["b".to_string()]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();
        let mut rx = deps.subscribe();

        // Only "a" should be ready initially
        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "a");
        assert!(rx.try_recv().is_err()); // nothing else ready

        // Complete "a", "b" should become ready
        deps.complete_success("a");
        let second = rx.try_recv().unwrap().unwrap();
        assert_eq!(second, "b");

        // Complete "b", "c" should become ready
        deps.complete_success("b");
        let third = rx.try_recv().unwrap().unwrap();
        assert_eq!(third, "c");

        // Complete "c", done signal
        deps.complete_success("c");
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_failure_blocks_dependents() {
        let providers = vec![
            ("a".to_string(), vec![]),
            ("b".to_string(), vec!["a".to_string()]),
            ("c".to_string(), vec!["b".to_string()]),
            ("d".to_string(), vec![]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();
        let mut rx = deps.subscribe();

        // "a" and "d" should be ready
        let mut initial = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            initial.push(id);
        }
        assert_eq!(initial.len(), 2);
        assert!(initial.contains(&"a".to_string()));
        assert!(initial.contains(&"d".to_string()));

        // Fail "a" — "b" and "c" should be blocked
        deps.complete_failure("a");
        let blocked = deps.blocked_providers();
        assert!(blocked.contains(&"b".to_string()));
        assert!(blocked.contains(&"c".to_string()));

        // Complete "d" — should get done signal
        deps.complete_success("d");
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_cycle_detection() {
        let providers = vec![
            ("a".to_string(), vec!["b".to_string()]),
            ("b".to_string(), vec!["a".to_string()]),
            ("c".to_string(), vec![]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();

        // Both "a" and "b" should be blocked
        let blocked = deps.blocked_providers();
        assert!(blocked.contains(&"a".to_string()));
        assert!(blocked.contains(&"b".to_string()));

        // "c" should still be emitted
        let mut rx = deps.subscribe();
        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "c");

        deps.complete_success("c");
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }

    #[test]
    fn test_unknown_dep_error() {
        let providers = vec![("a".to_string(), vec!["nonexistent".to_string()])];
        let result = PrepareDeps::new(&providers);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown provider"));
    }

    #[test]
    fn test_diamond_deps() {
        // A depends on B and C, B and C depend on D
        let providers = vec![
            ("d".to_string(), vec![]),
            ("b".to_string(), vec!["d".to_string()]),
            ("c".to_string(), vec!["d".to_string()]),
            ("a".to_string(), vec!["b".to_string(), "c".to_string()]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();
        let mut rx = deps.subscribe();

        // Only "d" should be ready
        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "d");
        assert!(rx.try_recv().is_err());

        // Complete "d", "b" and "c" should become ready
        deps.complete_success("d");
        let mut mid = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            mid.push(id);
        }
        assert_eq!(mid.len(), 2);
        assert!(mid.contains(&"b".to_string()));
        assert!(mid.contains(&"c".to_string()));

        // Complete both, "a" should become ready
        deps.complete_success("b");
        deps.complete_success("c");
        let last = rx.try_recv().unwrap().unwrap();
        assert_eq!(last, "a");

        deps.complete_success("a");
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }
}
