use eyre::{Result, bail};
use tokio::sync::mpsc;

use crate::deps_graph::DepsGraph;

/// Manages a dependency graph of prepare providers for execution scheduling.
/// Thin wrapper around `DepsGraph<String, String>` with prepare-specific
/// validation and error messages.
#[derive(Debug)]
pub struct PrepareDeps {
    inner: DepsGraph<String, String>,
}

impl PrepareDeps {
    /// Creates a new PrepareDeps from a list of (provider_id, depends) tuples.
    pub fn new(providers: &[(String, Vec<String>)]) -> Result<Self> {
        // Validate that all deps reference known providers before building the graph
        let known: std::collections::HashSet<&str> =
            providers.iter().map(|(id, _)| id.as_str()).collect();
        for (id, deps) in providers {
            for dep in deps {
                if !known.contains(dep.as_str()) {
                    bail!(
                        "prepare provider '{}' depends on unknown provider '{}'",
                        id,
                        dep
                    );
                }
            }
        }

        let nodes: Vec<(String, String)> = providers
            .iter()
            .map(|(id, _)| (id.clone(), id.clone()))
            .collect();

        let edges: Vec<(String, String)> = providers
            .iter()
            .flat_map(|(id, deps)| deps.iter().map(move |dep| (id.clone(), dep.clone())))
            .collect();

        let inner = DepsGraph::new(nodes, edges, |s: &String| s.clone())?;
        Ok(Self { inner })
    }

    /// Subscribe to receive providers that are ready to run.
    pub fn subscribe(&mut self) -> mpsc::UnboundedReceiver<Option<String>> {
        self.inner.subscribe()
    }

    /// Mark a provider as successfully completed.
    pub fn complete_success(&mut self, id: &str) {
        self.inner.complete_success(&id.to_string());
    }

    /// Mark a provider as failed and block all transitive dependents.
    pub fn complete_failure(&mut self, id: &str) {
        self.inner.complete_failure(&id.to_string());
    }

    /// Returns the list of blocked provider IDs.
    pub fn blocked_providers(&self) -> Vec<String> {
        self.inner.blocked_keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_graph() {
        let _deps = PrepareDeps::new(&[]).unwrap();
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

        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "a");
        assert!(rx.try_recv().is_err());

        deps.complete_success("a");
        let second = rx.try_recv().unwrap().unwrap();
        assert_eq!(second, "b");

        deps.complete_success("b");
        let third = rx.try_recv().unwrap().unwrap();
        assert_eq!(third, "c");

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

        let mut initial = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            initial.push(id);
        }
        assert_eq!(initial.len(), 2);
        assert!(initial.contains(&"a".to_string()));
        assert!(initial.contains(&"d".to_string()));

        deps.complete_failure("a");
        let blocked = deps.blocked_providers();
        assert!(blocked.contains(&"b".to_string()));
        assert!(blocked.contains(&"c".to_string()));

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

        let blocked = deps.blocked_providers();
        assert!(blocked.contains(&"a".to_string()));
        assert!(blocked.contains(&"b".to_string()));

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
        let providers = vec![
            ("d".to_string(), vec![]),
            ("b".to_string(), vec!["d".to_string()]),
            ("c".to_string(), vec!["d".to_string()]),
            ("a".to_string(), vec!["b".to_string(), "c".to_string()]),
        ];
        let mut deps = PrepareDeps::new(&providers).unwrap();
        let mut rx = deps.subscribe();

        let first = rx.try_recv().unwrap().unwrap();
        assert_eq!(first, "d");
        assert!(rx.try_recv().is_err());

        deps.complete_success("d");
        let mut mid = vec![];
        while let Ok(Some(id)) = rx.try_recv() {
            mid.push(id);
        }
        assert_eq!(mid.len(), 2);
        assert!(mid.contains(&"b".to_string()));
        assert!(mid.contains(&"c".to_string()));

        deps.complete_success("b");
        deps.complete_success("c");
        let last = rx.try_recv().unwrap().unwrap();
        assert_eq!(last, "a");

        deps.complete_success("a");
        let done = rx.try_recv().unwrap();
        assert!(done.is_none());
    }
}
