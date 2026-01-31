use std::collections::{HashMap, HashSet};

use eyre::Result;
use indexmap::IndexSet;
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use tokio::sync::mpsc;

use crate::toolset::tool_request::ToolRequest;

/// Unique key for a tool request (backend full name + version)
pub type ToolKey = String;

/// Creates a unique key for a ToolRequest
fn tool_key(tr: &ToolRequest) -> ToolKey {
    format!("{}@{}", tr.ba().full(), tr.version())
}

/// Manages a dependency graph of tools for installation scheduling.
/// Uses Kahn's algorithm to emit tools that are ready to install
/// (i.e., all their dependencies have been installed).
#[derive(Debug)]
pub struct ToolDeps {
    /// The dependency graph where edges point from a tool to its dependencies
    /// (i.e., edge A→B means "A depends on B", so B must be installed first)
    graph: DiGraph<ToolRequest, ()>,
    /// Maps tool keys to their node indices in the graph
    node_indices: HashMap<ToolKey, NodeIndex>,
    /// Tools that have already been sent for installation
    sent: HashSet<ToolKey>,
    /// Tools that have completed installation (success or failure)
    completed: HashSet<ToolKey>,
    /// Tools that failed to install
    failed: HashSet<ToolKey>,
    /// Tools that are blocked due to dependency failures
    blocked: HashSet<ToolKey>,
    /// Channel sender for emitting ready tools (None signals completion)
    tx: mpsc::UnboundedSender<Option<ToolRequest>>,
}

impl ToolDeps {
    /// Creates a new ToolDeps from a list of tool requests.
    /// Builds the dependency graph based on each tool's dependencies.
    pub fn new(requests: Vec<ToolRequest>) -> Result<Self> {
        let mut graph = DiGraph::new();
        let mut node_indices = HashMap::new();

        // First pass: add all requested tools to the graph
        for tr in &requests {
            let key = tool_key(tr);
            let idx = graph.add_node(tr.clone());
            node_indices.insert(key, idx);
        }

        // Build a set of all tool identifiers being installed for dependency lookup
        let versions_hash: HashSet<String> =
            requests.iter().flat_map(|tr| tr.ba().all_fulls()).collect();

        // Second pass: add edges for dependencies
        for tr in &requests {
            let tr_key = tool_key(tr);
            let tr_idx = node_indices[&tr_key];

            // Get all dependencies for this tool
            if let Ok(backend) = tr.backend() {
                if let Ok(deps) = backend.get_all_dependencies(true) {
                    for dep_ba in deps {
                        // Check if this dependency is being installed
                        let dep_fulls = dep_ba.all_fulls();
                        if dep_fulls.iter().any(|full| versions_hash.contains(full)) {
                            // Find the matching tool request in our set
                            for other_tr in &requests {
                                let other_fulls = other_tr.ba().all_fulls();
                                if dep_fulls.iter().any(|f| other_fulls.contains(f)) {
                                    let other_key = tool_key(other_tr);
                                    if tr_key != other_key {
                                        let other_idx = node_indices[&other_key];
                                        // Edge from tr to dep means "tr depends on dep"
                                        graph.update_edge(tr_idx, other_idx, ());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let (tx, _) = mpsc::unbounded_channel();

        Ok(Self {
            graph,
            node_indices,
            sent: HashSet::new(),
            completed: HashSet::new(),
            failed: HashSet::new(),
            blocked: HashSet::new(),
            tx,
        })
    }

    /// Subscribe to receive tools that are ready to install.
    /// Returns a receiver that will emit Some(ToolRequest) for each ready tool,
    /// followed by None when all tools have been processed.
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<ToolRequest>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.tx = tx;
        self.emit_leaves();
        rx
    }

    /// Mark a tool as successfully installed and emit any newly-ready tools.
    pub fn complete_success(&mut self, tr: &ToolRequest) {
        let key = tool_key(tr);
        self.completed.insert(key.clone());
        self.remove_node(tr);
        self.emit_leaves();
    }

    /// Mark a tool as failed and block all transitive dependents.
    pub fn complete_failure(&mut self, tr: &ToolRequest) {
        let key = tool_key(tr);
        self.completed.insert(key.clone());
        self.failed.insert(key.clone());

        // Find and block all transitive dependents
        if let Some(&idx) = self.node_indices.get(&key) {
            let dependents = self.get_transitive_dependents(idx);
            for dep_idx in dependents {
                let dep_tr = &self.graph[dep_idx];
                let dep_key = tool_key(dep_tr);
                self.blocked.insert(dep_key);
            }
        }

        self.remove_node(tr);
        self.emit_leaves();
    }

    /// Returns whether all tools have been processed
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Returns the list of blocked tools (those whose dependencies failed)
    pub fn blocked_tools(&self) -> Vec<ToolRequest> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let tr = &self.graph[idx];
                self.blocked.contains(&tool_key(tr))
            })
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// Emit all tools that have no remaining dependencies (leaf nodes)
    fn emit_leaves(&mut self) {
        let leaves = self.find_leaves();

        for tr in leaves {
            let key = tool_key(&tr);

            // Skip if already sent, blocked, or completed
            if self.sent.contains(&key) || self.blocked.contains(&key) {
                continue;
            }

            if self.sent.insert(key) {
                trace!("Scheduling tool install: {}", tr);
                if let Err(e) = self.tx.send(Some(tr)) {
                    trace!("Error sending tool: {e:?}");
                }
            }
        }

        // Check if we're done
        if self.is_all_done() {
            trace!("All tool installations finished");
            if let Err(e) = self.tx.send(None) {
                trace!("Error closing tool stream: {e:?}");
            }
        }
    }

    /// Find all leaf nodes (tools with no unsatisfied dependencies)
    fn find_leaves(&self) -> Vec<ToolRequest> {
        self.graph
            .externals(Direction::Outgoing)
            .map(|idx| self.graph[idx].clone())
            .collect()
    }

    /// Check if all tools have been processed (sent, completed, or blocked)
    fn is_all_done(&self) -> bool {
        // All done if graph is empty
        if self.is_empty() {
            return true;
        }

        // Or if all remaining tools are blocked
        self.graph.node_indices().all(|idx| {
            let tr = &self.graph[idx];
            let key = tool_key(tr);
            self.blocked.contains(&key)
        })
    }

    /// Remove a node from the graph
    fn remove_node(&mut self, tr: &ToolRequest) {
        let key = tool_key(tr);
        if let Some(&idx) = self.node_indices.get(&key) {
            self.graph.remove_node(idx);
            // Note: petgraph may reuse indices, so we should rebuild node_indices
            // However, since we never add new nodes after construction, we can just
            // leave stale entries (they won't match anything)
        }
    }

    /// Get all transitive dependents of a node (tools that depend on this one)
    fn get_transitive_dependents(&self, start_idx: NodeIndex) -> IndexSet<NodeIndex> {
        let mut dependents = IndexSet::new();
        let mut stack = vec![start_idx];

        while let Some(idx) = stack.pop() {
            // Find all nodes that have an edge TO this node (i.e., depend on it)
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
    fn test_empty_deps() {
        let deps = ToolDeps::new(vec![]).unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn test_find_leaves_empty_graph() {
        let deps = ToolDeps::new(vec![]).unwrap();
        let leaves = deps.find_leaves();
        assert!(leaves.is_empty());
    }

    #[test]
    fn test_is_all_done_empty() {
        let deps = ToolDeps::new(vec![]).unwrap();
        assert!(deps.is_all_done());
    }
}
