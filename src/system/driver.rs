//! Shared per-manager execution loop for `mise bootstrap packages apply`/`upgrade`/`use`.

use std::collections::HashMap;

use eyre::{Result, bail};

use crate::config::Settings;
use crate::system::ManagerPackages;
use crate::system::packages::{
    InstallOpts, PackageDesiredState, PackageRequest, PackageState, PackageStatus,
};
use crate::ui::prompt;

#[derive(Clone, Copy, PartialEq)]
pub enum Action {
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

pub struct DriverOpts {
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

/// Run `action` for every manager in `mgrs`, honoring the `--manager` filter,
/// disabled/unavailable managers, unsatisfiable version pins, and the
/// confirmation prompt.
pub async fn run(mgrs: Vec<ManagerPackages>, action: Action, d: &DriverOpts) -> Result<()> {
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
    // Each manager runs on its own: one that fails (a removal it refuses, a
    // package that does not exist, a broken repository) must not skip the
    // managers queued after it. Every failure is still reported, and still
    // fails the run, once all of them have had their turn.
    let mut failures = vec![];
    for mp in mgrs {
        if let Some(only) = &d.manager
            && mp.manager.name() != only
        {
            continue;
        }
        let mut errors = vec![];
        if let Err(err) = run_manager(&mp, action, d, &opts, &mut errors).await {
            errors.push(err);
        }
        let name = mp.manager.name();
        failures.extend(errors.into_iter().map(|err| (name.to_string(), err)));
    }
    combine_failures(failures)
}

/// Fold the per-manager failures of a run into its result. A single failure
/// is returned untouched, so its message reads as it always has.
fn combine_failures(mut failures: Vec<(String, eyre::Report)>) -> Result<()> {
    if failures.len() <= 1 {
        return failures.pop().map_or(Ok(()), |(_, err)| Err(err));
    }
    let details = failures
        .iter()
        .map(|(name, err)| format!("  {name}: {err:#}"))
        .collect::<Vec<_>>();
    bail!(
        "{} package manager operations failed:\n{}",
        failures.len(),
        details.join("\n")
    )
}

/// Whether a removal the manager cannot perform fails the run. When the run
/// was scoped to this manager or to named packages, it does, as an unavailable
/// manager does; a whole-config apply warns and skips it, the same way it
/// treats a version pin the manager cannot satisfy.
fn unsupported_removal_is_error(d: &DriverOpts) -> bool {
    d.manager.is_some() || d.explicit
}

/// Remove `remove` (the installed `state = "absent"` entries of one manager),
/// asking first when the run is attended.
async fn remove_packages(
    mp: &ManagerPackages,
    remove: &[PackageRequest],
    d: &DriverOpts,
    opts: &InstallOpts,
) -> Result<()> {
    let name = mp.manager.name();
    let list = remove
        .iter()
        .map(|request| request.to_string())
        .collect::<Vec<_>>();
    if !mp.manager.supports_remove() {
        // the entry stays visible in `status` either way
        if unsupported_removal_is_error(d) {
            bail!(
                "{name} does not support declarative package removal (cannot remove {})",
                list.join(", ")
            );
        }
        warn!(
            "{name}: cannot remove {}, skipping (declarative removal is not supported)",
            list.join(", ")
        );
        return Ok(());
    }
    if !d.dry_run && !d.yes && console::user_attended_stderr() {
        let msg = format!("{name}: remove {}?", list.join(", "));
        if !prompt::confirm(msg)?.is_yes() {
            info!("{name}: removal skipped");
            return Ok(());
        }
    }
    mp.manager.remove(remove, opts).await?;
    if !d.dry_run {
        info!("{name}: removed {}", list.join(", "));
    }
    Ok(())
}

/// Bring one manager's packages to the requested state. A failed removal is
/// pushed onto `failures` so the installs after it still run; anything else
/// that fails is returned.
async fn run_manager(
    mp: &ManagerPackages,
    action: Action,
    d: &DriverOpts,
    opts: &InstallOpts,
    failures: &mut Vec<eyre::Report>,
) -> Result<()> {
    let name = mp.manager.name();
    if mp.disabled {
        if d.manager.is_some() {
            bail!("manager '{name}' is excluded by the system_packages.managers setting");
        }
        debug!("{name}: skipping, excluded by system_packages.managers");
        return Ok(());
    }
    if let Some(reason) = mp.manager.unavailable_reason_async().await {
        if unavailable_manager_is_error(d) {
            // explicitly requested (via --manager or manager:package
            // specs) — failing silently would be a lie
            bail!("{name} is not available: {}", reason);
        }
        debug!("{name}: skipping, {reason}");
        return Ok(());
    }
    if !d.dry_run {
        mp.manager.prepare_mutation(&mp.requests).await?;
    }
    let statuses = mp
        .manager
        .installed_with_options(&mp.requests, &mp.options)
        .await?;
    if let Some(reason) = unavailable_package_reason(d, &statuses) {
        bail!("{reason}");
    }
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
        .filter(|status| status.request.desired == PackageDesiredState::Present)
        .filter(|s| match action {
            Action::Install => !s.state.is_installed() && !s.state.is_unavailable(),
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
        .filter(|status| status.request.desired == PackageDesiredState::Present)
        .filter(|status| status.state.is_installed())
        .count();
    if action == Action::Install && installed > 0 {
        info!("{name}: {installed} package(s) already installed");
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
        let remove = remove_targets
            .iter()
            .map(|status| status.request.clone())
            .collect::<Vec<_>>();
        // a removal that fails, or that this manager can never perform, must
        // not hold back its own installs either: record it and carry on
        if let Err(err) = remove_packages(mp, &remove, d, opts).await {
            failures.push(err);
        }
    }
    if targets.is_empty() {
        return Ok(());
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
            return Ok(());
        }
    }
    match action {
        Action::Install => {
            mp.manager
                .install_with_options(&targets, opts, &mp.options)
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
            mp.manager
                .upgrade_with_options(&targets, opts, &mp.options)
                .await?;
            if !d.dry_run {
                let after = mp
                    .manager
                    .installed_with_options(&targets, &mp.options)
                    .await?;
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
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn explicit_packages_reject_unavailable_entries_but_manager_filters_skip_them() {
        let explicit_opts = DriverOpts {
            manager: None,
            explicit: true,
            allow_unavailable_manager: true,
            dry_run: false,
            update: false,
            yes: true,
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
            display_name: None,
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
        };
        assert!(unavailable_manager_is_error(&manager_opts));
        assert_eq!(unavailable_package_reason(&manager_opts, &statuses), None);
    }
}

#[cfg(test)]
mod run_tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A fake manager that records what the driver asks of it.
    #[derive(Default)]
    struct RecordingManager {
        name: &'static str,
        installed: Vec<&'static str>,
        supports_remove: bool,
        fail_remove: bool,
        fail_install: bool,
        installs: Mutex<Vec<String>>,
        removes: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait(?Send)]
    impl crate::system::packages::SystemPackageManager for RecordingManager {
        fn name(&self) -> &str {
            self.name
        }

        fn is_available(&self) -> bool {
            true
        }

        fn unavailable_reason(&self) -> String {
            unreachable!()
        }

        async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
            Ok(pkgs
                .iter()
                .map(|request| PackageStatus {
                    request: request.clone(),
                    state: if self.installed.contains(&request.name.as_str()) {
                        PackageState::Installed {
                            version: "1.0".to_string(),
                        }
                    } else {
                        PackageState::Missing
                    },
                    display_name: None,
                })
                .collect())
        }

        async fn install(&self, pkgs: &[PackageRequest], _opts: &InstallOpts) -> Result<()> {
            if self.fail_install {
                bail!("{} install exploded", self.name);
            }
            let mut installs = self.installs.lock().unwrap();
            installs.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
            Ok(())
        }

        fn supports_remove(&self) -> bool {
            self.supports_remove
        }

        async fn remove(&self, pkgs: &[PackageRequest], _opts: &InstallOpts) -> Result<()> {
            assert!(self.supports_remove, "remove called on {}", self.name);
            let mut removes = self.removes.lock().unwrap();
            removes.extend(pkgs.iter().map(|pkg| pkg.name.clone()));
            if self.fail_remove {
                bail!("{} remove exploded", self.name);
            }
            Ok(())
        }
    }

    fn request(name: &str, desired: PackageDesiredState) -> PackageRequest {
        PackageRequest {
            name: name.to_string(),
            version: None,
            tap_url: None,
            desired,
        }
    }

    fn opts(manager: Option<&str>) -> DriverOpts {
        DriverOpts {
            manager: manager.map(str::to_string),
            explicit: false,
            allow_unavailable_manager: false,
            dry_run: false,
            update: false,
            yes: true,
        }
    }

    /// `first` holds an installed `git` declared absent plus a missing `curl`;
    /// `second` holds a missing `ripgrep`.
    fn two_managers(
        first: RecordingManager,
        second: RecordingManager,
    ) -> (
        Arc<RecordingManager>,
        Arc<RecordingManager>,
        Vec<ManagerPackages>,
    ) {
        let first = Arc::new(RecordingManager {
            name: "first",
            installed: vec!["git"],
            ..first
        });
        let second = Arc::new(RecordingManager {
            name: "second",
            ..second
        });
        let mgrs = vec![
            ManagerPackages {
                manager: first.clone(),
                requests: vec![
                    request("git", PackageDesiredState::Absent),
                    request("curl", PackageDesiredState::Present),
                ],
                options: Default::default(),
                disabled: false,
            },
            ManagerPackages {
                manager: second.clone(),
                requests: vec![request("ripgrep", PackageDesiredState::Present)],
                options: Default::default(),
                disabled: false,
            },
        ];
        (first, second, mgrs)
    }

    #[tokio::test]
    async fn unsupported_removal_does_not_block_installs() {
        let (first, second, mgrs) = two_managers(Default::default(), Default::default());

        run(mgrs, Action::Install, &opts(None)).await.unwrap();

        assert!(first.removes.lock().unwrap().is_empty());
        assert_eq!(*first.installs.lock().unwrap(), ["curl"]);
        assert_eq!(*second.installs.lock().unwrap(), ["ripgrep"]);
    }

    #[tokio::test]
    async fn unsupported_removal_fails_a_scoped_run_after_installing() {
        let (first, second, mgrs) = two_managers(Default::default(), Default::default());

        let err = run(mgrs, Action::Install, &opts(Some("first")))
            .await
            .unwrap_err();

        assert!(
            format!("{err:#}").contains("first does not support declarative package removal"),
            "{err:#}"
        );
        assert!(first.removes.lock().unwrap().is_empty());
        assert_eq!(*first.installs.lock().unwrap(), ["curl"]);
        assert!(second.installs.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_removal_does_not_block_installs() {
        let (first, second, mgrs) = two_managers(
            RecordingManager {
                supports_remove: true,
                fail_remove: true,
                ..Default::default()
            },
            Default::default(),
        );

        let err = run(mgrs, Action::Install, &opts(None)).await.unwrap_err();

        assert_eq!(format!("{err:#}"), "first remove exploded");
        assert_eq!(*first.removes.lock().unwrap(), ["git"]);
        assert_eq!(*first.installs.lock().unwrap(), ["curl"]);
        assert_eq!(*second.installs.lock().unwrap(), ["ripgrep"]);
    }

    #[tokio::test]
    async fn failed_install_does_not_block_later_managers() {
        let (first, second, mgrs) = two_managers(
            RecordingManager {
                supports_remove: true,
                fail_install: true,
                ..Default::default()
            },
            Default::default(),
        );

        let err = run(mgrs, Action::Install, &opts(None)).await.unwrap_err();

        assert_eq!(format!("{err:#}"), "first install exploded");
        assert_eq!(*first.removes.lock().unwrap(), ["git"]);
        assert_eq!(*second.installs.lock().unwrap(), ["ripgrep"]);
    }

    #[tokio::test]
    async fn every_failure_is_reported() {
        let (first, second, mgrs) = two_managers(
            RecordingManager {
                supports_remove: true,
                fail_remove: true,
                fail_install: true,
                ..Default::default()
            },
            RecordingManager {
                fail_install: true,
                ..Default::default()
            },
        );

        let err = run(mgrs, Action::Install, &opts(None)).await.unwrap_err();

        assert_eq!(
            format!("{err:#}"),
            "3 package manager operations failed:\n  \
             first: first remove exploded\n  \
             first: first install exploded\n  \
             second: second install exploded"
        );
        assert_eq!(*first.removes.lock().unwrap(), ["git"]);
        assert!(second.installs.lock().unwrap().is_empty());
    }
}
