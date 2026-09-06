use super::*;
use crate::config::{Settings, settings::SettingsPartial};
use crate::system::packages::PackageDesiredState;
use confique::Layer;

struct UpgradeFixture {
    root: tempfile::TempDir,
    server: mockito::ServerGuard,
    mocks: Vec<mockito::Mock>,
    request: PackageRequest,
    metadata: Value,
    receipt: PathBuf,
    archive: Vec<u8>,
    initial_links: Vec<(PathBuf, PathBuf)>,
    _env: EnvVarGuard,
}

impl UpgradeFixture {
    fn new(live: &str, recorded: &str, distributed: &str) -> Result<Self> {
        let root = trusted_tempdir()?;
        let prefix = root.path().join("brew");
        let app = prefix.join("Applications/Example.app");
        write_app(&app, live, "original")?;
        let payload = root.path().join("payload/Example.app");
        write_app(&payload, distributed, "distribution")?;
        let archive_path = root.path().join("app.zip");
        let output = std::process::Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&payload)
            .arg(&archive_path)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let archive = std::fs::read(archive_path)?;
        let server = mockito::Server::new();
        let token = format!(
            "upgrade-{}",
            root.path()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .trim_start_matches('.')
        );
        let mut env = EnvVarGuard::new();
        env.set("MISE_SYSTEM_BREW_PREFIX", &prefix);
        env.set(APP_DIR_ENV, prefix.join("Applications"));
        let mut settings = SettingsPartial::empty();
        settings.url_replacements = Some(
            [("https://formulae.brew.sh".into(), server.url())]
                .into_iter()
                .collect(),
        );
        let caskroom = caskroom_version_dir(&token, recorded);
        std::fs::create_dir_all(&caskroom)?;
        std::os::unix::fs::symlink(&app, caskroom.join("Example.app"))?;
        let previous_stage = root.path().join("previous-stage");
        std::fs::create_dir_all(&previous_stage)?;
        std::fs::write(
            previous_stage.join("obsolete"),
            "previous standalone binary",
        )?;
        let apps = [AppArtifact {
            source: "Example.app".into(),
            target: None,
        }];
        let binaries = [
            BinaryArtifact {
                source: "$APPDIR/Example.app/Contents/payload".into(),
                target: Some("example".into()),
            },
            BinaryArtifact {
                source: "obsolete".into(),
                target: Some("obsolete".into()),
            },
        ];
        let previous_cask = test_cask(&token, recorded);
        let appdir = app.parent().unwrap();
        durabilize_stage_payload(&previous_stage, &caskroom, &apps)?;
        let mut initial_links = Vec::new();
        for binary in &binaries {
            stage_binary(&previous_stage, &caskroom, &previous_cask, &apps, binary)?;
            link_binary(&caskroom, appdir, binary)?;
            let target = binary.target_path(appdir)?;
            initial_links.push((target.clone(), std::fs::read_link(target)?));
        }
        let links = initial_links
            .iter()
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        let targets = std::iter::once(app.clone())
            .chain(links.iter().cloned())
            .map(|path| {
                Ok(CaskTargetRecord {
                    fingerprint: cask_target_fingerprint(&path)?,
                    path,
                    uninstall: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let receipt_value: CaskReceipt = serde_json::from_value(serde_json::json!({
            "schema_version": 3, "version": recorded, "auto_updates": true,
            "apps": [app], "metadata_only_apps": [app], "binaries": links,
            "targets": targets,
            "prune_blocker": "metadata-only app ownership cannot be proven safely during pruning"
        }))?;
        let receipt = caskroom.join(".mise-cask.toml");
        std::fs::write(&receipt, toml::to_string_pretty(&receipt_value)?)?;
        let metadata = serde_json::json!({
            "token": token, "version": "9.2.6", "auto_updates": true,
            "url": format!("{}/app.zip", server.url()), "sha256": "no_check",
            "artifacts": [{"app": ["Example.app"]}, {"binary": ["Example.app/Contents/payload", {"target": "example"}]}]
        });
        Settings::reset(Some(settings));
        Ok(Self {
            request: PackageRequest {
                name: token,
                version: None,
                tap_url: None,
                desired: PackageDesiredState::Present,
            },
            root,
            server,
            mocks: Vec::new(),
            metadata,
            receipt,
            archive,
            initial_links,
            _env: env,
        })
    }

    fn publish(&mut self, downloads: usize) {
        self.mocks.push(
            self.server
                .mock(
                    "GET",
                    format!("/api/cask/{}.json", self.request.name).as_str(),
                )
                .with_header("content-type", "application/json")
                .with_body(self.metadata.to_string())
                .expect(1)
                .create(),
        );
        self.mocks.push(
            self.server
                .mock("GET", "/app.zip")
                .with_body(self.archive.clone())
                .expect(downloads)
                .create(),
        );
    }

    fn prefix(&self) -> PathBuf {
        self.root.path().join("brew")
    }
    fn app(&self) -> PathBuf {
        self.prefix().join("Applications/Example.app")
    }
    fn journal(&self) -> PathBuf {
        cask_journal_path_in(&crate::dirs::STATE, &self.request.name, "9.2.6")
    }

    fn run(
        &self,
        mode: InstallMode,
        dry_run: bool,
        hook: impl FnMut(InstallTestEvent) -> Result<()>,
    ) -> Result<()> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(BrewCaskManager::new().install_one_with_test_hook(
                &self.request,
                &InstallOpts {
                    dry_run,
                    update: false,
                },
                mode,
                hook,
            ))
    }

    fn assert_requests(&self) {
        for mock in &self.mocks {
            mock.assert();
        }
    }

    fn assert_links_unchanged(&self) -> Result<()> {
        for (path, destination) in &self.initial_links {
            assert_eq!(
                &std::fs::read_link(path)?,
                destination,
                "{}",
                path.display()
            );
        }
        Ok(())
    }
}

impl Drop for UpgradeFixture {
    fn drop(&mut self) {
        Settings::reset(None);
    }
}

fn write_app(app: &Path, version: &str, payload: &str) -> Result<()> {
    std::fs::create_dir_all(app.join("Contents"))?;
    let mut plist = plist::Dictionary::new();
    plist.insert("CFBundleShortVersionString".into(), version.into());
    plist::Value::Dictionary(plist).to_file_xml(app.join("Contents/Info.plist"))?;
    std::fs::write(app.join("Contents/payload"), payload)?;
    std::fs::set_permissions(
        app.join("Contents/payload"),
        std::fs::Permissions::from_mode(0o755),
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boundary {
    BeforeLock,
    BeforeActivate,
}

fn event_paths(event: InstallTestEvent) -> (Boundary, PathBuf, Option<PathBuf>) {
    match event {
        InstallTestEvent::BeforeLock { stage } => (Boundary::BeforeLock, stage, None),
        InstallTestEvent::BeforeActivate {
            stage,
            prepared_app,
        } => (Boundary::BeforeActivate, stage, Some(prepared_app)),
    }
}

#[test]
fn upgrade_cancels_observed_live_changes_before_lock_and_activation() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    for boundary in [Boundary::BeforeLock, Boundary::BeforeActivate] {
        for change in ["equal", "newer", "unreadable", "missing"] {
            let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
            fixture.publish(1);
            let receipt = std::fs::read(&fixture.receipt)?;
            let mut observed = Vec::new();
            let mut temporary_paths = Vec::new();
            fixture.run(InstallMode::Upgrade, false, |event| {
                let (phase, stage, prepared) = event_paths(event);
                observed.push(phase);
                temporary_paths.push(stage);
                temporary_paths.extend(prepared);
                if phase == boundary {
                    match change {
                        "equal" => write_app(&fixture.app(), "9.2.6", "self-updated")?,
                        "newer" => write_app(&fixture.app(), "9.2.7", "self-updated")?,
                        "unreadable" => std::fs::write(
                            fixture.app().join("Contents/Info.plist"),
                            "invalid plist",
                        )?,
                        "missing" => std::fs::remove_dir_all(fixture.app())?,
                        _ => unreachable!(),
                    }
                }
                Ok(())
            })?;
            assert!(observed.contains(&boundary), "{boundary:?}, {change}");
            if boundary == Boundary::BeforeLock {
                assert_eq!(observed, vec![Boundary::BeforeLock]);
            }
            assert_eq!(
                std::fs::read(&fixture.receipt)?,
                receipt,
                "{boundary:?}, {change}"
            );
            fixture.assert_links_unchanged()?;
            if change == "missing" {
                assert!(!fixture.app().exists());
            } else {
                let expected = if matches!(change, "equal" | "newer") {
                    "self-updated"
                } else {
                    "original"
                };
                assert_eq!(
                    std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
                    expected
                );
            }
            for path in temporary_paths {
                assert!(
                    !path.exists(),
                    "cancelled temporary path remains: {}",
                    path.display()
                );
            }
            assert!(!fixture.journal().exists());
            fixture.assert_requests();
        }
    }
    Ok(())
}

#[test]
fn upgrade_rechecks_receipt_and_homebrew_ownership_at_both_boundaries() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    for boundary in [Boundary::BeforeLock, Boundary::BeforeActivate] {
        for owner in ["changed receipt", "missing receipt", "Homebrew"] {
            let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
            fixture.publish(1);
            let mut expected_receipt = std::fs::read(&fixture.receipt)?;
            let mut reached = false;
            let result = fixture.run(InstallMode::Upgrade, false, |event| {
                let (phase, _, _) = event_paths(event);
                if phase == boundary {
                    reached = true;
                    if owner == "Homebrew" {
                        std::fs::create_dir_all(
                            caskroom_token_dir(&fixture.request.name).join(".metadata"),
                        )?;
                    } else if owner == "missing receipt" {
                        std::fs::remove_file(&fixture.receipt)?;
                    } else {
                        let mut receipt: CaskReceipt =
                            toml::from_str(std::str::from_utf8(&expected_receipt)?)?;
                        receipt.auto_updates = false;
                        expected_receipt = toml::to_string_pretty(&receipt)?.into_bytes();
                        std::fs::write(&fixture.receipt, &expected_receipt)?;
                    }
                }
                Ok(())
            });
            assert!(reached, "{boundary:?}, {owner}");
            if owner == "Homebrew" {
                assert!(result.is_err(), "Homebrew takeover at {boundary:?}");
                assert!(format!("{:#}", result.unwrap_err()).contains("Homebrew"));
            } else {
                result?;
            }
            if owner == "missing receipt" {
                assert!(!fixture.receipt.exists());
            } else {
                assert_eq!(std::fs::read(&fixture.receipt)?, expected_receipt);
            }
            assert_eq!(
                std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
                "original"
            );
            fixture.assert_links_unchanged()?;
            fixture.assert_requests();
        }
    }
    Ok(())
}

#[test]
fn upgrade_validates_staged_version_before_copy_or_replacement() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    for version in ["9.2.2", "9.2.7", "9.2", "9.2.6,123", "missing"] {
        let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", version)?;
        fixture.publish(1);
        let receipt = std::fs::read(&fixture.receipt)?;
        let mut copied = false;
        let result = fixture.run(InstallMode::Upgrade, false, |event| {
            let (phase, stage, _) = event_paths(event);
            if phase == Boundary::BeforeActivate {
                copied = true;
            }
            if phase == Boundary::BeforeLock && version == "missing" {
                std::fs::remove_file(stage.join("Example.app/Contents/Info.plist"))?;
            }
            Ok(())
        });
        assert!(result.is_err(), "invalid distribution {version}");
        assert!(!copied, "invalid distribution copied: {version}");
        assert_eq!(std::fs::read(&fixture.receipt)?, receipt);
        assert_eq!(
            std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
            "original"
        );
        fixture.assert_links_unchanged()?;
        fixture.assert_requests();
    }
    Ok(())
}

#[test]
fn upgrade_replaces_app_with_equal_receipt_and_preserves_metadata_only_ownership() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    for recorded in ["9.2.3", "9.2.6"] {
        let mut fixture = UpgradeFixture::new("9.2.3", recorded, "09.002.006")?;
        fixture.publish(1);
        let mut temporary_paths = Vec::new();
        fixture.run(InstallMode::Upgrade, false, |event| {
            let (phase, stage, prepared) = event_paths(event);
            temporary_paths.push(stage);
            temporary_paths.extend(prepared);
            if phase == Boundary::BeforeActivate {
                fixture.assert_links_unchanged()?;
                assert_eq!(
                    std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
                    "original"
                );
            }
            Ok(())
        })?;
        assert_eq!(
            std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
            "distribution"
        );
        let caskroom = caskroom_version_dir(&fixture.request.name, "9.2.6");
        let receipt: CaskReceipt =
            toml::from_str(&std::fs::read_to_string(caskroom.join(".mise-cask.toml"))?)?;
        assert_eq!(receipt.version, "9.2.6");
        assert!(receipt.auto_updates);
        assert_eq!(receipt.metadata_only_apps, vec![fixture.app()]);
        assert_eq!(
            std::fs::read_link(caskroom.join("Example.app"))?,
            fixture.app()
        );
        assert!(
            fixture
                .prefix()
                .join("bin/obsolete")
                .symlink_metadata()
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(fixture.prefix().join("bin/example"))?,
            "distribution"
        );
        for path in temporary_paths {
            assert!(!path.exists(), "temporary path remains: {}", path.display());
        }
        assert!(!fixture.journal().exists());
        fixture.assert_requests();
    }
    Ok(())
}

