//! Declaration-ordered graph storage, validation, and output.

use std::collections::{HashMap, VecDeque};

use eyre::{Result, bail};
use indexmap::IndexMap;
use serde::Serialize;

use crate::resource::{ResourceAction, ResourceId, ResourcePlan};

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
