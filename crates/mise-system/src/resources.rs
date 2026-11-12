use std::collections::HashMap;

use eyre::{Result, bail};
use indexmap::IndexSet;

use crate::host::Config;
use crate::packages::{PackageRequest, PackageState};

pub use mise_bootstrap::{
    BootstrapPlan, BootstrapPlanOutput, PlanSummary, ResourceAction, ResourceId, ResourceOrigin,
    ResourcePlan, serialize_path,
};

/// Build the resource plan currently supported by the provisioning engine.
/// Other bootstrap sections will move into this graph as resource adapters land.
///
/// `include_firewall = false` leaves the firewall out of the graph entirely,
/// so a run that skips it never inspects it (which needs elevation).
pub async fn plan(
    config: &Config,
    secrets: &super::secrets::SecretValues,
    include_firewall: bool,
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
        .filter(|id| match id.kind.as_str() {
            // plugin packages run after services, so services never wait for them
            "package" => builtin_packages.contains(id),
            "file" | "directory" => true,
            _ => false,
        })
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
    let firewall = if include_firewall {
        super::firewall::prepare_request_from_config(config)?
    } else {
        None
    };
    if let Some(mut firewall) = firewall {
        super::firewall::inspect_request(&mut firewall)?;
        let dependencies = plan
            .ids()
            .filter(|id| match id.kind.as_str() {
                "package" => builtin_packages.contains(id),
                "file" | "directory" | "service" => true,
                _ => false,
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

/// Add the `[bootstrap.repos]` repositories and, when `hooks_enabled`, the
/// `[bootstrap.hooks]` commands, which `plan` leaves out because they read
/// the network or run arbitrary commands rather than inspect local state.
///
/// Hooks are ordered as a bootstrap run executes them: by phase, then in
/// declaration order, with packages between the `packages` hooks and
/// repositories between the `repos` hooks.
pub async fn add_repos_and_hooks(
    plan: &mut BootstrapPlan,
    config: &Config,
    hooks_enabled: bool,
) -> Result<()> {
    use super::hooks::BootstrapHookPhase;
    use strum::IntoEnumIterator;

    // plugin packages run after tools, so they neither precede repos nor the
    // `post-packages` hook
    let builtin_packages = super::packages_from_config(config)?
        .into_iter()
        .filter(|packages| !packages.manager.is_plugin())
        .flat_map(|packages| {
            let manager = packages.manager.name().to_string();
            packages.requests.into_iter().map(move |request| {
                ResourceId::new("package", format!("{manager}:{}", request.name))
            })
        })
        .collect::<std::collections::HashSet<_>>();
    let packages = plan
        .ids()
        .filter(|id| builtin_packages.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    // user services with `requires_tools` run after tools and plugin packages
    let late_names = super::user_services::requests_from_config(config)?
        .into_iter()
        .filter(|request| request.requires_tools)
        .map(|request| ResourceId::new("user-service", request.name))
        .collect::<std::collections::HashSet<_>>();
    let late = plan
        .ids()
        .filter(|id| late_names.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    // accounts, files, services, the firewall, and Compose all run before repos
    let earlier = plan
        .ids()
        .filter(|id| id.kind != "package" || builtin_packages.contains(id))
        .filter(|id| !late_names.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    // post-package files, services, the firewall, and Compose follow the
    // `post-packages` hook
    let after_packages_hook = plan
        .ids()
        .filter(|id| {
            matches!(
                id.kind.as_str(),
                "service" | "user-service" | "firewall" | "firewall-rule" | "compose"
            ) && !late_names.contains(id)
                || plan.get(id).is_some_and(|r| {
                    r.phase == Some(mise_bootstrap::ManagedFilePhase::PostPackages)
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    // bootstrap defers the `post-packages` hook until plugin packages finish
    let defer_post_packages = plan
        .ids()
        .any(|id| id.kind == "package" && !builtin_packages.contains(id));
    let requests = super::repos_from_config(config);
    let mut repos = vec![];
    for status in super::repos::status(&requests).await? {
        let resource = repo_plan(&status);
        repos.push(resource.id.clone());
        plan.insert(resource)?;
    }
    for repo in &repos {
        for resource in &earlier {
            order_after(plan, repo, resource)?;
        }
    }
    // plugin packages run after tools, so after everything planned here
    let plugin_packages = plan
        .ids()
        .filter(|id| id.kind == "package" && !builtin_packages.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    for package in &plugin_packages {
        for resource in earlier.iter().chain(&repos) {
            order_after(plan, package, resource)?;
        }
    }
    for service in &late {
        for resource in earlier.iter().chain(&repos).chain(&plugin_packages) {
            order_after(plan, service, resource)?;
        }
    }
    if !hooks_enabled {
        return Ok(());
    }
    // every package, plugin ones included, follows the `pre-packages` hooks
    let all_packages = packages
        .iter()
        .chain(&plugin_packages)
        .cloned()
        .collect::<Vec<_>>();
    let hooks = super::hooks_from_config(config);
    let mut previous: Option<ResourceId> = None;
    // resources the next hook to run must follow
    let mut pending: Vec<ResourceId> = vec![];
    let mut phases = BootstrapHookPhase::iter()
        .filter(|phase| !(defer_post_packages && *phase == BootstrapHookPhase::PostPackages))
        .collect::<Vec<_>>();
    if defer_post_packages {
        // it runs after plugin packages, just before the `final` hook
        let at = phases
            .iter()
            .position(|phase| *phase == BootstrapHookPhase::Final)
            .unwrap_or(phases.len());
        phases.insert(at, BootstrapHookPhase::PostPackages);
    }
    for phase in phases {
        let before_phase = previous.clone();
        match phase {
            BootstrapHookPhase::PostPackages if defer_post_packages => {
                pending.extend(
                    plan.ids()
                        .filter(|id| id.kind != "hook" && !late_names.contains(id))
                        .cloned(),
                );
            }
            BootstrapHookPhase::PostPackages => pending.extend(packages.iter().cloned()),
            BootstrapHookPhase::PreRepos => pending.extend(earlier.iter().cloned()),
            BootstrapHookPhase::PostRepos => pending.extend(repos.iter().cloned()),
            BootstrapHookPhase::Final => pending.extend(late.iter().cloned()),
            _ => {}
        }
        let mut index = 0;
        for hook in hooks.iter().filter(|hook| hook.phase == phase) {
            index += 1;
            let name = phase.to_string();
            let id = ResourceId::new("hook", format!("{name}:{index}"));
            plan.insert(
                ResourcePlan::new(id.clone(), "not run", hook.run.clone(), ResourceAction::Run)
                    .with_hook_phase(name),
            )?;
            if let Some(previous) = previous.take() {
                plan.add_ordering(&id, previous)?;
            }
            for anchor in pending.drain(..) {
                plan.add_ordering(&id, anchor)?;
            }
            previous = Some(id);
        }
        // packages and repos run after the hooks of their `pre-` phase
        let targets: &[ResourceId] = match phase {
            BootstrapHookPhase::PrePackages => &all_packages,
            BootstrapHookPhase::PreRepos => &repos,
            _ => &[],
        };
        if let Some(last) = previous
            .as_ref()
            .filter(|last| Some(*last) != before_phase.as_ref())
        {
            for target in targets {
                plan.add_ordering(target, last.clone())?;
            }
            if phase == BootstrapHookPhase::PostPackages {
                if !defer_post_packages {
                    for target in &after_packages_hook {
                        order_after(plan, target, last)?;
                    }
                }
                for service in &late {
                    order_after(plan, service, last)?;
                }
            }
        }
    }
    Ok(())
}

/// Order `resource` after `predecessor` unless `predecessor` already waits on
/// `resource`, which an explicit dependency may declare.
fn order_after(
    plan: &mut BootstrapPlan,
    resource: &ResourceId,
    predecessor: &ResourceId,
) -> Result<()> {
    if plan.waits_on(predecessor, resource) {
        return Ok(());
    }
    plan.add_ordering(resource, predecessor.clone())
}

fn repo_plan(status: &super::repos::RepoStatus) -> ResourcePlan {
    use super::repos::RepoState;

    let request = &status.request;
    let desired = match &request.git_ref {
        Some(git_ref) => format!("{} at {git_ref}", request.url),
        None => request.url.clone(),
    };
    let (current, action) = match &status.state {
        RepoState::Current => ("current".to_string(), ResourceAction::Noop),
        RepoState::Missing => ("missing".to_string(), ResourceAction::Create),
        RepoState::Differs => {
            let current = match (&status.current_ref, &status.current_sha) {
                (Some(git_ref), _) => git_ref.clone(),
                (None, Some(sha)) => sha.chars().take(12).collect(),
                (None, None) => "differs".to_string(),
            };
            (current, ResourceAction::Update)
        }
        // Bootstrap refuses these unless `--skip-dirty` leaves the repo alone,
        // so the plan cannot say what a run would do.
        RepoState::Dirty => ("dirty (local changes)".to_string(), ResourceAction::Unknown),
        RepoState::Conflict(reason) => (format!("conflict ({reason})"), ResourceAction::Unknown),
    };
    ResourcePlan::new(
        ResourceId::new("repo", request.path.to_string_lossy()),
        current,
        desired,
        action,
    )
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
            desired: crate::packages::PackageDesiredState::Present,
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
        request.desired = crate::packages::PackageDesiredState::Absent;
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