#[test]
fn install_and_ineligible_or_dry_run_upgrades_do_not_fetch_or_change_artifacts() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    for case in [
        "install",
        "equal",
        "newer",
        "unknown",
        "unsupported",
        "dry run",
    ] {
        let live = match case {
            "equal" => "9.2.6",
            "newer" => "9.2.7",
            "unknown" => "9.2.3,123",
            _ => "9.2.3",
        };
        let mut fixture = UpgradeFixture::new(live, "9.2.6", "9.2.6")?;
        if case == "unsupported" {
            fixture.metadata["artifacts"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({"font": ["Example.ttf"]}));
        }
        fixture.metadata["depends_on"] =
            serde_json::json!({"cask": ["must-not-install-dependency"]});
        fixture.publish(0);
        let receipt = std::fs::read(&fixture.receipt)?;
        let mode = if case == "install" {
            InstallMode::Install
        } else {
            InstallMode::Upgrade
        };
        fixture.run(mode, case == "dry run", |_| {
            panic!("{case}: reached stage or copy boundary")
        })?;
        assert!(
            !fixture.prefix().join("Caskroom/.mise.lock").exists(),
            "{case}"
        );
        assert_eq!(std::fs::read(&fixture.receipt)?, receipt, "{case}");
        fixture.assert_links_unchanged()?;
        assert_eq!(
            std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
            "original"
        );
        assert!(!fixture.journal().exists());
        fixture.assert_requests();
    }
    Ok(())
}

