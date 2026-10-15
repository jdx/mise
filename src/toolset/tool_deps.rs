use std::collections::HashMap;

use eyre::{Result, bail};
use tokio::sync::mpsc;

use crate::deps_graph::DepsGraph;
use crate::install_context::install_dependency_declarations;
use crate::toolset::tool_request::ToolRequest;

/// Stable display and result key for an unresolved tool request.
///
/// This deliberately matches the historical progress identity. A request's
/// source controls resolution, but must not leak into user-facing progress
/// rows or result bookkeeping.
pub(super) type ToolKey = String;

/// Creates the stable progress/result key for a ToolRequest.
pub(crate) fn tool_key(tr: &ToolRequest) -> ToolKey {
    format!("{}@{}", tr.ba().short, tr.version())
}

/// Creates a scope-aware key for dependency scheduling and deduplication.
///
/// Two lockfile scopes can resolve the same selector to different concrete
/// pins, so graph nodes must retain their source identity.
pub(crate) fn graph_key(tr: &ToolRequest) -> ToolKey {
    format!("{}#{}", tool_key(tr), tr.source())
}

fn install_destination_key(tr: &ToolRequest) -> String {
    format!("{}@{}", tr.ba().short, tr.version())
}

/// Multiple option variants cannot safely share one install destination.
/// Reject them before any install job starts instead of letting one variant
/// silently satisfy (or race with) another.
pub fn ensure_compatible_install_requests(requests: &[ToolRequest]) -> Result<()> {
    let mut options_by_destination = std::collections::HashMap::new();
    for request in requests {
        let key = install_destination_key(request);
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
    progress_keys: HashMap<ToolKey, ToolKey>,
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
            .map(|tr| (graph_key(tr), tr.clone()))
            .collect();

        // Compute edges from the shared backend/plugin and per-tool declarations.
        // Metadata errors remain ignored here, matching the scheduler's historical
        // behavior; strict consumers validate the same declaration object.
        let mut edges: Vec<(ToolKey, ToolKey)> = vec![];
        for tr in &requests {
            let tr_key = graph_key(tr);
            let declarations = install_dependency_declarations(tr);
            for other_tr in &requests {
                if declarations.matches_request(other_tr) {
                    let other_key = graph_key(other_tr);
                    if tr_key != other_key {
                        edges.push((tr_key.clone(), other_key));
                    }
                }
            }
        }

        let progress_keys = requests
            .iter()
            .map(|tr| (graph_key(tr), tool_key(tr)))
            .collect();
        let inner = DepsGraph::new(nodes, edges, graph_key)?;
        Ok(Self {
            inner,
            progress_keys,
        })
    }

    /// Subscribe to receive tools that are ready to install.
    pub(super) fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<ToolRequest>> {
        self.inner.subscribe()
    }

    /// Mark a tool as successfully installed and emit any newly-ready tools.
    pub(super) fn complete_success(&mut self, tr: &ToolRequest) {
        self.inner.complete_success(&graph_key(tr));
    }

    /// Mark a tool as failed and block all transitive dependents.
    pub(super) fn complete_failure(&mut self, tr: &ToolRequest) {
        self.inner.complete_failure(&graph_key(tr));
    }

    /// Tools this request is still waiting on, by key.
    pub(super) fn pending_dependencies(&self, tr: &ToolRequest) -> Vec<ToolKey> {
        self.inner
            .dependencies_of(&graph_key(tr))
            .into_iter()
            .filter_map(|key| self.progress_keys.get(&key).cloned())
            .collect()
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

    use crate::args::BackendArg;
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
            ToolRequest::new(backend1, "1.0.0", source.clone()).unwrap(),
            ToolRequest::new(backend2, "1.0.0", source).unwrap(),
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
            ToolRequest::new_with_options(
                backend.clone(),
                "1.0.0",
                parse_tool_options("flavor=one"),
                ToolSource::Argument,
            )
            .unwrap(),
            ToolRequest::new_with_options(
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
    fn test_same_destination_from_different_sources_with_different_options_is_rejected() {
        let backend = Arc::new(BackendArg::from("tiny"));
        let requests = vec![
            ToolRequest::new_with_options(
                backend.clone(),
                "1.0.0",
                parse_tool_options("flavor=one"),
                ToolSource::ToolStub("/tmp/first-tool-stub.toml".into()),
            )
            .unwrap(),
            ToolRequest::new_with_options(
                backend,
                "1.0.0",
                parse_tool_options("flavor=two"),
                ToolSource::ToolStub("/tmp/second-tool-stub.toml".into()),
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
    fn selected_python_dependencies_remain_distinct_per_source() {
        let backend = Arc::new(BackendArg::from("pypi:demo"));
        let first = ToolRequest::new_with_options(
            backend.clone(),
            "1.0.0",
            parse_tool_options(r#"python = "3.12""#),
            ToolSource::ToolStub("/tmp/first-tool-stub.toml".into()),
        )
        .unwrap();
        let second = ToolRequest::new_with_options(
            backend,
            "1.0.0",
            parse_tool_options(r#"python = "3.12""#),
            ToolSource::ToolStub("/tmp/second-tool-stub.toml".into()),
        )
        .unwrap();

        let first_python = crate::backend::pipx::selected_python_dependency(&first)
            .unwrap()
            .unwrap();
        let second_python = crate::backend::pipx::selected_python_dependency(&second)
            .unwrap()
            .unwrap();
        assert_eq!(first_python.ba().short, "python");
        assert_eq!(second_python.ba().short, "python");
        assert_ne!(first_python.source(), second_python.source());
        assert_eq!(tool_key(&first_python), tool_key(&second_python));
        assert_ne!(graph_key(&first_python), graph_key(&second_python));
    }

    #[test]
    fn selected_python_dependency_waits_for_configured_option_variant() {
        let source = ToolSource::ToolStub("/tmp/project.mise.toml".into());
        let tool = ToolRequest::new_with_options(
            Arc::new(BackendArg::from("pypi:black")),
            "24.10.0",
            parse_tool_options(r#"python = "3.12""#),
            source.clone(),
        )
        .unwrap();
        let python = ToolRequest::new_with_options(
            Arc::new(BackendArg::from("python")),
            "3.12",
            parse_tool_options("patch_sysconfig = false"),
            source,
        )
        .unwrap();

        assert_dependency_edge(tool, python);
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
            ToolRequest::new_with_options(
                backend.clone(),
                "1.0.0",
                first_options,
                ToolSource::Argument,
            )
            .unwrap(),
            ToolRequest::new_with_options(backend, "1.0.0", second_options, ToolSource::Argument)
                .unwrap(),
        ];

        let err = ToolDeps::new(requests).unwrap_err();
        assert!(
            err.to_string()
                .contains("conflicting options for tiny@1.0.0")
        );
    }

    fn request(tool: &str, options: &str) -> ToolRequest {
        ToolRequest::new_with_options(
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
