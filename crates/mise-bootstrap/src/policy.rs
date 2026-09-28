//! Bootstrap-specific dependency policies.

use std::collections::HashMap;
use std::path::Path;

use eyre::{Result, bail};

use crate::plan::BootstrapPlan;
use crate::resource::ResourceId;
use crate::state::{AccountState, ManagedFilePhase, ManagedState};

impl BootstrapPlan {
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

    /// Depend on declared owners and groups for present resources, rejecting
    /// principals marked absent. Removed resources do not need principals.
    pub fn add_account_dependencies(
        &mut self,
        resource: &ResourceId,
        state: ManagedState,
        owner: Option<&str>,
        group: Option<&str>,
        user_states: &HashMap<String, AccountState>,
        group_states: &HashMap<String, AccountState>,
    ) -> Result<()> {
        if state != ManagedState::Present {
            return Ok(());
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{ResourceAction, ResourcePlan};

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

        plan.add_account_dependencies(
            &file,
            ManagedState::Present,
            Some("service"),
            Some("services"),
            &users,
            &groups,
        )
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
            .add_account_dependencies(
                &file,
                ManagedState::Present,
                Some("service"),
                None,
                &absent,
                &missing,
            )
            .unwrap_err();
        assert!(error.to_string().contains("owner 'service'"));
        let error = plan
            .add_account_dependencies(
                &file,
                ManagedState::Present,
                None,
                Some("service"),
                &missing,
                &absent,
            )
            .unwrap_err();
        assert!(error.to_string().contains("group 'service'"));
        plan.add_account_dependencies(
            &file,
            ManagedState::Present,
            Some("external"),
            None,
            &missing,
            &missing,
        )
        .unwrap();
        assert!(plan.get(&file).unwrap().depends_on.is_empty());
        plan.add_account_dependencies(
            &file,
            ManagedState::Absent,
            Some("service"),
            Some("service"),
            &absent,
            &absent,
        )
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
}
