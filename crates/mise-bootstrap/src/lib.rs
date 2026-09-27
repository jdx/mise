//! Resource identities, status, and dependency ordering for mise bootstrap.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::prelude::BASE64_URL_SAFE_NO_PAD;
use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::{Serialize, Serializer};

/// Desired state of a local bootstrap account.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    #[default]
    Present,
    Absent,
}

/// Desired state of a managed bootstrap file or directory.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ManagedState {
    #[default]
    Present,
    Absent,
}

/// When to apply a managed file relative to package installation.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagedFilePhase {
    PrePackages,
    #[default]
    PostPackages,
}

impl ManagedFilePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrePackages => "pre-packages",
            Self::PostPackages => "post-packages",
        }
    }
}

/// Stable identity for one declarative bootstrap resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ResourceId {
    pub kind: String,
    pub name: String,
}

/// Where a declarative resource came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResourceOrigin {
    #[serde(serialize_with = "serialize_path")]
    pub config: PathBuf,
    #[serde(serialize_with = "serialize_path")]
    pub config_root: PathBuf,
    pub environment: Vec<String>,
    #[serde(serialize_with = "serialize_optional_path")]
    pub source: Option<PathBuf>,
}

impl ResourceOrigin {
    /// Formats the declaration and source paths for a sibling-resource conflict.
    pub fn conflict_description(&self) -> String {
        let mut description = format!("config: {}", self.config.display());
        if let Some(source) = &self.source {
            description.push_str(&format!("\n    source: {}", source.display()));
        }
        description
    }
}

const ENCODED_PATH_PREFIX: &str = "mise:path-";

pub fn serialize_path<S>(path: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&path_json_string(path))
}

fn serialize_optional_path<S>(path: &Option<PathBuf>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    path.as_deref().map(path_json_string).serialize(serializer)
}

fn path_json_string(path: &Path) -> Cow<'_, str> {
    if let Some(path) = path.to_str()
        && !path.starts_with(ENCODED_PATH_PREFIX)
    {
        return Cow::Borrowed(path);
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        let encoded = BASE64_URL_SAFE_NO_PAD.encode(path.as_os_str().as_bytes());
        Cow::Owned(format!("{ENCODED_PATH_PREFIX}bytes:{encoded}"))
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        let bytes = path
            .as_os_str()
            .encode_wide()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let encoded = BASE64_URL_SAFE_NO_PAD.encode(bytes);
        Cow::Owned(format!("{ENCODED_PATH_PREFIX}utf16:{encoded}"))
    }
}

impl ResourceId {
    pub fn new(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.name)
    }
}

/// The operation needed to converge a resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAction {
    Create,
    Update,
    Remove,
    Noop,
    Unknown,
}

impl fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Remove => "remove",
            Self::Noop => "unchanged",
            Self::Unknown => "unknown",
        })
    }
}

/// A secret-safe description of one resource's current and desired state.
#[derive(Clone, Debug, Serialize)]
pub struct ResourcePlan {
    pub id: ResourceId,
    pub current: String,
    pub desired: String,
    pub action: ResourceAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<ResourceOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ResourceId>,
    /// Execution order only; these resources do not affect change prediction.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order_after: Vec<ResourceId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<ManagedFilePhase>,
}

impl ResourcePlan {
    pub fn new(
        id: ResourceId,
        current: impl Into<String>,
        desired: impl Into<String>,
        action: ResourceAction,
    ) -> Self {
        Self {
            id,
            current: current.into(),
            desired: desired.into(),
            action,
            origin: None,
            depends_on: vec![],
            order_after: vec![],
            phase: None,
        }
    }

