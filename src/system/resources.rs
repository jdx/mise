use std::collections::{HashMap, VecDeque};
use std::fmt;

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Serialize;

use crate::config::Config;
use crate::system::packages::{PackageRequest, PackageState};

/// Stable identity for one declarative bootstrap resource.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
pub struct ResourceId {
    pub kind: String,
    pub name: String,
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
    Noop,
    Unknown,
}

impl fmt::Display for ResourceAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Create => "create",
            Self::Update => "update",
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
            depends_on: vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct PlanSummary {
    pub create: usize,
    pub update: usize,
    pub unchanged: usize,
    pub unknown: usize,
}

impl PlanSummary {
    fn add(&mut self, action: ResourceAction) {
        match action {
            ResourceAction::Create => self.create += 1,
            ResourceAction::Update => self.update += 1,
            ResourceAction::Noop => self.unchanged += 1,
            ResourceAction::Unknown => self.unknown += 1,
        }
    }

    pub fn has_changes(self) -> bool {
        self.create + self.update > 0
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
pub async fn plan(config: &Config) -> Result<BootstrapPlan> {
    let mut plan = BootstrapPlan::default();
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
    for (manager_name, requests) in super::pending_plugin_packages_from_config(config) {
        for request in requests {
            plan.insert(ResourcePlan::new(
                ResourceId::new("package", format!("{manager_name}:{}", request.name)),
                "unavailable (package plugin is not installed)",
                desired_package(&request),
                ResourceAction::Unknown,
            ))?;
        }
    }
    // Validate dependency references and cycles even when callers only need JSON.
    plan.output()?;
    Ok(plan)
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
