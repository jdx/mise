use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display};
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use eyre::{Result, bail};
use serde::{Deserialize, Serialize};

use super::{Task, TaskCacheConfig, task_sources::TaskOutputs};

pub mod cargo;
pub mod node;

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
    /// Provider and metadata source that inferred this project.
    pub provenance: WorkspaceProvenance,
    /// Provider attribution for each inferred dependency edge.
    pub dependency_provenance: BTreeMap<ProjectId, WorkspaceProvenance>,
    /// Tasks inferred from ecosystem-specific project metadata.
    pub tasks: BTreeMap<String, WorkspaceTask>,
}

impl WorkspaceProject {
    /// Creates a project with no metadata, dependencies, or inferred tasks.
    pub fn new(id: ProjectId, root: impl Into<PathBuf>) -> Self {
        Self {
            id,
            root: root.into(),
            metadata: BTreeMap::new(),
            dependencies: BTreeSet::new(),
            provenance: WorkspaceProvenance::default(),
            dependency_provenance: BTreeMap::new(),
            tasks: BTreeMap::new(),
        }
    }
}

/// Attribution for a value inferred by a workspace provider.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorkspaceProvenance {
    /// Workspace provider that inferred the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Ecosystem metadata file that supplied the value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
}

/// A provider-neutral task inferred for a workspace project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WorkspaceTask {
    /// Command to execute in the project root.
    pub command: String,
    /// Human-readable description of the inferred task.
    pub description: String,
    /// File relative to the workspace root that supplied the task.
    pub source: PathBuf,
    /// Provider and metadata source that inferred this task.
    pub provenance: WorkspaceProvenance,
    /// Provider-derived task configuration applied before user defaults.
    pub suggestions: WorkspaceTaskSuggestions,
}

/// Optional task configuration a workspace provider can derive confidently.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorkspaceTaskSuggestions {
    /// Project-relative input patterns, mapped to mise task sources.
    pub inputs: Vec<String>,
    /// Project-relative output patterns. An empty vector means the task has no file outputs.
    pub outputs: Option<Vec<String>>,
    /// Whether task output caching should be enabled or disabled.
    pub cache: Option<bool>,
    /// Task dependencies, including project-relative and `^task` dependencies. An empty vector
    /// means the task has no dependencies.
    pub depends: Option<Vec<String>>,
    /// Additional metadata files that contributed these suggestions. These paths are used
    /// internally for cache invalidation and may be absolute, so provenance is the portable
    /// representation exposed in graph JSON.
    #[serde(skip)]
    pub config_sources: Vec<PathBuf>,
    /// Field-level attribution for provider suggestions.
    pub provenance: WorkspaceTaskSuggestionProvenance,
}

/// Provider attribution for each independently suggested task field.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WorkspaceTaskSuggestionProvenance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inputs: Option<WorkspaceProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<WorkspaceProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<WorkspaceProvenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends: Option<WorkspaceProvenance>,
}

impl WorkspaceTaskSuggestions {
    pub(crate) fn apply_before_defaults(&self, task: &mut Task) {
        if !self.inputs.is_empty() {
            task.sources.clone_from(&self.inputs);
        }
        if let Some(outputs) = &self.outputs {
            task.outputs = if outputs.is_empty() {
                TaskOutputs::NoFiles
            } else {
                TaskOutputs::Files(outputs.clone())
            };
        }
        if let Some(depends) = &self.depends
            && !depends.is_empty()
        {
            task.depends = depends.iter().cloned().map(Into::into).collect();
        }
        task.additional_config_sources
            .extend(self.config_sources.iter().cloned());
    }

    pub(crate) fn apply_after_defaults(&self, task: &mut Task) {
        if self.outputs.as_ref().is_some_and(Vec::is_empty) {
            task.outputs = TaskOutputs::NoFiles;
        }
        if self.depends.as_ref().is_some_and(Vec::is_empty) {
            task.depends.clear();
        }
        if let Some(enabled) = self.cache {
            task.cache
                .get_or_insert_with(TaskCacheConfig::default)
                .enabled = enabled;
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

    /// Re-discovers location-derived tasks after an explicit root override.
    fn discover_project_tasks(
        &self,
        _workspace_root: &Path,
        _project_root: &Path,
    ) -> Result<BTreeMap<String, WorkspaceTask>> {
        Ok(BTreeMap::new())
    }
}

/// A validated, deterministically ordered project graph from workspace providers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceProjectGraph {
    projects: BTreeMap<ProjectId, WorkspaceProject>,
    provider_errors: BTreeMap<String, String>,
}

/// A deterministic dependency cycle found in the workspace project graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceProjectCycleError {
    path: Vec<ProjectId>,
}

