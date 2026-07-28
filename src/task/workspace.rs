use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};

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

impl FromStr for ProjectId {
    type Err = eyre::Report;

    fn from_str(value: &str) -> Result<Self> {
        let Some((provider, local_id)) = value.split_once(':') else {
            bail!("workspace project ID {value:?} must include a provider namespace");
        };
        Self::new(provider, local_id)
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

/// Explicit changes applied after workspace providers discover their projects.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WorkspaceProjectOverride {
    /// Removes the project and every dependency edge connected to it.
    pub remove: bool,
    /// Adds a project at this root or replaces an inferred project's root.
    pub root: Option<PathBuf>,
    /// Replaces provider-inferred metadata when present.
    pub metadata: Option<BTreeMap<String, String>>,
    /// Replaces the complete provider-inferred dependency set when present.
    pub depends: Option<BTreeSet<String>>,
    /// Adds dependency edges after an optional `depends` replacement.
    pub depends_add: BTreeSet<String>,
    /// Removes dependency edges after an optional `depends` replacement.
    pub depends_remove: BTreeSet<String>,
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

/// A validated, deterministically ordered project graph from workspace providers.
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
        Self::discover_all(&[provider], workspace_root)
    }

    /// Discovers and merges projects from multiple workspace providers.
    ///
    /// Providers are evaluated in stable provider-ID order. Dependency edges
    /// are validated after every provider has contributed its projects so an
    /// edge may connect projects from different ecosystems.
    pub fn discover_all(
        providers: &[&dyn WorkspaceProvider],
        workspace_root: &Path,
    ) -> Result<WorkspaceProjectGraph> {
        let mut providers = providers
            .iter()
            .map(|provider| (provider.id().to_string(), *provider))
            .collect::<Vec<_>>();
        providers.sort_by(|(left, _), (right, _)| left.cmp(right));

        for (index, (provider_id, _)) in providers.iter().enumerate() {
            validate_id_part("provider", provider_id)?;
            if index > 0 && providers[index - 1].0 == *provider_id {
                bail!("duplicate workspace provider ID {provider_id:?}");
            }
        }

        let mut projects = BTreeMap::new();
        for (provider_id, provider) in providers {
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
                    bail!(
                        "workspace provider {provider_id:?} returned duplicate project ID {id:?}"
                    );
                }
            }
        }

        let graph = Self { projects };
        graph.validate_dependencies()?;
        Ok(graph)
    }

    /// Applies explicit project and dependency changes after provider discovery.
    pub fn with_overrides(
        mut self,
        overrides: &BTreeMap<String, WorkspaceProjectOverride>,
    ) -> Result<Self> {
        let mut removed = BTreeSet::new();
        for (raw_id, config) in overrides {
            let id = raw_id.parse::<ProjectId>()?;
            validate_override(&id, config)?;

            if config.remove {
                self.projects.remove(&id);
                removed.insert(id);
                continue;
            }

            if !self.projects.contains_key(&id) {
                let root = config.root.clone().ok_or_else(|| {
                    eyre::eyre!(
                        "workspace project {id:?} is not inferred and requires an explicit root"
                    )
                })?;
                self.projects
                    .insert(id.clone(), WorkspaceProject::new(id.clone(), root));
            }

            let project = self.projects.get_mut(&id).expect("project was inserted");
            if let Some(root) = &config.root {
                project.root = normalize_project_root(&id, root)?;
            }
            if let Some(metadata) = &config.metadata {
                project.metadata.clone_from(metadata);
            }
            if let Some(depends) = &config.depends {
                project.dependencies = parse_dependency_ids(&id, "depends", depends)?;
            }
            let depends_remove =
                parse_dependency_ids(&id, "depends_remove", &config.depends_remove)?;
            project
                .dependencies
                .retain(|dependency| !depends_remove.contains(dependency));
            project.dependencies.extend(parse_dependency_ids(
                &id,
                "depends_add",
                &config.depends_add,
            )?);
        }

        if !removed.is_empty() {
            for project in self.projects.values_mut() {
                project
                    .dependencies
                    .retain(|dependency| !removed.contains(dependency));
            }
        }
        self.validate_dependencies()?;
        Ok(self)
    }

    /// Returns projects in stable project-ID order.
    pub fn projects(&self) -> impl ExactSizeIterator<Item = &WorkspaceProject> {
        self.projects.values()
    }

    /// Finds a project by its stable ID.
    pub fn get(&self, id: &ProjectId) -> Option<&WorkspaceProject> {
        self.projects.get(id)
    }

    fn validate_dependencies(&self) -> Result<()> {
        for project in self.projects() {
            for dependency in &project.dependencies {
                if self.get(dependency).is_none() {
                    bail!(
                        "workspace project {:?} depends on unknown project {:?}",
                        project.id,
                        dependency
                    );
                }
            }
        }
        Ok(())
    }
}

