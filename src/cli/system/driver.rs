//! Shared per-manager execution loop for `mise bootstrap packages apply`/`upgrade`/`use`.

use std::collections::{HashMap, HashSet};

use eyre::{Result, bail};

use crate::config::Settings;
use crate::system::ManagerPackages;
use crate::system::packages::{InstallOpts, PackageDesiredState, PackageState, PackageStatus};
use crate::ui::prompt;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Action {
    Install,
    Upgrade,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Upgrade => "upgrade",
        }
    }
}

pub(crate) struct DriverOpts {
    /// `--manager` filter
    pub manager: Option<String>,
    /// packages were named explicitly on the CLI — unavailable managers are
    /// then a hard error instead of a silent (cross-platform config) skip
    pub explicit: bool,
    /// An explicitly named manager may still be written to shared config on a
    /// platform where that manager is unavailable.
    pub allow_unavailable_manager: bool,
    pub dry_run: bool,
    pub update: bool,
    pub yes: bool,
    /// Include configured optional packages without prompting.
    pub with_optional: bool,
}

fn unavailable_manager_is_error(d: &DriverOpts) -> bool {
    (d.manager.is_some() || d.explicit) && !d.allow_unavailable_manager
}

fn unavailable_package_reason<'a>(
    d: &DriverOpts,
    statuses: &'a [PackageStatus],
) -> Option<&'a str> {
    if !d.explicit {
        return None;
    }
    statuses
        .iter()
        .find_map(|status| status.state.unavailable_reason())
}

fn include_install_target(status: &PackageStatus, include_missing_optional: bool) -> bool {
    if status.state.is_installed() || status.state.is_unavailable() {
        return false;
    }
    status.request.desired != PackageDesiredState::Optional
        || !matches!(status.state, PackageState::Missing)
        || include_missing_optional
}

