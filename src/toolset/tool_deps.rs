use std::collections::HashSet;

use eyre::Result;
use tokio::sync::mpsc;

use crate::deps_graph::DepsGraph;
use crate::toolset::tool_request::ToolRequest;

/// Unique key for a tool request (backend full name + version)
pub type ToolKey = String;

/// Creates a unique key for a ToolRequest
fn tool_key(tr: &ToolRequest) -> ToolKey {
    format!("{}@{}", tr.ba().full(), tr.version())
}

/// Manages a dependency graph of tools for installation scheduling.
/// Thin wrapper around `DepsGraph<ToolKey, ToolRequest>` with
/// tool-specific dependency resolution.
#[derive(Debug)]
pub struct ToolDeps {
    inner: DepsGraph<ToolKey, ToolRequest>,
}

impl ToolDeps {
    /// Creates a new ToolDeps from a list of tool requests.
    /// Builds the dependency graph based on each tool's dependencies.
    /// Duplicate tool requests (same backend and version) are deduplicated.
    pub fn new(requests: Vec<ToolRequest>) -> Result<Self> {
        // Build nodes
        let nodes: Vec<(ToolKey, ToolRequest)> = requests
            .iter()
            .map(|tr| (tool_key(tr), tr.clone()))
            .collect();

        // Build a set of all tool identifiers being installed for dependency lookup
        let versions_hash: HashSet<String> =
            requests.iter().flat_map(|tr| tr.ba().all_fulls()).collect();

        // Compute edges from backend dependencies
        let mut edges: Vec<(ToolKey, ToolKey)> = vec![];
        for tr in &requests {
            let tr_key = tool_key(tr);

            if let Ok(backend) = tr.backend()
                && let Ok(deps) = backend.get_all_dependencies(true)
            {
                for dep_ba in deps {
                    let dep_fulls = dep_ba.all_fulls();
                    if dep_fulls.iter().any(|full| versions_hash.contains(full)) {
                        for other_tr in &requests {
                            let other_fulls = other_tr.ba().all_fulls();
                            if dep_fulls.iter().any(|f| other_fulls.contains(f)) {
                                let other_key = tool_key(other_tr);
                                if tr_key != other_key {
                                    edges.push((tr_key.clone(), other_key));
                                }
                            }
                        }
                    }
                }
            }
        }

        let inner = DepsGraph::new(nodes, edges, tool_key)?;
        Ok(Self { inner })
    }

    /// Subscribe to receive tools that are ready to install.
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<ToolRequest>> {
        self.inner.subscribe()
    }

    /// Mark a tool as successfully installed and emit any newly-ready tools.
    pub fn complete_success(&mut self, tr: &ToolRequest) {
        self.inner.complete_success(&tool_key(tr));
    }

    /// Mark a tool as failed and block all transitive dependents.
    pub fn complete_failure(&mut self, tr: &ToolRequest) {
        self.inner.complete_failure(&tool_key(tr));
    }

    /// Returns the list of blocked tools (those whose dependencies failed or are in cycles)
    pub fn blocked_tools(&self) -> Vec<ToolRequest> {
        self.inner.blocked_nodes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_deps() {
        let _deps = ToolDeps::new(vec![]).unwrap();
    }
}