fn validate_override(id: &ProjectId, config: &WorkspaceProjectOverride) -> Result<()> {
    if config.remove
        && (config.root.is_some()
            || config.metadata.is_some()
            || config.depends.is_some()
            || !config.depends_add.is_empty()
            || !config.depends_remove.is_empty())
    {
        bail!("removed workspace project {id:?} cannot define other overrides");
    }
    if let Some(dependency) = config
        .depends_add
        .intersection(&config.depends_remove)
        .next()
    {
        bail!("workspace project {id:?} cannot both add and remove dependency {dependency:?}");
    }
    Ok(())
}

fn parse_dependency_ids(
    project_id: &ProjectId,
    field: &str,
    dependencies: &BTreeSet<String>,
) -> Result<BTreeSet<ProjectId>> {
    dependencies
        .iter()
        .map(|dependency| {
            dependency.parse::<ProjectId>().map_err(|err| {
                eyre::eyre!(
                    "workspace project {project_id:?} has invalid {field} entry {dependency:?}: {err}"
                )
            })
        })
        .collect()
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
        id: &'static str,
        projects: Vec<WorkspaceProject>,
    }

    impl WorkspaceProvider for TestProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn discover(&self, _workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
            Ok(self.projects.clone())
        }
    }

    fn project(provider: &str, name: &str, root: &str) -> WorkspaceProject {
        WorkspaceProject::new(ProjectId::new(provider, name).unwrap(), root)
    }

    fn test_provider(projects: Vec<WorkspaceProject>) -> TestProvider {
        TestProvider {
            id: "test",
            projects,
        }
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
        let provider = test_provider(vec![
            project("test", "z", "./packages/z"),
            project("test", "a", ""),
        ]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();
        let projects = graph.projects().collect::<Vec<_>>();

        assert_eq!(projects[0].id.as_str(), "test:a");
        assert_eq!(projects[0].root, Path::new("."));
        assert_eq!(projects[1].id.as_str(), "test:z");
        assert_eq!(projects[1].root, Path::new("packages/z"));
    }

    #[test]
    fn discovery_rejects_duplicate_ids() {
        let provider = test_provider(vec![
            project("test", "app", "a"),
            project("test", "app", "b"),
        ]);

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("duplicate project ID"));
    }

    #[test]
    fn discovery_rejects_invalid_roots() {
        let absolute = test_provider(vec![project("test", "app", "/outside")]);
        let escaping = test_provider(vec![project("test", "app", "../outside")]);

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
        let provider = test_provider(vec![project("test", "app", "packages/tmp/../app")]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();

        assert_eq!(
            graph.projects().next().unwrap().root,
            Path::new("packages/app")
        );
    }

    #[test]
    fn discovery_rejects_dangling_dependency_edges() {
        let mut app = project("test", "app", "app");
        app.dependencies
            .insert(ProjectId::new("test", "missing").unwrap());
        let provider = test_provider(vec![app]);

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("depends on unknown project"));
    }

    #[test]
    fn discovery_rejects_ids_from_another_provider() {
        let provider = TestProvider {
            id: "test",
            projects: vec![WorkspaceProject::new(
                ProjectId::new("other", "app").unwrap(),
                "app",
            )],
        };

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        assert!(err.to_string().contains("must use the \"test:\" namespace"));
    }

    #[test]
    fn discovery_merges_providers_and_cross_provider_edges() {
        let cargo = TestProvider {
            id: "cargo",
            projects: vec![project("cargo", "core", "crates/core")],
        };
        let mut app = project("node", "app", "apps/app");
        app.dependencies
            .insert(ProjectId::new("cargo", "core").unwrap());
        let node = TestProvider {
            id: "node",
            projects: vec![app],
        };

        let graph =
            WorkspaceProjectGraph::discover_all(&[&node, &cargo], Path::new("/workspace")).unwrap();
        let ids = graph
            .projects()
            .map(|project| project.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["cargo:core", "node:app"]);
        assert_eq!(
            graph
                .get(&ProjectId::new("node", "app").unwrap())
                .unwrap()
                .dependencies,
            BTreeSet::from([ProjectId::new("cargo", "core").unwrap()])
        );
    }

    #[test]
    fn discovery_is_independent_of_provider_order() {
        let cargo = TestProvider {
            id: "cargo",
            projects: vec![project("cargo", "core", ".")],
        };
        let node = TestProvider {
            id: "node",
            projects: vec![project("node", "app", ".")],
        };

        let forward =
            WorkspaceProjectGraph::discover_all(&[&cargo, &node], Path::new("/workspace")).unwrap();
        let reverse =
            WorkspaceProjectGraph::discover_all(&[&node, &cargo], Path::new("/workspace")).unwrap();

        assert_eq!(forward, reverse);
    }

    #[test]
    fn discovery_rejects_duplicate_provider_ids() {
        let first = test_provider(vec![project("test", "first", "first")]);
        let second = test_provider(vec![project("test", "second", "second")]);

        let err = WorkspaceProjectGraph::discover_all(&[&first, &second], Path::new("/workspace"))
            .unwrap_err();

        assert!(err.to_string().contains("duplicate workspace provider ID"));
    }

    #[test]
    fn overrides_add_remove_and_modify_projects() {
        let lib_id = ProjectId::new("node", "lib").unwrap();
        let old_id = ProjectId::new("node", "old").unwrap();
        let mut app = project("node", "app", "apps/app");
        app.dependencies = BTreeSet::from([lib_id.clone(), old_id.clone()]);
        let node = TestProvider {
            id: "node",
            projects: vec![
                app,
                project("node", "lib", "packages/lib"),
                project("node", "old", "packages/old"),
            ],
        };
        let cargo = TestProvider {
            id: "cargo",
            projects: vec![project("cargo", "core", "crates/core")],
        };
        let overrides = BTreeMap::from([
            (
                "custom:docs".to_string(),
                WorkspaceProjectOverride {
                    root: Some("docs".into()),
                    ..Default::default()
                },
            ),
            (
                "node:app".to_string(),
                WorkspaceProjectOverride {
                    root: Some("apps/web".into()),
                    metadata: Some(BTreeMap::from([(
                        "kind".to_string(),
                        "frontend".to_string(),
                    )])),
                    depends_add: BTreeSet::from([
                        "cargo:core".to_string(),
                        "custom:docs".to_string(),
                    ]),
                    depends_remove: BTreeSet::from(["node:old".to_string()]),
                    ..Default::default()
                },
            ),
            (
                "node:lib".to_string(),
                WorkspaceProjectOverride {
                    remove: true,
                    ..Default::default()
                },
            ),
        ]);

        let graph = WorkspaceProjectGraph::discover_all(&[&node, &cargo], Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert!(graph.get(&lib_id).is_none());
        assert_eq!(app.root, Path::new("apps/web"));
        assert_eq!(
            app.metadata,
            BTreeMap::from([("kind".to_string(), "frontend".to_string())])
        );
        assert_eq!(
            app.dependencies,
            BTreeSet::from([
                ProjectId::new("cargo", "core").unwrap(),
                ProjectId::new("custom", "docs").unwrap(),
            ])
        );
    }

    #[test]
    fn overrides_can_replace_dependencies() {
        let mut app = project("node", "app", "app");
        app.dependencies
            .insert(ProjectId::new("node", "inferred").unwrap());
        let provider = TestProvider {
            id: "node",
            projects: vec![
                app,
                project("node", "inferred", "inferred"),
                project("node", "explicit", "explicit"),
            ],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                depends: Some(BTreeSet::from(["node:explicit".to_string()])),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();

        assert_eq!(
            graph
                .get(&ProjectId::new("node", "app").unwrap())
                .unwrap()
                .dependencies,
            BTreeSet::from([ProjectId::new("node", "explicit").unwrap()])
        );
    }

    #[test]
    fn overrides_reject_invalid_combinations_and_dangling_edges() {
        let graph = WorkspaceProjectGraph::default();
        let missing_root = BTreeMap::from([(
            "custom:new".to_string(),
            WorkspaceProjectOverride::default(),
        )]);
        assert!(
            graph
                .clone()
                .with_overrides(&missing_root)
                .unwrap_err()
                .to_string()
                .contains("requires an explicit root")
        );

        let remove_with_root = BTreeMap::from([(
            "custom:old".to_string(),
            WorkspaceProjectOverride {
                remove: true,
                root: Some("old".into()),
                ..Default::default()
            },
        )]);
        assert!(
            graph
                .clone()
                .with_overrides(&remove_with_root)
                .unwrap_err()
                .to_string()
                .contains("cannot define other overrides")
        );

        let dangling = BTreeMap::from([(
            "custom:new".to_string(),
            WorkspaceProjectOverride {
                root: Some("new".into()),
                depends_add: BTreeSet::from(["custom:missing".to_string()]),
                ..Default::default()
            },
        )]);
        assert!(
            graph
                .with_overrides(&dangling)
                .unwrap_err()
                .to_string()
                .contains("depends on unknown project")
        );
    }
}