/// Run `action` for every manager in `mgrs`, honoring the `--manager` filter,
/// disabled/unavailable managers, unsatisfiable version pins, and the
/// confirmation prompt.
pub(crate) async fn run(mgrs: Vec<ManagerPackages>, action: Action, d: &DriverOpts) -> Result<()> {
    if let Some(only) = &d.manager
        && !mgrs.iter().any(|mp| mp.manager.name() == only)
    {
        // distinguish "not configured" from "filtered out by settings" —
        // the aggregation drops managers excluded by
        // system_packages.managers before we ever see them
        if let Some(enabled) = &Settings::get().system_packages.managers
            && !enabled.contains(only)
        {
            bail!(
                "manager '{only}' is excluded by the system_packages.managers setting \
                 (currently: {})",
                enabled.join(", ")
            );
        }
        bail!("no packages requested for manager '{only}'");
    }
    if mgrs.is_empty() {
        info!("no bootstrap packages configured in [bootstrap.packages]");
        return Ok(());
    }
    let opts = InstallOpts {
        dry_run: d.dry_run,
        update: d.update,
    };
    for mp in mgrs {
        if let Some(only) = &d.manager
            && mp.manager.name() != only
        {
            continue;
        }
        let name = mp.manager.name();
        if mp.disabled {
            if d.manager.is_some() {
                bail!("manager '{name}' is excluded by the system_packages.managers setting");
            }
            debug!("{name}: skipping, excluded by system_packages.managers");
            continue;
        }
        if let Some(reason) = mp.manager.unavailable_reason_async().await {
            if unavailable_manager_is_error(d) {
                // explicitly requested (via --manager or manager:package
                // specs) — failing silently would be a lie
                bail!("{name} is not available: {}", reason);
            }
            debug!("{name}: skipping, {reason}");
            continue;
        }
        if !d.dry_run {
            mp.manager.prepare_mutation(&mp.requests).await?;
        }
        let statuses = mp.manager.installed(&mp.requests).await?;
        if let Some(reason) = unavailable_package_reason(d, &statuses) {
            bail!("{reason}");
        }
        let prompted_optional =
            if action == Action::Install && !d.with_optional && !d.dry_run && !d.yes {
                let options = statuses
                    .iter()
                    .filter(|status| status.request.desired == PackageDesiredState::Optional)
                    .filter(|status| matches!(status.state, PackageState::Missing))
                    .map(|status| format!("{name}:{}", status.request))
                    .collect::<Vec<_>>();
                if options.is_empty() {
                    HashSet::new()
                } else {
                    prompt::multiselect(
                        format!("{name}: optional packages"),
                        "Select optional packages to install",
                        options,
                    )?
                    .into_iter()
                    .collect()
                }
            } else {
                HashSet::new()
            };
        let remove_targets = if action == Action::Install {
            statuses
                .iter()
                .filter(|status| {
                    status.request.desired == PackageDesiredState::Absent
                        && !matches!(status.state, PackageState::Missing)
                        && !status.state.is_unavailable()
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };
        let mut targets: Vec<_> = statuses
            .iter()
            .filter(|status| status.request.desired != PackageDesiredState::Absent)
            .filter(|s| match action {
                Action::Install => include_install_target(
                    s,
                    d.with_optional || prompted_optional.contains(&format!("{name}:{}", s.request)),
                ),
                // upgrade acts on whatever is present (the manager no-ops
                // already-current packages); missing packages are skipped
                // below with a pointer at `install`
                Action::Upgrade => {
                    !matches!(s.state, PackageState::Missing) && !s.state.is_unavailable()
                }
            })
            .collect();
        let missing = statuses
            .iter()
            .filter(|status| status.request.desired == PackageDesiredState::Present)
            .filter(|status| matches!(status.state, PackageState::Missing))
            .count();
        if action == Action::Upgrade && missing > 0 {
            warn!(
                "{name}: {missing} package(s) not installed — run `mise bootstrap packages apply` first"
            );
        }
        // a pin this manager can never satisfy must not block the rest
        // of the batch — it stays visible in `status` as a mismatch
        if !mp.manager.supports_version_pins() {
            targets.retain(|status| {
                if status.request.version.is_some()
                    && !matches!(status.state, PackageState::NeedsRepair { .. })
                {
                    warn!(
                        "{name}: cannot {} pinned version '{}', skipping",
                        action.verb(),
                        status.request
                    );
                    false
                } else {
                    true
                }
            });
        }
        let installed = statuses
            .iter()
            .filter(|status| status.request.desired != PackageDesiredState::Absent)
            .filter(|status| status.state.is_installed())
            .count();
        if action == Action::Install && installed > 0 {
            info!("{name}: {installed} package(s) already installed");
        }
        if action == Action::Install && !d.with_optional {
            let skipped = statuses
                .iter()
                .filter(|status| status.request.desired == PackageDesiredState::Optional)
                .filter(|status| matches!(status.state, PackageState::Missing))
                .filter(|status| !prompted_optional.contains(&format!("{name}:{}", status.request)))
                .map(|status| status.request.to_string())
                .collect::<Vec<_>>();
            if !skipped.is_empty() {
                info!(
                    "{name}: skipped optional packages {} (use --with-optional to install)",
                    skipped.join(", ")
                );
            }
        }
        let already_absent = statuses
            .iter()
            .filter(|status| status.request.desired == PackageDesiredState::Absent)
            .filter(|status| matches!(status.state, PackageState::Missing))
            .count();
        if action == Action::Install && already_absent > 0 {
            info!("{name}: {already_absent} package(s) already absent");
        }
        if !remove_targets.is_empty() {
            if !mp.manager.supports_remove() {
                bail!("{name} does not support declarative package removal");
            }
            let remove = remove_targets
                .iter()
                .map(|status| status.request.clone())
                .collect::<Vec<_>>();
            let list = remove
                .iter()
                .map(|request| request.to_string())
                .collect::<Vec<_>>();
            if !d.dry_run && !d.yes && console::user_attended_stderr() {
                let msg = format!("{name}: remove {}?", list.join(", "));
                if !prompt::confirm(msg)?.is_yes() {
                    info!("{name}: removal skipped");
                } else {
                    mp.manager.remove(&remove, &opts).await?;
                    info!("{name}: removed {}", list.join(", "));
                }
            } else {
                mp.manager.remove(&remove, &opts).await?;
                if !d.dry_run {
                    info!("{name}: removed {}", list.join(", "));
                }
            }
        }
        if targets.is_empty() {
            continue;
        }
        let targets = targets
            .into_iter()
            .map(|status| status.request.clone())
            .collect::<Vec<_>>();
        let list = targets.iter().map(|r| r.to_string()).collect::<Vec<_>>();
        if !d.dry_run && !d.yes && console::user_attended_stderr() {
            let msg = format!("{name}: {} {}?", action.verb(), list.join(", "));
            if !prompt::confirm(msg)?.is_yes() {
                info!("{name}: skipped");
                continue;
            }
        }
        match action {
            Action::Install => {
                mp.manager
                    .install_with_options(&targets, &opts, &mp.options)
                    .await?;
                if !d.dry_run {
                    info!("{name}: installed {}", list.join(", "));
                }
            }
            Action::Upgrade => {
                // managers no-op packages that are already current, so
                // re-query afterwards and report only what actually changed
                let prior: HashMap<String, String> = statuses
                    .iter()
                    .filter_map(|s| match &s.state {
                        PackageState::Installed { version }
                        | PackageState::NeedsRepair { installed: version }
                        | PackageState::VersionMismatch { installed: version } => {
                            Some((s.request.name.clone(), version.clone()))
                        }
                        #[cfg(unix)]
                        PackageState::InstalledAutoUpdates { version } => {
                            Some((s.request.name.clone(), version.clone()))
                        }
                        PackageState::Missing => None,
                        #[cfg(unix)]
                        PackageState::Unavailable { .. } => None,
                    })
                    .collect();
                mp.manager.upgrade(&targets, &opts).await?;
                if !d.dry_run {
                    let after = mp.manager.installed(&targets).await?;
                    let changed: Vec<String> = after
                        .iter()
                        .filter_map(|s| match &s.state {
                            PackageState::Installed { version }
                            | PackageState::NeedsRepair { installed: version }
                            | PackageState::VersionMismatch { installed: version } => {
                                let old = prior.get(&s.request.name)?;
                                (old != version)
                                    .then(|| format!("{} {old} -> {version}", s.request.name))
                            }
                            #[cfg(unix)]
                            PackageState::InstalledAutoUpdates { version } => {
                                let old = prior.get(&s.request.name)?;
                                (old != version)
                                    .then(|| format!("{} {old} -> {version}", s.request.name))
                            }
                            PackageState::Missing => None,
                            #[cfg(unix)]
                            PackageState::Unavailable { .. } => None,
                        })
                        .collect();
                    if changed.is_empty() {
                        info!("{name}: already up to date");
                    } else {
                        info!("{name}: upgraded {}", changed.join(", "));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::system::packages::PackageRequest;

    fn package_status(desired: PackageDesiredState, state: PackageState) -> PackageStatus {
        PackageStatus {
            request: PackageRequest {
                name: "example".to_string(),
                version: None,
                tap_url: None,
                desired,
            },
            state,
        }
    }

    #[test]
    fn missing_optional_packages_require_selection() {
        let status = package_status(PackageDesiredState::Optional, PackageState::Missing);
        assert!(!include_install_target(&status, false));
        assert!(include_install_target(&status, true));
    }

    #[test]
    fn installed_optional_packages_remain_managed() {
        let status = package_status(
            PackageDesiredState::Optional,
            PackageState::VersionMismatch {
                installed: "1.0.0".to_string(),
            },
        );
        assert!(include_install_target(&status, false));
    }

    #[test]
    fn explicit_packages_reject_unavailable_entries_but_manager_filters_skip_them() {
        let explicit_opts = DriverOpts {
            manager: None,
            explicit: true,
            allow_unavailable_manager: true,
            dry_run: false,
            update: false,
            yes: true,
            with_optional: false,
        };
        let statuses = vec![PackageStatus {
            request: PackageRequest {
                name: "example".to_string(),
                version: None,
                tap_url: None,
                desired: PackageDesiredState::Present,
            },
            state: PackageState::Unavailable {
                reason: "unsupported on this platform".to_string(),
            },
        }];

        assert!(!unavailable_manager_is_error(&explicit_opts));
        assert_eq!(
            unavailable_package_reason(&explicit_opts, &statuses),
            Some("unsupported on this platform")
        );

        let manager_opts = DriverOpts {
            manager: Some("brew-cask".to_string()),
            explicit: false,
            allow_unavailable_manager: false,
            dry_run: false,
            update: false,
            yes: true,
            with_optional: false,
        };
        assert!(unavailable_manager_is_error(&manager_opts));
        assert_eq!(unavailable_package_reason(&manager_opts, &statuses), None);
    }
}
