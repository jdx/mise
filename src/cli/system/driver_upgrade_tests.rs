use super::*;
use crate::system::ManagerPackageOptions;
use crate::system::packages::{
    PackageRequest, PackageUpgradeOutcome, PackageUpgradeResult, SystemPackageManager,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct LegacyManager {
    calls: Mutex<Vec<&'static str>>,
    changed: bool,
    fail: bool,
}

#[async_trait(?Send)]
impl SystemPackageManager for LegacyManager {
    fn name(&self) -> &str {
        "legacy"
    }
    fn is_available(&self) -> bool {
        true
    }
    fn unavailable_reason(&self) -> String {
        unreachable!()
    }
    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        let mut calls = self.calls.lock().unwrap();
        let upgraded = calls.contains(&"upgrade");
        calls.push("installed");
        let version = if upgraded && self.changed { "2" } else { "1" };
        Ok(pkgs
            .iter()
            .map(|request| PackageStatus {
                request: request.clone(),
                state: PackageState::Installed {
                    version: version.into(),
                },
            })
            .collect())
    }
    async fn install(&self, _: &[PackageRequest], _: &InstallOpts) -> Result<()> {
        panic!("driver must invoke upgrade")
    }
    async fn upgrade(&self, _: &[PackageRequest], _: &InstallOpts) -> Result<()> {
        self.calls.lock().unwrap().push("upgrade");
        if self.fail {
            bail!("upgrade failed");
        }
        Ok(())
    }
}

struct ReportingManager {
    legacy: LegacyManager,
    expected: Vec<PackageRequest>,
    results: Mutex<Option<Vec<PackageUpgradeResult>>>,
    dry_run: bool,
}

#[async_trait(?Send)]
impl SystemPackageManager for ReportingManager {
    fn name(&self) -> &str {
        "brew-cask"
    }
    fn is_available(&self) -> bool {
        true
    }
    fn unavailable_reason(&self) -> String {
        unreachable!()
    }
    async fn installed(&self, pkgs: &[PackageRequest]) -> Result<Vec<PackageStatus>> {
        assert!(
            self.legacy.calls.lock().unwrap().is_empty(),
            "Some report must avoid post-upgrade installed queries"
        );
        self.legacy.installed(pkgs).await
    }
    async fn install(&self, _: &[PackageRequest], _: &InstallOpts) -> Result<()> {
        panic!("unexpected install")
    }
    async fn upgrade(&self, _: &[PackageRequest], _: &InstallOpts) -> Result<()> {
        panic!("reporting path must execute once through upgrade_with_report")
    }
    async fn upgrade_with_report(
        &self,
        pkgs: &[PackageRequest],
        opts: &InstallOpts,
    ) -> Result<Option<Vec<PackageUpgradeResult>>> {
        assert_eq!(pkgs, self.expected);
        assert_eq!(opts.dry_run, self.dry_run);
        self.legacy.calls.lock().unwrap().push("report");
        Ok(Some(
            self.results
                .lock()
                .unwrap()
                .take()
                .expect("report invoked once"),
        ))
    }
}

fn request(name: &str) -> PackageRequest {
    PackageRequest {
        name: name.into(),
        version: None,
        tap_url: None,
        desired: PackageDesiredState::Present,
    }
}

fn options(dry_run: bool) -> DriverOpts {
    DriverOpts {
        manager: None,
        explicit: true,
        allow_unavailable_manager: false,
        dry_run,
        update: false,
        yes: true,
    }
}

fn group(manager: Arc<dyn SystemPackageManager>, requests: Vec<PackageRequest>) -> ManagerPackages {
    ManagerPackages {
        manager,
        requests,
        options: ManagerPackageOptions::None,
        disabled: false,
    }
}

fn output_since(start: usize) -> String {
    crate::output::tests::STDERR.lock().unwrap()[start..].join("\n")
}

#[test]
fn default_upgrade_report_delegates_once_and_propagates_errors() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    for fail in [false, true] {
        let manager = LegacyManager {
            fail,
            ..Default::default()
        };
        let result = runtime
            .block_on(manager.upgrade_with_report(&[request("example")], &InstallOpts::default()));
        if fail {
            assert!(result.unwrap_err().to_string().contains("upgrade failed"));
        } else {
            assert!(result?.is_none());
        }
        assert_eq!(*manager.calls.lock().unwrap(), vec!["upgrade"]);
    }
    Ok(())
}

