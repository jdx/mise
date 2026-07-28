use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display};
use std::path::{Component, Path, PathBuf};

use eyre::{Result, bail};
use serde::Serialize;

/// A stable, provider-namespaced identifier for a workspace project.
///
/// Providers should derive the local part from ecosystem metadata, such as a
/// package name, rather than from the project's current filesystem location.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    /// Creates an ID whose namespace prevents collisions between providers.
    pub fn new(provider: &str, local_id: &str) -> Result<Self> {
        validate_id_part("provider", provider)?;
        validate_id_part("project", local_id)?;
        Ok(Self(format!("{provider}:{local_id}")))
    }

    /// Returns the serialized project ID.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

fn validate_id_part(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("workspace {kind} ID cannot be empty");
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        bail!(
            "workspace {kind} ID {value:?} contains surrounding whitespace or control characters"
        );
    }
    if kind == "provider" && value.contains(':') {
        bail!("workspace provider ID {value:?} cannot contain ':'");
    }
    Ok(())
}

/// A project discovered from ecosystem-specific workspace metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceProject {
    /// Stable identity used by dependency edges and task scoping.
    pub id: ProjectId,
    /// Normalized path relative to the workspace root. `.` represents the root.
    pub root: PathBuf,
    /// Provider-neutral facts that consumers may use for inspection or inference.
    pub metadata: BTreeMap<String, String>,
    /// Projects that this project directly depends on.
    pub dependencies: BTreeSet<ProjectId>,
}

impl WorkspaceProject {
    /// Creates a project with no metadata or dependencies.
    pub fn new(id: ProjectId, root: impl Into<PathBuf>) -> Self {
        Self {
            id,
            root: root.into(),
            metadata: BTreeMap::new(),
            dependencies: BTreeSet::new(),
        }
    }
}

/// Discovers projects and dependency edges from one workspace ecosystem.
pub trait WorkspaceProvider: Debug + Send + Sync {
    /// Stable namespace used when constructing project IDs.
    fn id(&self) -> &str;

    /// Returns every project known to this provider.
    ///
    /// The returned order is ignored. Project IDs, metadata, and dependency
    /// sets determine the canonical graph order instead.
    fn discover(&self, workspace_root: &Path) -> Result<Vec<WorkspaceProject>>;
}

/// A validated, deterministically ordered project graph from one provider.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceProjectGraph {
    projects: BTreeMap<ProjectId, WorkspaceProject>,
}

impl WorkspaceProjectGraph {
    /// Discovers and validates a provider's projects.
    pub fn discover(
        provider: &dyn WorkspaceProvider,
        workspace_root: &Path,
    ) -> Result<WorkspaceProjectGraph> {
        let provider_id = provider.id();
        validate_id_part("provider", provider_id)?;

        let mut projects = BTreeMap::new();
        for mut project in provider.discover(workspace_root)? {
            let expected_prefix = format!("{provider_id}:");
            let Some(local_id) = project.id.as_str().strip_prefix(&expected_prefix) else {
                bail!(
                    "workspace provider {provider_id:?} returned project ID {:?}; IDs must use the {expected_prefix:?} namespace",
                    project.id
                );
            };
            validate_id_part("project", local_id)?;
            project.root = normalize_project_root(&project.id, &project.root)?;
            let id = project.id.clone();
            if projects.insert(id.clone(), project).is_some() {
                bail!("workspace provider {provider_id:?} returned duplicate project ID {id:?}");
            }
        }

        let graph = Self { projects };
        for project in graph.projects() {
            for dependency in &project.dependencies {
                if graph.get(dependency).is_none() {
                    bail!(
                        "workspace project {:?} depends on unknown project {:?}",
                        project.id,
                        dependency
                    );
                }
            }
        }

        Ok(graph)
    }

    /// Returns projects in stable project-ID order.
    pub fn projects(&self) -> impl ExactSizeIterator<Item = &WorkspaceProject> {
        self.projects.values()
    }

    /// Finds a project by its stable ID.
    pub fn get(&self, id: &ProjectId) -> Option<&WorkspaceProject> {
        self.projects.get(id)
    }
}

fn normalize_project_root(id: &ProjectId, root: &Path) -> Result<PathBuf> {
    if root.is_absolute() {
        bail!(
            "workspace project {id:?} has absolute root {root:?}; roots must be workspace-relative"
        );
    }

    let mut normalized = PathBuf::new();
    for component in root.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir => {
                if !normalized.pop() {
                    bail!(
                        "workspace project {id:?} has root {root:?} that escapes the workspace root"
                    );
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                bail!(
                    "workspace project {id:?} has absolute root {root:?}; roots must be workspace-relative"
                );
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        normalized.push(".");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestProvider {
        projects: Vec<WorkspaceProject>,
    }

    impl WorkspaceProvider for TestProvider {
        fn id(&self) -> &str {
            "test"
        }

        fn discover(&self, _workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
            Ok(self.projects.clone())
        }
    }

    fn project(name: &str, root: &str) -> WorkspaceProject {
        WorkspaceProject::new(ProjectId::new("test", name).unwrap(), root)
    }

    #[test]
    fn project_ids_are_provider_namespaced() {
        assert_eq!(
            ProjectId::new("node", "@scope/app").unwrap().as_str(),
            "node:@scope/app"
        );
        assert!(ProjectId::new("", "app").is_err());
        assert!(ProjectId::new("node", " app").is_err());
    }

    #[test]
    fn discovery_is_ordered_and_normalizes_roots() {
        let provider = TestProvider {
            projects: vec![project("z", "./packages/z"), project("a", "")],
        };

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();
        let projects = graph.projects().collect::<Vec<_>>();

        assert_eq!(projects[0].id.as_str(), "test:a");
        assert_eq!(projects[0].root, Path::new("."));
        assert_eq!(projects[1].id.as_str(), "test:z");
        assert_eq!(projects[1].root, Path::new("packages/z"));
    }

    #[test]
    fn discovery_rejects_duplicate_ids() {
        let provider = TestProvider {
            projects: vec![project("app", "a"), project("app", "b")],
        };

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("duplicate project ID"));
    }

    #[test]
    fn discovery_rejects_invalid_roots() {
        let absolute = TestProvider {
            projects: vec![project("app", "/outside")],
        };
        let escaping = TestProvider {
            projects: vec![project("app", "../outside")],
        };

        assert!(
            WorkspaceProjectGraph::discover(&absolute, Path::new("/workspace"))
                .unwrap_err()
                .to_string()
                .contains("absolute root")
        );
        assert!(
            WorkspaceProjectGraph::discover(&escaping, Path::new("/workspace"))
                .unwrap_err()
                .to_string()
                .contains("escapes the workspace root")
        );
    }

    #[test]
    fn discovery_normalizes_internal_parent_components() {
        let provider = TestProvider {
            projects: vec![project("app", "packages/tmp/../app")],
        };

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();

        assert_eq!(
            graph.projects().next().unwrap().root,
            Path::new("packages/app")
        );
    }

    #[test]
    fn discovery_rejects_dangling_dependency_edges() {
        let mut app = project("app", "app");
        app.dependencies
            .insert(ProjectId::new("test", "missing").unwrap());
        let provider = TestProvider {
            projects: vec![app],
        };

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("depends on unknown project"));
    }

    #[test]
    fn discovery_rejects_ids_from_another_provider() {
        let provider = TestProvider {
            projects: vec![WorkspaceProject::new(
                ProjectId::new("other", "app").unwrap(),
                "app",
            )],
        };

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("must use the \"test:\" namespace"));
    }
}
