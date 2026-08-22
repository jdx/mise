//! Shared per-manager execution loop for `mise bootstrap packages apply`/`upgrade`/`use`.

use std::collections::HashMap;

use eyre::{Result, bail};

use crate::config::Settings;
use crate::system::ManagerPackages;
use crate::system::packages::{InstallOpts, PackageState, PackageStatus};
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
}

fn unavailable_manager_is_error(d: &DriverOpts) -> bool {
    (d.manager.is_some() || d.explicit) && !d.allow_unavailable_manager
}

fn unsupported_package_reason<'a>(
    d: &DriverOpts,
    manager: &str,
    statuses: &'a [PackageStatus],
) -> Option<&'a str> {
    statuses.iter().find_map(|status| {
        let reason = status.state.unsupported_reason()?;
        let implicit_uninstalled_cask = implicit_uninstalled_cask(d, manager, status);
        (!implicit_uninstalled_cask).then_some(reason)
    })
}

#[cfg(unix)]
fn implicit_uninstalled_cask(d: &DriverOpts, manager: &str, status: &PackageStatus) -> bool {
    manager == "brew-cask"
        && !d.explicit
        && !crate::system::packages::brew::BrewCaskManager::unsupported_state_is_installed(
            &status.request,
        )
}

#[cfg(not(unix))]
fn implicit_uninstalled_cask(_d: &DriverOpts, _manager: &str, _status: &PackageStatus) -> bool {
    false
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
        let statuses = mp.manager.installed(&mp.requests).await?;
        #[cfg(unix)]
        let statuses = {
            let mut statuses = statuses;
            if name == "brew-cask" {
                for status in &mut statuses {
                    if matches!(status.state, PackageState::Missing)
                        && let Some(reason) = crate::system::packages::brew::BrewCaskManager::platform_unavailable_reason(
                                &status.request,
                            )
                            .await?
                    {
                        status.state = PackageState::unsupported(reason);
                    }
                }
            }
            statuses
        };
        if let Some(reason) = unsupported_package_reason(d, name, &statuses) {
            bail!("{reason}");
        }
        let mut targets: Vec<_> = statuses
            .iter()
            .filter(|s| match action {
                Action::Install => {
                    !s.state.is_installed() && s.state.unsupported_reason().is_none()
                }
                // upgrade acts on whatever is present (the manager no-ops
                // already-current packages); missing packages are skipped
                // below with a pointer at `install`.
                Action::Upgrade => !matches!(s.state, PackageState::Missing),
            })
            .collect();
        let missing = statuses
            .iter()
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
            .filter(|status| status.state.is_installed())
            .count();
        if action == Action::Install && installed > 0 {
            info!("{name}: {installed} package(s) already installed");
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
            if !prompt::confirm(msg)? {
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
                    .filter_map(|s| {
                        s.state
                            .installed_version()
                            .map(|version| (s.request.name.clone(), version.to_string()))
                    })
                    .collect();
                mp.manager.upgrade(&targets, &opts).await?;
                if !d.dry_run {
                    let after = mp.manager.installed(&targets).await?;
                    let changed: Vec<String> = after
                        .iter()
                        .filter_map(|s| {
                            let version = s.state.installed_version()?;
                            let old = prior.get(&s.request.name)?;
                            (old != version)
                                .then(|| format!("{} {old} -> {version}", s.request.name))
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

    #[test]
    fn unsupported_packages_fail_when_explicit() {
        let opts = DriverOpts {
            manager: None,
            explicit: true,
            allow_unavailable_manager: false,
            dry_run: false,
            update: false,
            yes: true,
        };
        let unsupported = vec![PackageStatus {
            request: PackageRequest {
                name: "example".to_string(),
                version: None,
                tap_url: None,
            },
            state: PackageState::Unsupported {
                reason: "unsafe lifecycle semantics".to_string(),
            },
        }];
        assert_eq!(
            unsupported_package_reason(&opts, "brew-cask", &unsupported),
            Some("unsafe lifecycle semantics")
        );
    }

    #[test]
    fn implicit_uninstalled_casks_skip_unsupported_platforms() {
        let opts = DriverOpts {
            manager: None,
            explicit: false,
            allow_unavailable_manager: false,
            dry_run: false,
            update: false,
            yes: true,
        };
        let unsupported = vec![PackageStatus {
            request: PackageRequest {
                name: "mise-test-unsupported-platform-cask".to_string(),
                version: None,
                tap_url: None,
            },
            state: PackageState::unsupported("unsupported on this platform"),
        }];
        assert_eq!(
            unsupported_package_reason(&opts, "brew-cask", &unsupported),
            None
        );
        assert_eq!(
            unsupported_package_reason(&opts, "other", &unsupported),
            Some("unsupported on this platform")
        );
    }
}