#[test]
fn activation_failure_restores_original_app_and_propagates_error() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
    fixture.publish(1);
    let receipt = std::fs::read(&fixture.receipt)?;
    let mut reached = false;
    let result = fixture.run(InstallMode::Upgrade, false, |event| {
        if let InstallTestEvent::BeforeActivate { prepared_app, .. } = event {
            reached = true;
            std::fs::remove_dir_all(prepared_app)?;
        }
        Ok(())
    });
    assert!(reached);
    assert!(result.is_err());
    assert_eq!(std::fs::read(&fixture.receipt)?, receipt);
    assert_eq!(
        std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
        "original"
    );
    fixture.assert_links_unchanged()?;
    fixture.assert_requests();
    Ok(())
}

#[test]
fn explicit_upgrade_keeps_auto_updating_dependencies_in_install_mode() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
    let token = format!("{}-dependency", fixture.request.name);
    let app = fixture.prefix().join("Applications/Dependency.app");
    write_app(&app, "9.2.3", "dependency original")?;
    let mut receipt: CaskReceipt = toml::from_str(&std::fs::read_to_string(&fixture.receipt)?)?;
    receipt.apps = vec![app.clone()];
    receipt.metadata_only_apps = vec![app.clone()];
    receipt.binaries.clear();
    receipt.targets = vec![CaskTargetRecord {
        path: app.clone(),
        fingerprint: cask_target_fingerprint(&app)?,
        uninstall: None,
    }];
    let caskroom = caskroom_version_dir(&token, "9.2.6");
    std::fs::create_dir_all(&caskroom)?;
    std::os::unix::fs::symlink(&app, caskroom.join("Dependency.app"))?;
    let receipt_path = caskroom.join(".mise-cask.toml");
    let receipt_bytes = toml::to_string_pretty(&receipt)?;
    std::fs::write(&receipt_path, &receipt_bytes)?;
    fixture.metadata["depends_on"] = serde_json::json!({"cask": [token]});
    let metadata = serde_json::json!({
        "token": token, "version": "9.2.6", "auto_updates": true,
        "url": format!("{}/dependency.zip", fixture.server.url()), "sha256": "no_check",
        "artifacts": [{"app": ["Dependency.app"]}]
    });
    fixture.mocks.push(
        fixture
            .server
            .mock("GET", format!("/api/cask/{token}.json").as_str())
            .with_header("content-type", "application/json")
            .with_body(metadata.to_string())
            .expect(1)
            .create(),
    );
    fixture.mocks.push(
        fixture
            .server
            .mock("GET", "/dependency.zip")
            .with_status(500)
            .expect(0)
            .create(),
    );
    fixture.publish(1);
    fixture.run(InstallMode::Upgrade, false, |_| Ok(()))?;
    assert_eq!(std::fs::read_to_string(&receipt_path)?, receipt_bytes);
    assert_eq!(
        std::fs::read_to_string(app.join("Contents/payload"))?,
        "dependency original"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
        "distribution"
    );
    fixture.assert_requests();
    Ok(())
}