    pub fn with_origin(mut self, origin: ResourceOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    pub fn with_file_phase(mut self, phase: ManagedFilePhase) -> Self {
        self.phase = Some(phase);
        self
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PlanSummary {
    pub create: usize,
    pub update: usize,
    pub remove: usize,
    pub unchanged: usize,
    pub unknown: usize,
}

impl PlanSummary {
    fn add(&mut self, action: ResourceAction) {
        match action {
            ResourceAction::Create => self.create += 1,
            ResourceAction::Update => self.update += 1,
            ResourceAction::Remove => self.remove += 1,
            ResourceAction::Noop => self.unchanged += 1,
            ResourceAction::Unknown => self.unknown += 1,
        }
    }

    pub fn has_changes(self) -> bool {
        self.create + self.update + self.remove > 0
    }

    pub fn has_unknown(self) -> bool {
        self.unknown > 0
    }
}

#[derive(Serialize)]
pub struct BootstrapPlanOutput<'a> {
    pub resources: Vec<&'a ResourcePlan>,
    pub summary: PlanSummary,
}

/// A validated resource graph in declaration order.
#[derive(Default)]
pub struct BootstrapPlan {
    resources: IndexMap<ResourceId, ResourcePlan>,
}

impl BootstrapPlan {
    pub fn contains(&self, id: &ResourceId) -> bool {
        self.resources.contains_key(id)
    }

    pub fn get(&self, id: &ResourceId) -> Option<&ResourcePlan> {
        self.resources.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &ResourceId> {
        self.resources.keys()
    }

    pub fn values(&self) -> impl Iterator<Item = &ResourcePlan> {
        self.resources.values()
    }

    pub fn insert(&mut self, resource: ResourcePlan) -> Result<()> {
        if self.resources.contains_key(&resource.id) {
            bail!(
                "bootstrap resource '{}' is declared more than once",
                resource.id
            );
        }
        self.resources.insert(resource.id.clone(), resource);
        Ok(())
    }

    pub fn add_dependency(&mut self, resource: &ResourceId, dependency: ResourceId) -> Result<()> {
        let Some(resource) = self.resources.get_mut(resource) else {
            bail!("cannot add dependency to missing bootstrap resource '{resource}'");
        };
        if !resource.depends_on.contains(&dependency) {
            resource.depends_on.push(dependency);
        }
        Ok(())
    }

    pub fn output(&self) -> Result<BootstrapPlanOutput<'_>> {
        let resources = self.ordered()?;
        let mut summary = PlanSummary::default();
        for resource in &resources {
            summary.add(resource.action);
        }
        Ok(BootstrapPlanOutput { resources, summary })
    }

    pub fn add_ordering(&mut self, resource: &ResourceId, predecessor: ResourceId) -> Result<()> {
        let Some(resource) = self.resources.get_mut(resource) else {
            bail!("cannot order missing bootstrap resource '{resource}'");
        };
        if !resource.order_after.contains(&predecessor) {
            resource.order_after.push(predecessor);
        }
        Ok(())
    }

    /// Order managed file phases around built-in and plugin packages.
    pub fn add_file_phase_ordering(&mut self, packages: &[ResourceId]) -> Result<()> {
        let early = self
            .values()
            .filter(|resource| resource.phase == Some(ManagedFilePhase::PrePackages))
            .map(|resource| resource.id.clone())
            .collect::<Vec<_>>();
        let late = self
            .values()
            .filter(|resource| resource.phase == Some(ManagedFilePhase::PostPackages))
            .map(|resource| resource.id.clone())
            .collect::<Vec<_>>();
        for package in packages {
            for file in &early {
                self.add_ordering(package, file.clone())?;
            }
        }
        for file in &late {
            for dependency in packages.iter().chain(&early) {
                self.add_ordering(file, dependency.clone())?;
            }
        }
        // Installed and pending plugin managers both run after the file phases.
        let plugin_packages = self
            .ids()
            .filter(|id| id.kind == "package" && !packages.contains(id))
            .cloned()
            .collect::<Vec<_>>();
        for package in plugin_packages {
            for predecessor in early.iter().chain(&late).chain(packages) {
                self.add_ordering(&package, predecessor.clone())?;
            }
        }
        Ok(())
    }

    /// Depend on declared owners and groups, rejecting principals marked absent.
    /// Call this for managed resources whose desired state is present.
    pub fn add_account_dependencies(
        &mut self,
        resource: &ResourceId,
        owner: Option<&str>,
        group: Option<&str>,
        user_states: &HashMap<String, AccountState>,
        group_states: &HashMap<String, AccountState>,
    ) -> Result<()> {
        if let Some(owner) = owner {
            match user_states.get(owner) {
                Some(AccountState::Present) => {
                    self.add_dependency(resource, ResourceId::new("user", owner))?;
                }
                Some(AccountState::Absent) => bail!(
                    "bootstrap resource '{resource}' requires owner '{owner}', but that user is absent"
                ),
                None => {}
            }
        }
        if let Some(group) = group {
            match group_states.get(group) {
                Some(AccountState::Present) => {
                    self.add_dependency(resource, ResourceId::new("group", group))?;
                }
                Some(AccountState::Absent) => bail!(
                    "bootstrap resource '{resource}' requires group '{group}', but that group is absent"
                ),
                None => {}
            }
        }
        Ok(())
    }

    /// Order a managed path against its nearest declared parent directory.
    pub fn add_managed_parent_dependency(
        &mut self,
        child: &ResourceId,
        child_path: &Path,
        child_state: ManagedState,
        parent_path: &Path,
        parent_state: ManagedState,
    ) -> Result<()> {
        let parent = ResourceId::new("directory", parent_path.to_string_lossy());
        match (child_state, parent_state) {
            (ManagedState::Present, ManagedState::Present) => {
                self.add_dependency(child, parent)?;
            }
            (ManagedState::Absent, ManagedState::Absent) => {
                self.add_dependency(&parent, child.clone())?;
            }
            (ManagedState::Present, ManagedState::Absent) => {
                bail!(
                    "{} '{}' cannot be present while managed parent '{}' is absent",
                    child.kind,
                    child_path.display(),
                    parent_path.display()
                );
            }
            (ManagedState::Absent, ManagedState::Present) => {}
        }
        Ok(())
    }

    fn ordered(&self) -> Result<Vec<&ResourcePlan>> {
        let mut incoming = self
            .resources
            .keys()
            .cloned()
            .map(|id| (id, 0_usize))
            .collect::<IndexMap<_, _>>();
        let mut outgoing: HashMap<ResourceId, Vec<ResourceId>> = HashMap::new();

        for resource in self.resources.values() {
            for dependency in resource.depends_on.iter().chain(&resource.order_after) {
                let Some(count) = incoming.get_mut(&resource.id) else {
                    unreachable!("every resource was added to incoming")
                };
                if !self.resources.contains_key(dependency) {
                    bail!(
                        "bootstrap resource '{}' depends on missing resource '{}'",
                        resource.id,
                        dependency
                    );
                }
                *count += 1;
                outgoing
                    .entry(dependency.clone())
                    .or_default()
                    .push(resource.id.clone());
            }
        }

        let mut ready = incoming
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
            .collect::<VecDeque<_>>();
        let mut ordered = Vec::with_capacity(self.resources.len());
        while let Some(id) = ready.pop_front() {
            ordered.push(&self.resources[&id]);
            if let Some(dependents) = outgoing.get(&id) {
                for dependent in dependents {
                    let count = incoming
                        .get_mut(dependent)
                        .expect("dependent resource is present");
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(dependent.clone());
                    }
                }
            }
        }

        if ordered.len() != self.resources.len() {
            let cycle = incoming
                .into_iter()
                .filter_map(|(id, count)| (count > 0).then_some(id.to_string()))
                .collect::<Vec<_>>()
                .join(", ");
            bail!("bootstrap resource dependency cycle: {cycle}");
        }
        Ok(ordered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_resources_depend_on_present_declared_accounts() {
        let mut plan = BootstrapPlan::default();
        let file = ResourceId::new("file", "/etc/service.conf");
        let user = ResourceId::new("user", "service");
        let group = ResourceId::new("group", "services");
        for id in [&file, &user, &group] {
            plan.insert(ResourcePlan::new(
                id.clone(),
                "missing",
                "present",
                ResourceAction::Create,
            ))
            .unwrap();
        }
        let users = HashMap::from([("service".to_string(), AccountState::Present)]);
        let groups = HashMap::from([("services".to_string(), AccountState::Present)]);

        plan.add_account_dependencies(&file, Some("service"), Some("services"), &users, &groups)
            .unwrap();

        assert_eq!(plan.get(&file).unwrap().depends_on, [user, group]);
        assert_eq!(plan.output().unwrap().resources.last().unwrap().id, file);
    }

    #[test]
    fn managed_resources_reject_absent_declared_accounts() {
        let mut plan = BootstrapPlan::default();
        let file = ResourceId::new("file", "/etc/service.conf");
        plan.insert(ResourcePlan::new(
            file.clone(),
            "missing",
            "present",
            ResourceAction::Create,
        ))
        .unwrap();
        let absent = HashMap::from([("service".to_string(), AccountState::Absent)]);
        let missing = HashMap::new();

        let error = plan
            .add_account_dependencies(&file, Some("service"), None, &absent, &missing)
            .unwrap_err();
        assert!(error.to_string().contains("owner 'service'"));
        let error = plan
            .add_account_dependencies(&file, None, Some("service"), &missing, &absent)
            .unwrap_err();
        assert!(error.to_string().contains("group 'service'"));
        plan.add_account_dependencies(&file, Some("external"), None, &missing, &missing)
            .unwrap();
        assert!(plan.get(&file).unwrap().depends_on.is_empty());
    }

    #[test]
    fn managed_paths_order_creation_and_removal_by_parent() {
        let path = Path::new("/etc/vendor/config");
        let parent_path = Path::new("/etc/vendor");
        let child = ResourceId::new("file", path.to_string_lossy());
        let parent = ResourceId::new("directory", parent_path.to_string_lossy());

        for (state, expected_dependency) in [
            (ManagedState::Present, child.clone()),
            (ManagedState::Absent, parent.clone()),
        ] {
            let mut plan = BootstrapPlan::default();
            for id in [&child, &parent] {
                plan.insert(ResourcePlan::new(
                    id.clone(),
                    "current",
                    "desired",
                    ResourceAction::Update,
                ))
                .unwrap();
            }
            plan.add_managed_parent_dependency(&child, path, state, parent_path, state)
                .unwrap();
            assert_eq!(plan.get(&expected_dependency).unwrap().depends_on.len(), 1);
        }

        let mut plan = BootstrapPlan::default();
        let error = plan
            .add_managed_parent_dependency(
                &child,
                path,
                ManagedState::Present,
                parent_path,
                ManagedState::Absent,
            )
            .unwrap_err();
        assert!(error.to_string().contains("file '/etc/vendor/config'"));
    }

    #[test]
    fn file_phases_order_resources_around_packages() {
        let mut plan = BootstrapPlan::default();
        let late = ResourceId::new("file", "/etc/service.conf");
        let package = ResourceId::new("package", "apt:vendor");
        let early = ResourceId::new("file", "/etc/apt/sources.list.d/vendor.sources");
        let plugin = ResourceId::new("package", "custom:vendor");
        for (id, phase) in [
            (plugin.clone(), None),
            (late.clone(), Some(ManagedFilePhase::PostPackages)),
            (package.clone(), None),
            (early.clone(), Some(ManagedFilePhase::PrePackages)),
        ] {
            let mut resource = ResourcePlan::new(id, "missing", "present", ResourceAction::Create);
            resource.phase = phase;
            plan.insert(resource).unwrap();
        }
        plan.add_file_phase_ordering(std::slice::from_ref(&package))
            .unwrap();
        let output = plan.output().unwrap();
        assert_eq!(
            output
                .resources
                .iter()
                .map(|resource| &resource.id)
                .collect::<Vec<_>>(),
            [&early, &package, &late, &plugin]
        );
        assert!(
            output
                .resources
                .iter()
                .all(|resource| resource.depends_on.is_empty())
        );
    }

    #[cfg(unix)]
    #[test]
    fn resource_origin_serializes_non_utf8_paths_losslessly() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let invalid_path = PathBuf::from(OsString::from_vec(b"/tmp/invalid-\xff".to_vec()));
        let other_invalid_path = PathBuf::from(OsString::from_vec(b"/tmp/invalid-\xfe".to_vec()));
        let origin = ResourceOrigin {
            config: invalid_path.clone(),
            config_root: invalid_path.clone(),
            environment: vec![],
            source: Some(invalid_path.clone()),
        };

        let value = serde_json::to_value(origin).unwrap();
        let encoded = value["config"].as_str().unwrap();
        assert!(encoded.starts_with("mise:path-bytes:"));
        assert_eq!(value["config_root"], encoded);
        assert_eq!(value["source"], encoded);
        assert_ne!(encoded, path_json_string(&other_invalid_path));
    }

    fn resource(name: &str) -> ResourcePlan {
        ResourcePlan::new(
            ResourceId::new("test", name),
            "missing",
            "present",
            ResourceAction::Create,
        )
    }

    fn depends_on(
        mut resource: ResourcePlan,
        dependencies: impl IntoIterator<Item = ResourceId>,
    ) -> ResourcePlan {
        resource.depends_on.extend(dependencies);
        resource
    }

    #[test]
    fn orders_dependencies_before_dependents() {
        let mut plan = BootstrapPlan::default();
        plan.insert(depends_on(
            resource("service"),
            [ResourceId::new("test", "file")],
        ))
        .unwrap();
        plan.insert(resource("file")).unwrap();

        let output = plan.output().unwrap();
        assert_eq!(output.resources[0].id.name, "file");
        assert_eq!(output.resources[1].id.name, "service");
    }

    #[test]
    fn rejects_duplicate_resources() {
        let mut plan = BootstrapPlan::default();
        plan.insert(resource("file")).unwrap();
        let error = plan.insert(resource("file")).unwrap_err();
        assert!(error.to_string().contains("declared more than once"));
    }

    #[test]
    fn rejects_missing_dependencies() {
        let mut plan = BootstrapPlan::default();
        plan.insert(depends_on(
            resource("service"),
            [ResourceId::new("test", "file")],
        ))
        .unwrap();
        let error = plan.output().err().unwrap();
        assert!(error.to_string().contains("depends on missing resource"));
    }

    #[test]
    fn rejects_dependency_cycles() {
        let mut plan = BootstrapPlan::default();
        plan.insert(depends_on(resource("a"), [ResourceId::new("test", "b")]))
            .unwrap();
        plan.insert(depends_on(resource("b"), [ResourceId::new("test", "a")]))
            .unwrap();
        let error = plan.output().err().unwrap();
        assert!(error.to_string().contains("dependency cycle"));
    }

    #[test]
    fn summarizes_resource_actions() {
        let mut plan = BootstrapPlan::default();
        for (name, action) in [
            ("create", ResourceAction::Create),
            ("update", ResourceAction::Update),
            ("remove", ResourceAction::Remove),
            ("noop", ResourceAction::Noop),
            ("unknown", ResourceAction::Unknown),
        ] {
            plan.insert(ResourcePlan::new(
                ResourceId::new("test", name),
                "current",
                "desired",
                action,
            ))
            .unwrap();
        }
        assert_eq!(
            plan.output().unwrap().summary,
            PlanSummary {
                create: 1,
                update: 1,
                remove: 1,
                unchanged: 1,
                unknown: 1,
            }
        );
    }
}
