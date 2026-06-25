//! Shared per-manager execution loop for `mise bootstrap packages apply`/`upgrade`/`use`.

use std::collections::HashMap;

use eyre::{Result, bail};

use crate::config::Settings;
use crate::system::ManagerPackages;
use crate::system::packages::{InstallOpts, PackageState};
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
    pub dry_run: bool,
    pub update: bool,
    pub yes: bool,
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
        if !mp.manager.is_available() {
            if d.manager.is_some() || d.explicit {
                // explicitly requested (via --manager or manager:package
                // specs) — failing silently would be a lie
                bail!(
                    "{name} is not available: {}",
                    mp.manager.unavailable_reason()
                );
            }
            debug!("{name}: skipping, {}", mp.manager.unavailable_reason());
            continue;
        }
        let statuses = mp.manager.installed(&mp.requests).await?;
        let mut targets: Vec<_> = statuses
            .iter()
            .filter(|s| match action {
                Action::Install => !matches!(s.state, PackageState::Installed { .. }),
                // upgrade acts on whatever is present (the manager no-ops
                // already-current packages); missing packages are skipped
                // below with a pointer at `install`
                Action::Upgrade => !matches!(s.state, PackageState::Missing),
            })
            .map(|s| s.request.clone())
            .collect();
        let skipped = statuses.len() - targets.len();
        if action == Action::Upgrade && skipped > 0 {
            warn!(
                "{name}: {skipped} package(s) not installed — run `mise bootstrap packages apply` first"
            );
        }
        // a pin this manager can never satisfy must not block the rest
        // of the batch — it stays visible in `status` as a mismatch
        if !mp.manager.supports_version_pins() {
            targets.retain(|r| {
                if r.version.is_some() {
                    warn!(
                        "{name}: cannot {} pinned version '{r}', skipping",
                        action.verb()
                    );
                    false
                } else {
                    true
                }
            });
        }
        if action == Action::Install && skipped > 0 {
            info!("{name}: {skipped} package(s) already installed");
        }
        if targets.is_empty() {
            continue;
        }
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
                mp.manager.install(&targets, &opts).await?;
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
                        | PackageState::VersionMismatch { installed: version } => {
                            Some((s.request.name.clone(), version.clone()))
                        }
                        PackageState::Missing => None,
                    })
                    .collect();
                mp.manager.upgrade(&targets, &opts).await?;
                if !d.dry_run {
                    let after = mp.manager.installed(&targets).await?;
                    let changed: Vec<String> = after
                        .iter()
                        .filter_map(|s| match &s.state {
                            PackageState::Installed { version }
                            | PackageState::VersionMismatch { installed: version } => {
                                let old = prior.get(&s.request.name)?;
                                (old != version)
                                    .then(|| format!("{} {old} -> {version}", s.request.name))
                            }
                            PackageState::Missing => None,
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