#[test]
fn driver_uses_legacy_state_comparison_only_for_none_reports() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    for (changed, dry_run, fail) in [
        (true, false, false),
        (false, false, false),
        (true, true, false),
        (false, false, true),
    ] {
        let manager = Arc::new(LegacyManager {
            changed,
            fail,
            ..Default::default()
        });
        let start = crate::output::tests::STDERR.lock().unwrap().len();
        let result = runtime.block_on(run(
            vec![group(manager.clone(), vec![request("example")])],
            Action::Upgrade,
            &options(dry_run),
        ));
        if fail {
            assert!(result.is_err());
        } else {
            result?;
        }
        let expected = if dry_run || fail {
            vec!["installed", "upgrade"]
        } else {
            vec!["installed", "upgrade", "installed"]
        };
        assert_eq!(*manager.calls.lock().unwrap(), expected);
        let output = output_since(start);
        if !dry_run && !fail {
            let expected = if changed {
                "legacy: upgraded example 1 -> 2"
            } else {
                "legacy: already up to date"
            };
            assert!(output.contains(expected), "{output}");
        } else {
            assert!(!output.contains("already up to date"), "{output}");
        }
    }
    Ok(())
}

#[test]
fn driver_reports_mixed_equal_unknown_and_homebrew_batches_in_input_order() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    for (case, dry_run) in [
        ("mixed", false),
        ("mixed", true),
        ("unknown", false),
        ("equal", false),
        ("Homebrew", false),
    ] {
        let entries = match case {
            "mixed" => vec![
                (
                    "older",
                    if dry_run {
                        PackageUpgradeOutcome::WouldUpgrade {
                            from: "9.2.3".into(),
                            to: "9.2.6".into(),
                        }
                    } else {
                        PackageUpgradeOutcome::Upgraded {
                            from: "9.2.3".into(),
                            to: "9.2.6".into(),
                        }
                    },
                ),
                (
                    "equal",
                    PackageUpgradeOutcome::UpToDate {
                        version: "9.2.6".into(),
                    },
                ),
                (
                    "newer",
                    PackageUpgradeOutcome::Skipped {
                        reason: "live version 9.2.7 is newer than cask 9.2.6".into(),
                    },
                ),
                (
                    "unknown",
                    PackageUpgradeOutcome::Skipped {
                        reason: "cannot determine live version: missing CFBundleShortVersionString"
                            .into(),
                    },
                ),
                (
                    "external",
                    PackageUpgradeOutcome::Skipped {
                        reason: "managed by Homebrew".into(),
                    },
                ),
            ],
            "unknown" => vec![(
                "unknown",
                PackageUpgradeOutcome::Skipped {
                    reason: "cannot determine live version: missing CFBundleShortVersionString"
                        .into(),
                },
            )],
            "equal" => vec![(
                "equal",
                PackageUpgradeOutcome::UpToDate {
                    version: "9.2.6".into(),
                },
            )],
            "Homebrew" => vec![(
                "external",
                PackageUpgradeOutcome::Skipped {
                    reason: "managed by Homebrew".into(),
                },
            )],
            _ => unreachable!(),
        };
        let requests = entries
            .iter()
            .map(|(name, _)| request(name))
            .collect::<Vec<_>>();
        let results = entries
            .into_iter()
            .map(|(name, outcome)| PackageUpgradeResult {
                request: request(name),
                outcome,
            })
            .collect();
        let manager = Arc::new(ReportingManager {
            legacy: LegacyManager::default(),
            expected: requests.clone(),
            results: Mutex::new(Some(results)),
            dry_run,
        });
        let start = crate::output::tests::STDERR.lock().unwrap().len();
        runtime.block_on(run(
            vec![group(manager.clone(), requests.clone())],
            Action::Upgrade,
            &options(dry_run),
        ))?;
        assert_eq!(
            *manager.legacy.calls.lock().unwrap(),
            vec!["installed", "report"]
        );
        let output = output_since(start);
        let mut last_position = None;
        for req in requests {
            let marker = format!("brew-cask:{}:", req.name);
            assert_eq!(output.matches(&marker).count(), 1, "{case}: {output}");
            let position = output.find(&marker).unwrap();
            if let Some(last) = last_position {
                assert!(position > last, "{output}");
            }
            last_position = Some(position);
        }
        let summary = match (case, dry_run) {
            ("mixed", true) => "brew-cask: 1 would upgrade, 1 up to date, 3 skipped",
            ("mixed", false) => "brew-cask: 1 upgraded, 1 up to date, 3 skipped",
            ("equal", _) => "brew-cask: 0 upgraded, 1 up to date, 0 skipped",
            _ => "brew-cask: 0 upgraded, 0 up to date, 1 skipped",
        };
        assert!(output.contains(summary), "{case}: {output}");
        assert!(
            !output.contains("brew-cask: already up to date"),
            "{output}"
        );
        if case == "mixed" {
            let verb = if dry_run { "would upgrade" } else { "upgraded" };
            assert!(
                output.contains(&format!("brew-cask:older: {verb} 9.2.3 -> 9.2.6")),
                "{output}"
            );
            assert!(
                output.contains(
                    "brew-cask:newer: skipped: live version 9.2.7 is newer than cask 9.2.6"
                ),
                "{output}"
            );
        }
    }
    Ok(())
}
