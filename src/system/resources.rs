use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::path::PathBuf;

use eyre::{Result, bail};
use indexmap::{IndexMap, IndexSet};
use serde::Serialize;

use crate::config::Config;
use crate::system::packages::{PackageRequest, PackageState};

/// Stable identity for one declarative bootstrap resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ResourceId {
    pub kind: String,
    pub name: String,
}

/// Where a declarative resource came from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResourceOrigin {
    pub config: PathBuf,
    pub config_root: PathBuf,
    pub environment: Vec<String>,
    pub source: Option<PathBuf>,
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
        }
    }

    pub fn with_origin(mut self, origin: ResourceOrigin) -> Self {
        self.origin = Some(origin);
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

    fn ordered(&self) -> Result<Vec<&ResourcePlan>> {
        let mut incoming = self
            .resources
            .keys()
            .cloned()
            .map(|id| (id, 0_usize))
            .collect::<IndexMap<_, _>>();
        let mut outgoing: HashMap<ResourceId, Vec<ResourceId>> = HashMap::new();

        for resource in self.resources.values() {
            for dependency in &resource.depends_on {
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

/// Build the resource plan currently supported by the provisioning engine.
/// Other bootstrap sections will move into this graph as resource adapters land.
pub async fn plan(
    config: &Config,
    secrets: &super::secrets::SecretValues,
) -> Result<BootstrapPlan> {
    let mut plan = BootstrapPlan::default();
    let accounts = super::accounts::prepare_requests_from_config(config)?;
    let group_states = accounts
        .groups
        .iter()
        .map(|group| (group.name.clone(), group.state))
        .collect::<HashMap<_, _>>();
    let user_states = accounts
        .users
        .iter()
        .map(|user| (user.name.clone(), user.state))
        .collect::<HashMap<_, _>>();
    for group in &accounts.groups {
        plan.insert(group.plan())?;
    }
    for user in &accounts.users {
        plan.insert(user.plan())?;
        if user.state == super::accounts::AccountState::Present {
            for group in user
                .group
                .iter()
                .chain(user.groups.iter().flat_map(|groups| groups.iter()))
            {
                if group_states.get(group) == Some(&super::accounts::AccountState::Present) {
                    plan.add_dependency(
                        &ResourceId::new("user", &user.name),
                        ResourceId::new("group", group),
                    )?;
                }
            }
        }
    }
    for group in accounts
        .groups
        .iter()
        .filter(|group| group.state == super::accounts::AccountState::Absent)
    {
        for user in accounts.users.iter().filter(|user| {
            user.state == super::accounts::AccountState::Absent
                || user.current_primary_group() == Some(group.name.as_str())
        }) {
            plan.add_dependency(
                &ResourceId::new("group", &group.name),
                ResourceId::new("user", &user.name),
            )?;
        }
    }
    for manager_packages in super::packages_from_config(config) {
        let manager = manager_packages.manager;
        let manager_name = manager.name().to_string();
        let unavailable = if manager_packages.disabled {
            Some("excluded by system_packages.managers".to_string())
        } else {
            manager.unavailable_reason_async().await
        };

        if let Some(reason) = unavailable {
            for request in manager_packages.requests {
                plan.insert(ResourcePlan::new(
                    ResourceId::new("package", format!("{manager_name}:{}", request.name)),
                    format!("unavailable ({reason})"),
                    desired_package(&request),
                    ResourceAction::Unknown,
                ))?;
            }
            continue;
        }

        let supports_version_pins = manager.supports_version_pins();
        for status in manager.installed(&manager_packages.requests).await? {
            let id = ResourceId::new("package", format!("{manager_name}:{}", status.request.name));
            let desired = desired_package(&status.request);
            let (current, action) =
                package_resource_state(status.state, &status.request, supports_version_pins);
            plan.insert(ResourcePlan::new(id, current, desired, action))?;
        }
    }
    for (manager_name, requests) in
        super::pending_plugin_packages_from_config_including_disabled(config)
    {
        let reason = if super::package_manager_is_enabled(&manager_name) {
            "package plugin is not installed"
        } else {
            "excluded by system_packages.managers"
        };
        for request in requests {
            plan.insert(ResourcePlan::new(
                ResourceId::new("package", format!("{manager_name}:{}", request.name)),
                format!("unavailable ({reason})"),
                desired_package(&request),
                ResourceAction::Unknown,
            ))?;
        }
    }
    let (files, directories, unavailable_files) =
        super::managed_files::status_requests_from_config(config, secrets)?;
    super::managed_files::validate_principals(
        &files,
        &directories,
        cfg!(target_os = "linux").then_some(&accounts),
        cfg!(target_os = "linux"),
    )?;
    let services = super::services::status_requests_from_config(config)?;
    super::services::validate_notifications(&files, &directories, &services)?;
    let notified_services = super::managed_files::pending_notifications(&files, &directories)?;
    let directory_states = directories
        .iter()
        .map(|directory| (directory.path.clone(), directory.state))
        .collect::<std::collections::HashMap<_, _>>();
    for directory in &directories {
        let resource = directory.plan()?;
        plan.insert(resource)?;
        add_account_dependencies(
            &mut plan,
            &ResourceId::new("directory", directory.path.to_string_lossy()),
            directory.state,
            directory.owner.as_deref(),
            directory.group.as_deref(),
            &user_states,
            &group_states,
        )?;
    }
    for directory in &directories {
        let Some((parent, parent_state)) = directory
            .path
            .ancestors()
            .skip(1)
            .find_map(|parent| directory_states.get_key_value(parent))
        else {
            continue;
        };
        let id = ResourceId::new("directory", directory.path.to_string_lossy());
        let parent_id = ResourceId::new("directory", parent.to_string_lossy());
        match (directory.state, parent_state) {
            (
                super::managed_files::ManagedState::Present,
                super::managed_files::ManagedState::Present,
            ) => {
                plan.add_dependency(&id, parent_id)?;
            }
            (
                super::managed_files::ManagedState::Absent,
                super::managed_files::ManagedState::Absent,
            ) => {
                plan.add_dependency(&parent_id, id)?;
            }
            (
                super::managed_files::ManagedState::Present,
                super::managed_files::ManagedState::Absent,
            ) => {
                bail!(
                    "directory '{}' cannot be present while managed parent '{}' is absent",
                    directory.path.display(),
                    parent.display()
                );
            }
            (
                super::managed_files::ManagedState::Absent,
                super::managed_files::ManagedState::Present,
            ) => {}
        }
    }
    for file in files {
        let resource = file.plan()?;
        let id = resource.id.clone();
        plan.insert(resource)?;
        add_account_dependencies(
            &mut plan,
            &id,
            file.state,
            file.owner.as_deref(),
            file.group.as_deref(),
            &user_states,
            &group_states,
        )?;
        if let Some((parent, parent_state)) = file
            .path
            .ancestors()
            .skip(1)
            .find_map(|parent| directory_states.get_key_value(parent))
        {
            let parent_id = ResourceId::new("directory", parent.to_string_lossy());
            match (file.state, parent_state) {
                (
                    super::managed_files::ManagedState::Present,
                    super::managed_files::ManagedState::Present,
                ) => {
                    plan.add_dependency(&id, parent_id)?;
                }
                (
                    super::managed_files::ManagedState::Absent,
                    super::managed_files::ManagedState::Absent,
                ) => {
                    plan.add_dependency(&parent_id, id)?;
                }
                (
                    super::managed_files::ManagedState::Present,
                    super::managed_files::ManagedState::Absent,
                ) => {
                    bail!(
                        "file '{}' cannot be present while managed parent '{}' is absent",
                        file.path.display(),
                        parent.display()
                    );
                }
                (
                    super::managed_files::ManagedState::Absent,
                    super::managed_files::ManagedState::Present,
                ) => {}
            }
        }
    }
    for resource in unavailable_files {
        plan.insert(resource)?;
    }
    let service_dependencies = plan
        .resources
        .keys()
        .filter(|id| matches!(id.kind.as_str(), "package" | "file" | "directory"))
        .cloned()
        .collect::<Vec<_>>();
    for resource in super::services::plans_with_notifications(&services, &notified_services) {
        let id = resource.id.clone();
        plan.insert(resource)?;
        for dependency in &service_dependencies {
            plan.add_dependency(&id, dependency.clone())?;
        }
    }
    if let Some(mut firewall) = super::firewall::prepare_request_from_config(config)? {
        super::firewall::inspect_request(&mut firewall)?;
        let dependencies = plan
            .resources
            .keys()
            .filter(|id| {
                matches!(
                    id.kind.as_str(),
                    "package" | "file" | "directory" | "service"
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let firewall_resources = firewall.plans();
        for resource in firewall_resources {
            plan.insert(resource)?;
        }
        let policy_id = ResourceId::new("firewall", "linux");
        let rule_ids = plan
            .resources
            .keys()
            .filter(|id| id.kind == "firewall-rule")
            .cloned()
            .collect::<Vec<_>>();
        for rule_id in &rule_ids {
            for dependency in &dependencies {
                plan.add_dependency(rule_id, dependency.clone())?;
            }
            // Backends apply allow rules before activating default-deny policy.
            plan.add_dependency(&policy_id, rule_id.clone())?;
        }
        for dependency in &dependencies {
            plan.add_dependency(&policy_id, dependency.clone())?;
        }
    }
    let mut compose = super::compose::prepare_requests_from_config(config)?;
    super::compose::inspect_requests(&mut compose);
    for request in &compose {
        let mut dependencies = request
            .path_dependencies()
            .iter()
            .filter(|dependency| plan.resources.contains_key(*dependency))
            .cloned()
            .collect::<IndexSet<_>>();
        let firewall = ResourceId::new("firewall", "linux");
        if plan.resources.contains_key(&firewall) {
            dependencies.insert(firewall);
        }
        for dependency in request.explicit_dependencies() {
            if !plan.resources.contains_key(dependency) {
                bail!(
                    "bootstrap compose project '{}' depends on missing resource '{}'",
                    request.name,
                    dependency
                );
            }
            dependencies.insert(dependency.clone());
        }
        let dependency_changed = dependencies.iter().any(|dependency| {
            plan.resources.get(dependency).is_some_and(|resource| {
                matches!(
                    resource.action,
                    ResourceAction::Create | ResourceAction::Update | ResourceAction::Remove
                )
            })
        });
        let resource = request.plan_with_dependency_change(dependency_changed);
        let id = resource.id.clone();
        plan.insert(resource)?;
        for dependency in dependencies {
            plan.add_dependency(&id, dependency)?;
        }
    }
    // Validate dependency references and cycles even when callers only need JSON.
    plan.output()?;
    Ok(plan)
}

fn add_account_dependencies(
    plan: &mut BootstrapPlan,
    resource: &ResourceId,
    state: super::managed_files::ManagedState,
    owner: Option<&str>,
    group: Option<&str>,
    user_states: &HashMap<String, super::accounts::AccountState>,
    group_states: &HashMap<String, super::accounts::AccountState>,
) -> Result<()> {
    if state != super::managed_files::ManagedState::Present {
        return Ok(());
    }
    if let Some(owner) = owner {
        match user_states.get(owner) {
            Some(super::accounts::AccountState::Present) => {
                plan.add_dependency(resource, ResourceId::new("user", owner))?;
            }
            Some(super::accounts::AccountState::Absent) => bail!(
                "bootstrap resource '{resource}' requires owner '{owner}', but that user is absent"
            ),
            None => {}
        }
    }
    if let Some(group) = group {
        match group_states.get(group) {
            Some(super::accounts::AccountState::Present) => {
                plan.add_dependency(resource, ResourceId::new("group", group))?;
            }
            Some(super::accounts::AccountState::Absent) => bail!(
                "bootstrap resource '{resource}' requires group '{group}', but that group is absent"
            ),
            None => {}
        }
    }
    Ok(())
}

fn desired_package(request: &super::packages::PackageRequest) -> String {
    request
        .version
        .as_ref()
        .map(|version| format!("installed ({version})"))
        .unwrap_or_else(|| "installed (any version)".to_string())
}

fn package_resource_state(
    state: PackageState,
    request: &PackageRequest,
    supports_version_pins: bool,
) -> (String, ResourceAction) {
    let unsupported_pin = request.version.is_some() && !supports_version_pins;
    match state {
        PackageState::Installed { version } => {
            (format!("installed ({version})"), ResourceAction::Noop)
        }
        PackageState::Missing if unsupported_pin => (
            "missing (manager cannot install pinned versions)".to_string(),
            ResourceAction::Unknown,
        ),
        PackageState::Missing => ("missing".to_string(), ResourceAction::Create),
        PackageState::NeedsRepair { installed } => (
            format!("{installed} (needs repair)"),
            ResourceAction::Update,
        ),
        PackageState::VersionMismatch { installed } if unsupported_pin => (
            format!("{installed} (manager cannot install pinned versions)"),
            ResourceAction::Unknown,
        ),
        PackageState::VersionMismatch { installed } => (installed, ResourceAction::Update),
        #[cfg(unix)]
        PackageState::Unavailable { reason } => {
            (format!("skipped ({reason})"), ResourceAction::Unknown)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn package_request(version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: "example".to_string(),
            version: version.map(str::to_string),
            tap_url: None,
        }
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

    #[test]
    fn unpinnable_missing_and_mismatched_packages_are_unknown() {
        let request = package_request(Some("1.2.3"));

        for state in [
            PackageState::Missing,
            PackageState::VersionMismatch {
                installed: "1.0.0".to_string(),
            },
        ] {
            let (current, action) = package_resource_state(state, &request, false);
            assert_eq!(action, ResourceAction::Unknown);
            assert!(current.contains("cannot install pinned versions"));
        }
    }

    #[test]
    fn unpinnable_package_repair_remains_actionable() {
        let request = package_request(Some("1.2.3"));
        let (_, action) = package_resource_state(
            PackageState::NeedsRepair {
                installed: "1.2.3".to_string(),
            },
            &request,
            false,
        );

        assert_eq!(action, ResourceAction::Update);
    }

    #[test]
    fn managers_with_pin_support_plan_missing_and_mismatched_packages() {
        let request = package_request(Some("1.2.3"));
        let (_, missing_action) = package_resource_state(PackageState::Missing, &request, true);
        let (_, mismatch_action) = package_resource_state(
            PackageState::VersionMismatch {
                installed: "1.0.0".to_string(),
            },
            &request,
            true,
        );

        assert_eq!(missing_action, ResourceAction::Create);
        assert_eq!(mismatch_action, ResourceAction::Update);
    }
}
