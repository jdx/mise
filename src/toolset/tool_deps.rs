use std::collections::HashSet;

use eyre::Result;
use tokio::sync::mpsc;

use crate::cli::args::BackendArg;
use crate::deps_graph::DepsGraph;
use crate::toolset::tool_request::ToolRequest;

/// Unique key for a tool request (tool short name + version)
pub type ToolKey = String;

/// Creates a unique key for a ToolRequest
pub(crate) fn tool_key(tr: &ToolRequest) -> ToolKey {
    format!("{}@{}", tr.ba().short, tr.version())
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
    /// Duplicate tool requests (same tool short name and version) are deduplicated.
    /// Distinct aliases may resolve to the same backend/version but still need separate
    /// install jobs because they can have different options and install directories.
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

        // Add edges from user-specified depends in tool options
        for tr in &requests {
            let tr_key = tool_key(tr);
            if let Some(user_deps) = &tr.options().depends {
                for dep_str in user_deps {
                    let dep_ba = BackendArg::from(dep_str.as_str());
                    let dep_fulls = dep_ba.all_fulls();
                    let mut found = false;
                    for other_tr in &requests {
                        let other_fulls = other_tr.ba().all_fulls();
                        if dep_fulls.iter().any(|f| other_fulls.contains(f)) {
                            let other_key = tool_key(other_tr);
                            if tr_key != other_key {
                                edges.push((tr_key.clone(), other_key));
                                found = true;
                            }
                        }
                    }
                    if !found {
                        warn!(
                            "tool '{}': depends on '{}' which is not in the current install set",
                            tr_key, dep_str
                        );
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
    use std::sync::Arc;

    use crate::config::Config;
    use crate::toolset::{ToolSource, ToolVersionOptions};

    #[test]
    fn test_empty_deps() {
        let _deps = ToolDeps::new(vec![]).unwrap();
    }

    #[tokio::test]
    async fn test_aliases_to_same_backend_are_distinct() {
        let _config = Config::get().await.unwrap();
        let source = ToolSource::Argument;
        let backend1 = Arc::new(BackendArg::new(
            "foo".to_string(),
            Some("github:owner/repo".to_string()),
        ));
        let backend2 = Arc::new(BackendArg::new(
            "bar".to_string(),
            Some("github:owner/repo".to_string()),
        ));
        let requests = vec![
            ToolRequest::Version {
                backend: backend1,
                version: "1.0.0".to_string(),
                options: ToolVersionOptions::default(),
                source: source.clone(),
            },
            ToolRequest::Version {
                backend: backend2,
                version: "1.0.0".to_string(),
                options: ToolVersionOptions::default(),
                source,
            },
        ];

        let mut deps = ToolDeps::new(requests).unwrap();
        let mut rx = deps.subscribe();
        let mut emitted = vec![];
        while let Ok(Some(tr)) = rx.try_recv() {
            emitted.push(tr.ba().short.clone());
        }

        emitted.sort();
        assert_eq!(emitted, vec!["bar".to_string(), "foo".to_string()]);
    }
}