#[test]
fn cancellation_removes_current_journal_and_preserves_other_recovery_journal() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
    fixture.publish(1);
    let other_path = cask_journal_path_in(&crate::dirs::STATE, &fixture.request.name, "8.0.0");
    let other_bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "token": fixture.request.name, "version": "8.0.0", "completed": ["app[0]"]
    }))?;
    let mut reached = false;
    fixture.run(InstallMode::Upgrade, false, |event| {
        if let InstallTestEvent::BeforeActivate { .. } = event {
            reached = true;
            assert!(fixture.journal().exists());
            std::fs::write(&other_path, &other_bytes)?;
            write_app(&fixture.app(), "9.2.6", "self-updated")?;
        }
        Ok(())
    })?;
    assert!(reached);
    assert!(!fixture.journal().exists());
    assert_eq!(std::fs::read(&other_path)?, other_bytes);
    std::fs::remove_file(other_path)?;
    fixture.assert_requests();
    Ok(())
}

fn set_user_immutable(path: &Path, enabled: bool) -> Result<()> {
    let flag = if enabled { "uchg" } else { "nouchg" };
    let output = std::process::Command::new("/usr/bin/chflags")
        .arg(flag)
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!(
            "chflags {flag} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[test]
fn cancellation_cleanup_failure_propagates_without_changing_owned_artifacts() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut fixture = UpgradeFixture::new("9.2.3", "9.2.6", "9.2.6")?;
    fixture.publish(1);
    let receipt = std::fs::read(&fixture.receipt)?;
    let mut immutable = None;
    let result = fixture.run(InstallMode::Upgrade, false, |event| {
        if let InstallTestEvent::BeforeActivate { prepared_app, .. } = event {
            write_app(&fixture.app(), "9.2.6", "self-updated")?;
            set_user_immutable(&prepared_app, true)?;
            immutable = Some(prepared_app);
        }
        Ok(())
    });
    let path = immutable.expect("before-activation callback must run");
    set_user_immutable(&path, false)?;
    assert!(
        result.is_err(),
        "failed cancellation cleanup must propagate"
    );
    assert_eq!(std::fs::read(&fixture.receipt)?, receipt);
    assert_eq!(
        std::fs::read_to_string(fixture.app().join("Contents/payload"))?,
        "self-updated"
    );
    fixture.assert_links_unchanged()?;
    fixture.assert_requests();
    Ok(())
}