impl WorkspaceProjectCycleError {
    /// Returns the closed cycle path, with the first project repeated at the end.
    pub fn path(&self) -> &[ProjectId] {
        &self.path
    }
}

impl Display for WorkspaceProjectCycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self
            .path
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" -> ");
        let project = format!(
            "{:?}",
            self.path.first().expect("cycle path is not empty").as_str()
        );
        write!(
            f,
            "workspace project dependency cycle detected: {path}; adjust [monorepo.projects.{project}] depends, depends_add, or depends_remove to break the cycle"
        )
    }
}

impl std::error::Error for WorkspaceProjectCycleError {}

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
        let graph = Self::collect_provider_projects(providers, workspace_root, false)?;
        graph.validate()?;
        Ok(graph)
    }

    /// Discovers projects, applies explicit overrides, and validates the result.
    ///
    /// Provider edges are intentionally not validated until after overrides so
    /// explicit configuration can repair incomplete or incorrect inference.
    pub fn discover_all_with_overrides(
        providers: &[&dyn WorkspaceProvider],
        workspace_root: &Path,
        overrides: &BTreeMap<String, WorkspaceProjectOverride>,
    ) -> Result<WorkspaceProjectGraph> {
        Self::discover_all_with_overrides_inner(providers, workspace_root, overrides, false)
    }

    /// Discovers projects for task loading while isolating provider failures.
    ///
    /// A malformed manifest in one ecosystem must not disable inferred tasks
    /// from another ecosystem. Explicit overrides and successfully discovered
    /// provider results remain fully validated.
    pub fn discover_all_with_overrides_lenient(
        providers: &[&dyn WorkspaceProvider],
        workspace_root: &Path,
        overrides: &BTreeMap<String, WorkspaceProjectOverride>,
    ) -> Result<WorkspaceProjectGraph> {
        Self::discover_all_with_overrides_inner(providers, workspace_root, overrides, true)
    }

    fn discover_all_with_overrides_inner(
        providers: &[&dyn WorkspaceProvider],
        workspace_root: &Path,
        overrides: &BTreeMap<String, WorkspaceProjectOverride>,
        skip_provider_errors: bool,
    ) -> Result<WorkspaceProjectGraph> {
        let mut graph =
            Self::collect_provider_projects(providers, workspace_root, skip_provider_errors)?
                .with_overrides(overrides)?;
        for (raw_id, config) in overrides {
            if config.remove || config.root.is_none() {
                continue;
            }
            let id = raw_id.parse::<ProjectId>()?;
            let Some((provider_id, _)) = id.as_str().split_once(':') else {
                continue;
            };
            let Some(provider) = providers
                .iter()
                .find(|provider| provider.id() == provider_id)
            else {
                continue;
            };
            let project = graph
                .projects
                .get_mut(&id)
                .expect("non-removed override project exists");
            match provider.discover_project_tasks(workspace_root, project.root.as_path()) {
                Ok(mut tasks) => {
                    for task in tasks.values_mut() {
                        attach_task_provenance(task, provider_id);
                    }
                    project.tasks = tasks;
                }
                Err(error) => {
                    warn!(
                        "failed to infer workspace tasks for {id} at {}: {error:#}",
                        project.root.display()
                    );
                    project.tasks.clear();
                }
            }
        }
        Ok(graph)
    }

    fn collect_provider_projects(
        providers: &[&dyn WorkspaceProvider],
        workspace_root: &Path,
        skip_provider_errors: bool,
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
        let mut provider_errors = BTreeMap::new();
        for (provider_id, provider) in providers {
            let discovered = match provider.discover(workspace_root) {
                Ok(projects) => projects,
                Err(error) if skip_provider_errors => {
                    let error = format!("{error:#}");
                    warn!(
                        "failed to discover {provider_id} workspace projects at {}; inferred tasks \
                         and upstream task dependencies from this provider are unavailable: {error}",
                        workspace_root.display(),
                    );
                    provider_errors.insert(provider_id, error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            for mut project in discovered {
                let expected_prefix = format!("{provider_id}:");
                let Some(local_id) = project.id.as_str().strip_prefix(&expected_prefix) else {
                    bail!(
                        "workspace provider {provider_id:?} returned project ID {:?}; IDs must use the {expected_prefix:?} namespace",
                        project.id
                    );
                };
                validate_id_part("project", local_id)?;
                project.root = normalize_project_root(&project.id, &project.root)?;
                attach_provider_provenance(&mut project, &provider_id);
                let id = project.id.clone();
                if projects.insert(id.clone(), project).is_some() {
                    bail!(
                        "workspace provider {provider_id:?} returned duplicate project ID {id:?}"
                    );
                }
            }
        }

        Ok(Self {
            projects,
            provider_errors,
        })
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
                let root = normalize_project_root(&id, root)?;
                if root != project.root {
                    project.tasks.clear();
                    for provenance in project.dependency_provenance.values_mut() {
                        provenance.source = None;
                    }
                    project.root = root;
                }
                project.provenance = WorkspaceProvenance::default();
            }
            if let Some(metadata) = &config.metadata {
                project.metadata.clone_from(metadata);
                project.provenance = WorkspaceProvenance::default();
            }
            if let Some(depends) = &config.depends {
                project.dependencies = parse_dependency_ids(&id, "depends", depends)?;
                project.dependency_provenance.clear();
            }
            let depends_remove =
                parse_dependency_ids(&id, "depends_remove", &config.depends_remove)?;
            project
                .dependencies
                .retain(|dependency| !depends_remove.contains(dependency));
            project
                .dependency_provenance
                .retain(|dependency, _| !depends_remove.contains(dependency));
            let depends_add = parse_dependency_ids(&id, "depends_add", &config.depends_add)?;
            for dependency in depends_add {
                if project.dependencies.insert(dependency.clone()) {
                    project.dependency_provenance.remove(&dependency);
                }
            }
        }

        if !removed.is_empty() {
            for project in self.projects.values_mut() {
                project
                    .dependencies
                    .retain(|dependency| !removed.contains(dependency));
                project
                    .dependency_provenance
                    .retain(|dependency, _| !removed.contains(dependency));
            }
        }
        self.validate()?;
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

    /// Summarizes provider failures retained during lenient task discovery.
    pub(crate) fn provider_discovery_error(&self) -> Option<String> {
        (!self.provider_errors.is_empty()).then(|| {
            self.provider_errors
                .iter()
                .map(|(provider, error)| format!("{provider}: {error}"))
                .collect::<Vec<_>>()
                .join("; ")
        })
    }

    /// Returns matching projects in the transitive dependency closure.
    ///
    /// Dependencies are returned in deterministic post-order, with upstream
    /// projects before the projects that depend on them. The predicate only
    /// controls which projects are returned; traversal continues through
    /// non-matching projects so they can connect matching projects farther
    /// upstream.
    pub fn matching_dependency_projects(
        &self,
        id: &ProjectId,
        mut matches: impl FnMut(&WorkspaceProject) -> bool,
    ) -> Result<Vec<&WorkspaceProject>> {
        if !self.projects.contains_key(id) {
            bail!("unknown workspace project {id:?}");
        }

        let mut visited = BTreeSet::new();
        let mut matching = Vec::new();
        self.collect_matching_dependencies(id, &mut visited, &mut matches, &mut matching);
        Ok(matching)
    }

    fn collect_matching_dependencies<'a>(
        &'a self,
        id: &ProjectId,
        visited: &mut BTreeSet<ProjectId>,
        matches: &mut impl FnMut(&WorkspaceProject) -> bool,
        matching: &mut Vec<&'a WorkspaceProject>,
    ) {
        let project = self
            .projects
            .get(id)
            .expect("workspace project graph is validated");
        for dependency_id in &project.dependencies {
            if !visited.insert(dependency_id.clone()) {
                continue;
            }
            self.collect_matching_dependencies(dependency_id, visited, matches, matching);
            let dependency = self
                .projects
                .get(dependency_id)
                .expect("workspace project graph is validated");
            if matches(dependency) {
                matching.push(dependency);
            }
        }
    }

    fn validate(&self) -> Result<()> {
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
        if let Some(path) = self.find_cycle() {
            return Err(WorkspaceProjectCycleError { path }.into());
        }
        Ok(())
    }

    fn find_cycle(&self) -> Option<Vec<ProjectId>> {
        let mut visited = BTreeSet::new();
        let mut active = BTreeMap::new();
        let mut path = Vec::new();
        for id in self.projects.keys() {
            if let Some(cycle) = self.find_cycle_from(id, &mut visited, &mut active, &mut path) {
                return Some(cycle);
            }
        }
        None
    }

    fn find_cycle_from(
        &self,
        id: &ProjectId,
        visited: &mut BTreeSet<ProjectId>,
        active: &mut BTreeMap<ProjectId, usize>,
        path: &mut Vec<ProjectId>,
    ) -> Option<Vec<ProjectId>> {
        if visited.contains(id) {
            return None;
        }
        if let Some(start) = active.get(id) {
            let mut cycle = path[*start..].to_vec();
            cycle.push(id.clone());
            return Some(cycle);
        }

        active.insert(id.clone(), path.len());
        path.push(id.clone());
        for dependency in &self.projects.get(id)?.dependencies {
            if let Some(cycle) = self.find_cycle_from(dependency, visited, active, path) {
                return Some(cycle);
            }
        }
        path.pop();
        active.remove(id);
        visited.insert(id.clone());
        None
    }
}

