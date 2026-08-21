use eyre::{Result, bail};
use tokio::sync::mpsc;

use crate::deps_graph::DepsGraph;
use crate::install_context::install_dependency_declarations;
use crate::toolset::tool_request::ToolRequest;

/// Unique key for a tool request (tool short name + version).
pub(super) type ToolKey = String;

/// Creates a unique key for a ToolRequest
pub(crate) fn tool_key(tr: &ToolRequest) -> ToolKey {
    format!("{}@{}", tr.ba().short, tr.version())
}

/// Multiple option variants cannot safely share one install destination.
/// Reject them before any install job starts instead of letting one variant
/// silently satisfy (or race with) another.
pub(crate) fn ensure_compatible_install_requests(requests: &[ToolRequest]) -> Result<()> {
    let mut options_by_destination = std::collections::HashMap::new();
    for request in requests {
        let key = tool_key(request);
        let options = request.options();
        if let Some(existing) = options_by_destination.get(&key)
            && existing != &options
        {
            bail!(
                "conflicting options for {key}: multiple requests share the same install destination"
            );
        }
        options_by_destination.insert(key, options);
    }
    Ok(())
}

/// Manages a dependency graph of tools for installation scheduling.
/// Thin wrapper around `DepsGraph<ToolKey, ToolRequest>` with
/// tool-specific dependency resolution.
#[derive(Debug)]
pub(super) struct ToolDeps {
    inner: DepsGraph<ToolKey, ToolRequest>,
}

impl ToolDeps {
    /// Creates a new ToolDeps from a list of tool requests.
    /// Builds the dependency graph based on each tool's dependencies.
    /// Duplicate tool requests (same tool short name and version) are deduplicated.
    /// Distinct aliases may resolve to the same backend/version but still need separate
    /// install jobs because they have distinct short names and install directories.
    pub(super) fn new(requests: Vec<ToolRequest>) -> Result<Self> {
        ensure_compatible_install_requests(&requests)?;

        // Build nodes
        let nodes: Vec<(ToolKey, ToolRequest)> = requests
            .iter()
            .map(|tr| (tool_key(tr), tr.clone()))
            .collect();

        // Compute edges from the shared backend/plugin and per-tool declarations.
        // Metadata errors remain ignored here, matching the scheduler's historical
        // behavior; strict consumers validate the same declaration object.
        let mut edges: Vec<(ToolKey, ToolKey)> = vec![];
        for tr in &requests {
            let tr_key = tool_key(tr);
            let declarations = install_dependency_declarations(tr);
            for dependency in declarations.iter() {
                for other_tr in &requests {
                    if dependency
                        .all_fulls()
                        .iter()
                        .any(|identity| other_tr.ba().all_fulls().contains(identity))
                    {
                        let other_key = tool_key(other_tr);
                        if tr_key != other_key {
                            edges.push((tr_key.clone(), other_key));
                        }
                    }
                }
            }
            for (raw, dependency) in declarations.direct() {
                if !requests.iter().any(|other| {
                    tr_key != tool_key(other)
                        && dependency
                            .all_fulls()
                            .iter()
                            .any(|identity| other.ba().all_fulls().contains(identity))
                }) {
                    warn!(
                        "tool '{}': depends on '{}' which is not in the current install set",
                        tr_key, raw
                    );
                }
            }
        }

        let inner = DepsGraph::new(nodes, edges, tool_key)?;
        Ok(Self { inner })
    }

    /// Subscribe to receive tools that are ready to install.
    pub(super) fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<ToolRequest>> {
        self.inner.subscribe()
    }

    /// Mark a tool as successfully installed and emit any newly-ready tools.
    pub(super) fn complete_success(&mut self, tr: &ToolRequest) {
        self.inner.complete_success(&tool_key(tr));
    }

    /// Mark a tool as failed and block all transitive dependents.
    pub(super) fn complete_failure(&mut self, tr: &ToolRequest) {
        self.inner.complete_failure(&tool_key(tr));
    }

    /// Returns the list of blocked tools (those whose dependencies failed or are in cycles)
    pub(super) fn blocked_tools(&self) -> Vec<ToolRequest> {
        self.inner.blocked_nodes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::cli::args::BackendArg;
    use crate::config::Config;
    use crate::toolset::{CoreToolOptions, ToolSource, ToolVersionOptions, parse_tool_options};

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

    #[test]
    fn test_same_tool_and_version_with_different_options_are_rejected() {
        let backend = Arc::new(BackendArg::from("tiny"));
        let requests = vec![
            ToolRequest::new_opts(
                backend.clone(),
                "1.0.0",
                parse_tool_options("flavor=one"),
                ToolSource::Argument,
            )
            .unwrap(),
            ToolRequest::new_opts(
                backend,
                "1.0.0",
                parse_tool_options("flavor=two"),
                ToolSource::Argument,
            )
            .unwrap(),
        ];

        let err = ToolDeps::new(requests).unwrap_err();
        assert!(
            err.to_string()
                .contains("conflicting options for tiny@1.0.0")
        );
    }

    #[test]
    fn test_same_tool_and_version_with_different_core_options_are_rejected() {
        let backend = Arc::new(BackendArg::from("tiny"));
        let first_options = ToolVersionOptions {
            core: CoreToolOptions {
                depends: Some(vec!["node".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };
        let second_options = ToolVersionOptions {
            core: CoreToolOptions {
                depends: Some(vec!["python".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        };
        let requests = vec![
            ToolRequest::Version {
                backend: backend.clone(),
                version: "1.0.0".to_string(),
                options: first_options,
                source: ToolSource::Argument,
            },
            ToolRequest::Version {
                backend,
                version: "1.0.0".to_string(),
                options: second_options,
                source: ToolSource::Argument,
            },
        ];

        let err = ToolDeps::new(requests).unwrap_err();
        assert!(
            err.to_string()
                .contains("conflicting options for tiny@1.0.0")
        );
    }

    fn request(tool: &str, options: &str) -> ToolRequest {
        ToolRequest::new_opts(
            Arc::new(BackendArg::from(tool)),
            "1.0.0",
            parse_tool_options(options),
            ToolSource::Argument,
        )
        .unwrap()
    }

    fn assert_dependency_edge(dependent: ToolRequest, dependency: ToolRequest) {
        let mut deps = ToolDeps::new(vec![dependent.clone(), dependency.clone()]).unwrap();
        let mut rx = deps.subscribe();
        assert_eq!(rx.try_recv().unwrap(), Some(dependency.clone()));
        assert!(rx.try_recv().is_err());
        deps.complete_success(&dependency);
        assert_eq!(rx.try_recv().unwrap(), Some(dependent));
    }

    #[tokio::test]
    async fn backend_dependency_graph_edge_is_preserved() {
        let _config = Config::get().await.unwrap();
        assert_dependency_edge(request("elixir", ""), request("erlang", ""));
    }

    #[tokio::test]
    async fn per_tool_dependency_graph_edge_is_preserved() {
        let _config = Config::get().await.unwrap();
        assert_dependency_edge(
            request("needs-dummy", "depends=dummy"),
            request("dummy", ""),
        );
    }
}
