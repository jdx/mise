use std::collections::HashMap;

use eyre::{Result, bail};
use indexmap::IndexSet;

use crate::config::Config;
use crate::system::packages::{PackageRequest, PackageState};

pub use mise_bootstrap::{
    BootstrapPlan, BootstrapPlanOutput, PlanSummary, ResourceAction, ResourceId, ResourceOrigin,
    ResourcePlan, serialize_path,
};

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
    for manager_packages in super::packages_from_config(config)? {
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
        let supports_remove = manager.supports_remove();
        for status in manager
            .installed_with_options(&manager_packages.requests, &manager_packages.options)
            .await?
        {
            let id = ResourceId::new("package", format!("{manager_name}:{}", status.request.name));
            let desired = desired_package(&status.request);
            let (current, action) = package_resource_state(
                status.state,
                &status.request,
                supports_version_pins,
                supports_remove,
            );
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
    let user_services = super::services_common::user_service_names(config)?;
    super::services::validate_notifications(&files, &directories, &services, &user_services)?;
    let notified_services = super::managed_files::pending_notifications(&files, &directories)?;
    let directory_states = directories
        .iter()
        .map(|directory| (directory.path.clone(), directory.state))
        .collect::<std::collections::HashMap<_, _>>();
    for directory in &directories {
        let resource = directory.plan()?;
        plan.insert(resource)?;
        plan.add_account_dependencies(
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
        plan.add_managed_parent_dependency(
            &id,
            &directory.path,
            directory.state,
            parent,
            *parent_state,
        )?;
    }
    for file in files {
        let resource = file.plan()?;
        let id = resource.id.clone();
        plan.insert(resource)?;
        plan.add_account_dependencies(
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
            plan.add_managed_parent_dependency(&id, &file.path, file.state, parent, *parent_state)?;
        }
    }
    for resource in unavailable_files {
        plan.insert(resource)?;
    }
    let builtin_packages = super::packages_from_config(config)?
        .into_iter()
        .filter(|packages| !packages.manager.is_plugin())
        .flat_map(|packages| {
            let manager = packages.manager.name().to_string();
            packages.requests.into_iter().map(move |request| {
                ResourceId::new("package", format!("{manager}:{}", request.name))
            })
        })
        .collect::<Vec<_>>();
    plan.add_file_phase_ordering(&builtin_packages)?;
    let service_dependencies = plan
        .ids()
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
    let user_service_requests = super::user_services::requests_from_config(config)?;
    for status in super::user_services::status(&user_service_requests).await? {
        plan.insert(status.plan())?;
    }
    if let Some(mut firewall) = super::firewall::prepare_request_from_config(config)? {
        super::firewall::inspect_request(&mut firewall)?;
        let dependencies = plan
            .ids()
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
            .ids()
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
            .filter(|dependency| plan.contains(dependency))
            .cloned()
            .collect::<IndexSet<_>>();
        let firewall = ResourceId::new("firewall", "linux");
        if plan.contains(&firewall) {
            dependencies.insert(firewall);
        }
        for dependency in request.explicit_dependencies() {
            if !plan.contains(dependency) {
                bail!(
                    "bootstrap compose project '{}' depends on missing resource '{}'",
                    request.name,
                    dependency
                );
            }
            dependencies.insert(dependency.clone());
        }
        let dependency_changed = dependencies.iter().any(|dependency| {
            plan.get(dependency).is_some_and(|resource| {
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

fn desired_package(request: &super::packages::PackageRequest) -> String {
    if request.desired == super::packages::PackageDesiredState::Absent {
        return "absent".to_string();
    }
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
    supports_remove: bool,
) -> (String, ResourceAction) {
    if request.desired == super::packages::PackageDesiredState::Absent {
        return match state {
            PackageState::Missing => ("absent".to_string(), ResourceAction::Noop),
            #[cfg(unix)]
            PackageState::Unavailable { reason } => {
                (format!("skipped ({reason})"), ResourceAction::Unknown)
            }
            PackageState::Installed { version }
            | PackageState::NeedsRepair { installed: version }
            | PackageState::VersionMismatch { installed: version } => (
                format!("installed ({version})"),
                if supports_remove {
                    ResourceAction::Remove
                } else {
                    ResourceAction::Unknown
                },
            ),
            #[cfg(unix)]
            PackageState::InstalledAutoUpdates { version } => (
                format!("installed ({version})"),
                if supports_remove {
                    ResourceAction::Remove
                } else {
                    ResourceAction::Unknown
                },
            ),
        };
    }
    let unsupported_pin = request.version.is_some() && !supports_version_pins;
    match state {
        PackageState::Installed { version } => {
            (format!("installed ({version})"), ResourceAction::Noop)
        }
        #[cfg(unix)]
        PackageState::InstalledAutoUpdates { version } => {
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

    fn package_request(version: Option<&str>) -> PackageRequest {
        PackageRequest {
            name: "example".to_string(),
            version: version.map(str::to_string),
            tap_url: None,
            desired: crate::system::packages::PackageDesiredState::Present,
        }
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
            let (current, action) = package_resource_state(state, &request, false, false);
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
            false,
        );

        assert_eq!(action, ResourceAction::Update);
    }

    #[test]
    fn managers_with_pin_support_plan_missing_and_mismatched_packages() {
        let request = package_request(Some("1.2.3"));
        let (_, missing_action) =
            package_resource_state(PackageState::Missing, &request, true, false);
        let (_, mismatch_action) = package_resource_state(
            PackageState::VersionMismatch {
                installed: "1.0.0".to_string(),
            },
            &request,
            true,
            false,
        );

        assert_eq!(missing_action, ResourceAction::Create);
        assert_eq!(mismatch_action, ResourceAction::Update);
    }

    #[test]
    fn absent_package_plans_removal_only_when_supported() {
        let mut request = package_request(None);
        request.desired = crate::system::packages::PackageDesiredState::Absent;
        let (_, absent_action) =
            package_resource_state(PackageState::Missing, &request, false, true);
        let (_, remove_action) = package_resource_state(
            PackageState::Installed {
                version: "1.0.0".to_string(),
            },
            &request,
            false,
            true,
        );
        let (_, unsupported_action) = package_resource_state(
            PackageState::Installed {
                version: "1.0.0".to_string(),
            },
            &request,
            false,
            false,
        );

        assert_eq!(absent_action, ResourceAction::Noop);
        assert_eq!(remove_action, ResourceAction::Remove);
        assert_eq!(unsupported_action, ResourceAction::Unknown);
    }
}