fn attach_provider_provenance(project: &mut WorkspaceProject, provider: &str) {
    project
        .provenance
        .provider
        .get_or_insert_with(|| provider.to_string());
    for dependency in &project.dependencies {
        project
            .dependency_provenance
            .entry(dependency.clone())
            .or_default()
            .provider
            .get_or_insert_with(|| provider.to_string());
    }
    for task in project.tasks.values_mut() {
        attach_task_provenance(task, provider);
    }
}

fn attach_task_provenance(task: &mut WorkspaceTask, provider: &str) {
    task.provenance
        .provider
        .get_or_insert_with(|| provider.to_string());
    task.provenance
        .source
        .get_or_insert_with(|| task.source.clone());
    let suggestion_source = task.suggestions.config_sources.first().cloned();
    let attach_suggestion = |provenance: &mut Option<WorkspaceProvenance>| {
        let provenance = provenance.get_or_insert_default();
        provenance
            .provider
            .get_or_insert_with(|| provider.to_string());
        if let Some(source) = &suggestion_source {
            provenance.source.get_or_insert_with(|| source.clone());
        }
    };
    if !task.suggestions.inputs.is_empty() {
        attach_suggestion(&mut task.suggestions.provenance.inputs);
    }
    if task.suggestions.outputs.is_some() {
        attach_suggestion(&mut task.suggestions.provenance.outputs);
    }
    if task.suggestions.cache.is_some() {
        attach_suggestion(&mut task.suggestions.provenance.cache);
    }
    if task.suggestions.depends.is_some() {
        attach_suggestion(&mut task.suggestions.provenance.depends);
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

    #[derive(Debug)]
    struct FailingTaskProvider;

    impl WorkspaceProvider for FailingTaskProvider {
        fn id(&self) -> &str {
            "test"
        }

        fn discover(&self, _workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
            let mut project = WorkspaceProject::new(ProjectId::new("test", "app").unwrap(), "old");
            project.tasks.insert(
                "build".to_string(),
                WorkspaceTask {
                    command: "old build".to_string(),
                    description: "old build".to_string(),
                    source: "old/manifest".into(),
                    provenance: WorkspaceProvenance::default(),
                    suggestions: WorkspaceTaskSuggestions::default(),
                },
            );
            Ok(vec![project])
        }

        fn discover_project_tasks(
            &self,
            _workspace_root: &Path,
            _project_root: &Path,
        ) -> Result<BTreeMap<String, WorkspaceTask>> {
            bail!("task discovery failed")
        }
    }

    #[derive(Debug)]
    struct FailingDiscoveryProvider;

    impl WorkspaceProvider for FailingDiscoveryProvider {
        fn id(&self) -> &str {
            "broken"
        }

        fn discover(&self, _workspace_root: &Path) -> Result<Vec<WorkspaceProject>> {
            bail!("broken workspace metadata")
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
    fn task_suggestions_map_to_inferred_task_configuration() {
        let suggestions = WorkspaceTaskSuggestions {
            inputs: vec!["src/**".to_string(), "package.json".to_string()],
            outputs: Some(vec!["dist/**".to_string()]),
            cache: Some(true),
            depends: Some(vec!["prepare".to_string(), "^build".to_string()]),
            config_sources: vec![PathBuf::from("turbo.json")],
            provenance: WorkspaceTaskSuggestionProvenance::default(),
        };
        let mut task = Task::default();

        suggestions.apply_before_defaults(&mut task);
        suggestions.apply_after_defaults(&mut task);

        assert_eq!(task.sources, vec!["src/**", "package.json"]);
        assert_eq!(
            task.outputs,
            TaskOutputs::Files(vec!["dist/**".to_string()])
        );
        assert!(task.cache.as_ref().is_some_and(|cache| cache.enabled));
        assert_eq!(
            task.depends
                .iter()
                .map(|dependency| dependency.task.as_str())
                .collect::<Vec<_>>(),
            vec!["prepare", "^build"]
        );
        assert_eq!(
            task.additional_config_sources,
            [PathBuf::from("turbo.json")]
        );
    }

    #[test]
    fn task_suggestions_distinguish_empty_values_from_no_suggestion() {
        let mut no_outputs = Task::default();
        let no_output_suggestions = WorkspaceTaskSuggestions {
            outputs: Some(Vec::new()),
            cache: Some(false),
            ..Default::default()
        };
        no_output_suggestions.apply_before_defaults(&mut no_outputs);
        no_outputs.merge_template(&super::super::TaskTemplate {
            outputs: TaskOutputs::Files(vec!["default".to_string()]),
            ..Default::default()
        });
        no_output_suggestions.apply_after_defaults(&mut no_outputs);
        assert_eq!(no_outputs.outputs, TaskOutputs::NoFiles);
        assert!(
            no_outputs
                .cache
                .as_ref()
                .is_some_and(|cache| !cache.enabled)
        );

        let mut unspecified = Task {
            sources: vec!["existing".to_string()],
            outputs: TaskOutputs::Files(vec!["existing".to_string()]),
            cache: Some(TaskCacheConfig {
                enabled: true,
                ..Default::default()
            }),
            depends: vec!["existing".to_string().into()],
            ..Default::default()
        };
        WorkspaceTaskSuggestions::default().apply_before_defaults(&mut unspecified);
        WorkspaceTaskSuggestions::default().apply_after_defaults(&mut unspecified);
        assert_eq!(unspecified.sources, vec!["existing"]);
        assert_eq!(
            unspecified.outputs,
            TaskOutputs::Files(vec!["existing".to_string()])
        );
        assert_eq!(unspecified.depends[0].task, "existing");
        assert!(
            unspecified
                .cache
                .as_ref()
                .is_some_and(|cache| cache.enabled)
        );

        let suggestions = WorkspaceTaskSuggestions {
            depends: Some(Vec::new()),
            ..Default::default()
        };
        let mut no_dependencies = Task::default();
        suggestions.apply_before_defaults(&mut no_dependencies);
        no_dependencies.merge_template(&super::super::TaskTemplate {
            depends: vec!["default".to_string().into()],
            ..Default::default()
        });
        suggestions.apply_after_defaults(&mut no_dependencies);
        assert!(no_dependencies.depends.is_empty());
    }

    #[test]
    fn cache_suggestion_preserves_default_cache_inputs() {
        let suggestions = WorkspaceTaskSuggestions {
            cache: Some(true),
            ..Default::default()
        };
        let mut task = Task::default();

        suggestions.apply_before_defaults(&mut task);
        task.merge_template(&super::super::TaskTemplate {
            cache: Some(TaskCacheConfig {
                env: vec!["NODE_ENV".to_string()],
                command_inputs: vec!["node --version".to_string()],
                ..Default::default()
            }),
            ..Default::default()
        });
        suggestions.apply_after_defaults(&mut task);

        assert_eq!(
            task.cache,
            Some(TaskCacheConfig {
                enabled: true,
                env: vec!["NODE_ENV".to_string()],
                command_inputs: vec!["node --version".to_string()],
            })
        );
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
        assert_eq!(projects[0].provenance.provider.as_deref(), Some("test"));
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
    fn lenient_discovery_isolates_provider_errors() {
        let working = TestProvider {
            id: "node",
            projects: vec![project("node", "app", "apps/app")],
        };

        assert!(
            WorkspaceProjectGraph::discover_all_with_overrides(
                &[&FailingDiscoveryProvider, &working],
                Path::new("/workspace"),
                &BTreeMap::new(),
            )
            .unwrap_err()
            .to_string()
            .contains("broken workspace metadata")
        );

        let graph = WorkspaceProjectGraph::discover_all_with_overrides_lenient(
            &[&FailingDiscoveryProvider, &working],
            Path::new("/workspace"),
            &BTreeMap::new(),
        )
        .unwrap();

        assert!(graph.get(&ProjectId::new("node", "app").unwrap()).is_some());
        assert_eq!(
            graph.provider_discovery_error().as_deref(),
            Some("broken: broken workspace metadata")
        );
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
        app.tasks.insert(
            "build".to_string(),
            WorkspaceTask {
                command: "npm run build --".to_string(),
                description: "vite build".to_string(),
                source: "apps/app/package.json".into(),
                provenance: WorkspaceProvenance::default(),
                suggestions: WorkspaceTaskSuggestions::default(),
            },
        );
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
        assert!(app.tasks.is_empty());
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
    fn depends_add_preserves_existing_provider_provenance() {
        let dependency_id = ProjectId::new("node", "lib").unwrap();
        let mut app = project("node", "app", "app");
        app.dependencies.insert(dependency_id.clone());
        let provider = TestProvider {
            id: "node",
            projects: vec![app, project("node", "lib", "lib")],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                depends_add: BTreeSet::from(["node:lib".to_string()]),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert_eq!(
            app.dependency_provenance[&dependency_id]
                .provider
                .as_deref(),
            Some("node")
        );
    }

    #[test]
    fn root_overrides_clear_stale_sources_and_retain_edge_provider() {
        let dependency_id = ProjectId::new("node", "lib").unwrap();
        let mut app = project("node", "app", "packages/app");
        app.dependencies.insert(dependency_id.clone());
        app.provenance.source = Some("packages/app/package.json".into());
        app.dependency_provenance.insert(
            dependency_id.clone(),
            WorkspaceProvenance {
                source: Some("packages/app/package.json".into()),
                ..Default::default()
            },
        );
        let provider = TestProvider {
            id: "node",
            projects: vec![app, project("node", "lib", "packages/lib")],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                root: Some("overrides/app".into()),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert_eq!(app.provenance, WorkspaceProvenance::default());
        assert_eq!(
            app.dependency_provenance[&dependency_id]
                .provider
                .as_deref(),
            Some("node")
        );
        assert_eq!(app.dependency_provenance[&dependency_id].source, None);
    }

    #[test]
    fn equal_root_overrides_attribute_project_to_configuration() {
        let dependency_id = ProjectId::new("node", "lib").unwrap();
        let mut app = project("node", "app", "packages/app");
        app.dependencies.insert(dependency_id.clone());
        let provider = TestProvider {
            id: "node",
            projects: vec![app, project("node", "lib", "packages/lib")],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                root: Some("packages/app".into()),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert_eq!(app.provenance, WorkspaceProvenance::default());
        assert_eq!(
            app.dependency_provenance[&dependency_id]
                .provider
                .as_deref(),
            Some("node")
        );
    }

    #[test]
    fn metadata_overrides_clear_provider_provenance() {
        let provider = TestProvider {
            id: "node",
            projects: vec![project("node", "app", "packages/app")],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                metadata: Some(BTreeMap::from([("kind".to_string(), "web".to_string())])),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap();
        let app = graph.get(&ProjectId::new("node", "app").unwrap()).unwrap();

        assert_eq!(app.provenance, WorkspaceProvenance::default());
    }

    #[test]
    fn override_task_inference_errors_are_isolated() {
        let overrides = BTreeMap::from([(
            "test:app".to_string(),
            WorkspaceProjectOverride {
                root: Some("new".into()),
                ..Default::default()
            },
        )]);

        let graph = WorkspaceProjectGraph::discover_all_with_overrides(
            &[&FailingTaskProvider],
            Path::new("/workspace"),
            &overrides,
        )
        .unwrap();
        let project = graph.get(&ProjectId::new("test", "app").unwrap()).unwrap();

        assert_eq!(project.root, Path::new("new"));
        assert!(project.tasks.is_empty());
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

    #[test]
    fn configured_discovery_can_repair_dangling_inferred_edges() {
        let missing = ProjectId::new("node", "missing").unwrap();
        let mut app = project("node", "app", "app");
        app.dependencies.insert(missing);
        let provider = TestProvider {
            id: "node",
            projects: vec![app],
        };
        let overrides = BTreeMap::from([(
            "node:app".to_string(),
            WorkspaceProjectOverride {
                depends_remove: BTreeSet::from(["node:missing".to_string()]),
                ..Default::default()
            },
        )]);

        assert!(
            WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
                .unwrap_err()
                .to_string()
                .contains("depends on unknown project")
        );
        let graph = WorkspaceProjectGraph::discover_all_with_overrides(
            &[&provider],
            Path::new("/workspace"),
            &overrides,
        )
        .unwrap();

        assert!(
            graph
                .get(&ProjectId::new("node", "app").unwrap())
                .unwrap()
                .dependencies
                .is_empty()
        );
    }

    #[test]
    fn discovery_reports_self_cycles_with_override_guidance() {
        let app_id = ProjectId::new("node", "app").unwrap();
        let mut app = project("node", "app", "app");
        app.dependencies.insert(app_id.clone());
        let provider = TestProvider {
            id: "node",
            projects: vec![app],
        };

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap_err();
        let cycle = err.downcast_ref::<WorkspaceProjectCycleError>().unwrap();

        assert_eq!(cycle.path(), &[app_id.clone(), app_id]);
        assert_eq!(
            err.to_string(),
            "workspace project dependency cycle detected: node:app -> node:app; adjust [monorepo.projects.\"node:app\"] depends, depends_add, or depends_remove to break the cycle"
        );
    }

    #[test]
    fn cross_provider_cycle_diagnostics_are_order_independent() {
        let cargo_id = ProjectId::new("cargo", "core").unwrap();
        let node_id = ProjectId::new("node", "app").unwrap();
        let mut core = project("cargo", "core", "core");
        core.dependencies.insert(node_id.clone());
        let cargo = TestProvider {
            id: "cargo",
            projects: vec![core],
        };
        let mut app = project("node", "app", "app");
        app.dependencies.insert(cargo_id.clone());
        let node = TestProvider {
            id: "node",
            projects: vec![app],
        };

        let forward =
            WorkspaceProjectGraph::discover_all(&[&cargo, &node], Path::new("/workspace"))
                .unwrap_err();
        let reverse =
            WorkspaceProjectGraph::discover_all(&[&node, &cargo], Path::new("/workspace"))
                .unwrap_err();

        assert_eq!(forward.to_string(), reverse.to_string());
        assert_eq!(
            forward
                .downcast_ref::<WorkspaceProjectCycleError>()
                .unwrap()
                .path(),
            &[cargo_id.clone(), node_id, cargo_id]
        );
    }

    #[test]
    fn configured_discovery_can_repair_inferred_cycles() {
        let app_id = ProjectId::new("node", "app").unwrap();
        let lib_id = ProjectId::new("node", "lib").unwrap();
        let mut app = project("node", "app", "app");
        app.dependencies.insert(lib_id.clone());
        let mut lib = project("node", "lib", "lib");
        lib.dependencies.insert(app_id.clone());
        let provider = TestProvider {
            id: "node",
            projects: vec![app, lib],
        };
        let overrides = BTreeMap::from([(
            "node:lib".to_string(),
            WorkspaceProjectOverride {
                depends_remove: BTreeSet::from(["node:app".to_string()]),
                ..Default::default()
            },
        )]);

        assert!(
            WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
                .unwrap_err()
                .downcast_ref::<WorkspaceProjectCycleError>()
                .is_some()
        );
        let graph = WorkspaceProjectGraph::discover_all_with_overrides(
            &[&provider],
            Path::new("/workspace"),
            &overrides,
        )
        .unwrap();

        assert!(graph.get(&lib_id).unwrap().dependencies.is_empty());
        assert_eq!(
            graph.get(&app_id).unwrap().dependencies,
            BTreeSet::from([lib_id])
        );
    }

    #[test]
    fn overrides_reject_new_cycles() {
        let provider = TestProvider {
            id: "node",
            projects: vec![project("node", "app", "app"), project("node", "lib", "lib")],
        };
        let overrides = BTreeMap::from([
            (
                "node:app".to_string(),
                WorkspaceProjectOverride {
                    depends_add: BTreeSet::from(["node:lib".to_string()]),
                    ..Default::default()
                },
            ),
            (
                "node:lib".to_string(),
                WorkspaceProjectOverride {
                    depends_add: BTreeSet::from(["node:app".to_string()]),
                    ..Default::default()
                },
            ),
        ]);

        let err = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace"))
            .unwrap()
            .with_overrides(&overrides)
            .unwrap_err();

        assert!(err.downcast_ref::<WorkspaceProjectCycleError>().is_some());
    }

    #[test]
    fn matching_dependencies_traverse_projects_without_the_requested_task() {
        let core_id = ProjectId::new("node", "core").unwrap();
        let bridge_id = ProjectId::new("node", "bridge").unwrap();
        let app_id = ProjectId::new("node", "app").unwrap();
        let core = project("node", "core", "core");
        let mut bridge = project("node", "bridge", "bridge");
        bridge.dependencies.insert(core_id.clone());
        let mut app = project("node", "app", "app");
        app.dependencies.insert(bridge_id);
        let provider = TestProvider {
            id: "node",
            projects: vec![app, bridge, core],
        };
        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();

        let projects = graph
            .matching_dependency_projects(&app_id, |project| project.id == core_id)
            .unwrap();

        assert_eq!(
            projects
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            vec!["node:core"]
        );
    }

    #[test]
    fn matching_dependencies_are_deduplicated_in_deterministic_post_order() {
        let core_id = ProjectId::new("node", "core").unwrap();
        let left_id = ProjectId::new("node", "left").unwrap();
        let right_id = ProjectId::new("node", "right").unwrap();
        let app_id = ProjectId::new("node", "app").unwrap();
        let core = project("node", "core", "core");
        let mut left = project("node", "left", "left");
        left.dependencies.insert(core_id.clone());
        let mut right = project("node", "right", "right");
        right.dependencies.insert(core_id);
        let mut app = project("node", "app", "app");
        app.dependencies = BTreeSet::from([right_id, left_id]);
        let provider = TestProvider {
            id: "node",
            projects: vec![right, app, core, left],
        };
        let graph = WorkspaceProjectGraph::discover(&provider, Path::new("/workspace")).unwrap();

        let projects = graph
            .matching_dependency_projects(&app_id, |_| true)
            .unwrap();

        assert_eq!(
            projects
                .iter()
                .map(|project| project.id.as_str())
                .collect::<Vec<_>>(),
            vec!["node:core", "node:left", "node:right"]
        );
    }

    #[test]
    fn matching_dependencies_reject_an_unknown_starting_project() {
        let graph = WorkspaceProjectGraph::default();
        let missing = ProjectId::new("node", "missing").unwrap();

        let err = graph
            .matching_dependency_projects(&missing, |_| true)
            .unwrap_err();

        assert_eq!(
            err.to_string(),
            "unknown workspace project ProjectId(\"node:missing\")"
        );
    }
}
