use super::super::upgrade::{UpgradeDecision, compare_live_version, read_live_version_from};
#[cfg(unix)]
use super::super::upgrade::{assess_auto_update, read_live_version_at};
#[cfg(unix)]
use super::*;
use crate::result::Result;

fn assert_unknown(decision: UpgradeDecision, reason: &str) {
    match decision {
        UpgradeDecision::Unknown { reason: actual } => {
            assert!(actual.to_string().contains(reason), "{actual}")
        }
        other => panic!("expected unknown containing {reason:?}, got {other:?}"),
    }
}

#[test]
fn comparison_orders_numeric_components_and_preserves_original_strings() {
    use std::cmp::Ordering::{Equal, Greater, Less};

    let huge_live = format!("1.{}", "9".repeat(80));
    let huge_available = format!("1.1{}", "0".repeat(80));
    for (case, live, available, ordering) in [
        ("older", "9.2.3", "9.2.6", Less),
        ("equal", "4.52.155", "4.52.155", Equal),
        ("newer", "2.1.0", "2.0.0", Greater),
        ("leading zeroes", "01.002.3", "1.2.3", Equal),
        ("all zeroes", "000.00", "0.0", Equal),
        ("numeric ordering", "1.9", "1.10", Less),
        ("single integer", "99", "100", Less),
        (
            "huge components",
            huge_live.as_str(),
            huge_available.as_str(),
            Less,
        ),
    ] {
        let expected = match ordering {
            Less => UpgradeDecision::Older {
                live: live.into(),
                available: available.into(),
            },
            Equal => UpgradeDecision::Equal {
                live: live.into(),
                available: available.into(),
            },
            Greater => UpgradeDecision::Newer {
                live: live.into(),
                available: available.into(),
            },
        };
        assert_eq!(
            compare_live_version(live, available),
            expected,
            "{case}: {live:?} vs {available:?}"
        );
    }
}

#[test]
fn comparison_rejects_unsupported_versions_on_either_side() {
    for (case, invalid) in [
        ("empty", ""),
        ("comma", "1.2,3"),
        ("suffix", "1.2rc1"),
        ("leading space", " 1.2"),
        ("trailing space", "1.2 "),
        ("prefix", "v1.2"),
        ("plus", "+1.2"),
        ("negative", "-1.2"),
        ("empty component", "1..2"),
        ("leading dot", ".1.2"),
        ("trailing dot", "1.2."),
        ("non-ASCII digits", "１.２"),
        ("latest", "latest"),
        ("different component count", "1.2.0"),
    ] {
        for (live, available) in [(invalid, "1.2"), ("1.2", invalid)] {
            let decision = compare_live_version(live, available);
            assert!(
                matches!(&decision, UpgradeDecision::Unknown { reason } if reason.to_string().contains("compar")),
                "{case}: {live:?} vs {available:?}: {decision:?}"
            );
        }
    }
}

#[cfg(unix)]
fn write_version(app: &Path, value: plist::Value, binary: bool) -> Result<()> {
    std::fs::create_dir_all(app.join("Contents"))?;
    let mut dict = plist::Dictionary::new();
    dict.insert("CFBundleShortVersionString".into(), value);
    let value = plist::Value::Dictionary(dict);
    if binary {
        value.to_file_binary(app.join("Contents/Info.plist"))?;
    } else {
        value.to_file_xml(app.join("Contents/Info.plist"))?;
    }
    Ok(())
}

#[cfg(unix)]
fn read_fixture(appdir: &Path) -> Result<UpgradeDecision> {
    let parent = open_trusted_appdir_readonly(appdir)?;
    read_live_version_at(&parent, std::ffi::OsStr::new("Example.app"), "9.2.6")
}

fn plist_reader(value: plist::Value, binary: bool) -> Result<std::io::Cursor<Vec<u8>>> {
    let mut bytes = Vec::new();
    if binary {
        value.to_writer_binary(&mut bytes)?;
    } else {
        value.to_writer_xml(&mut bytes)?;
    }
    Ok(std::io::Cursor::new(bytes))
}

#[test]
fn plist_reads_xml_and_binary_preserving_original_version() -> Result<()> {
    for binary in [false, true] {
        let mut dict = plist::Dictionary::new();
        dict.insert("CFBundleShortVersionString".into(), "09.002.003".into());
        let reader = plist_reader(plist::Value::Dictionary(dict), binary)?;
        assert_eq!(
            read_live_version_from(reader, "9.2.6"),
            UpgradeDecision::Older {
                live: "09.002.003".into(),
                available: "9.2.6".into()
            },
            "binary encoding: {binary}"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn plist_missing_app_is_distinct_from_missing_plist() -> Result<()> {
    let tmp = trusted_tempdir()?;
    assert_unknown(read_fixture(tmp.path())?, "app");
    std::fs::create_dir_all(tmp.path().join("Example.app/Contents"))?;
    assert_unknown(read_fixture(tmp.path())?, "plist");
    Ok(())
}

#[test]
fn plist_rejects_corruption() {
    assert_unknown(
        read_live_version_from(std::io::Cursor::new(b"broken plist"), "9.2.6"),
        "plist",
    );
}

#[test]
fn plist_requires_short_version_string_without_build_version_fallback() -> Result<()> {
    let mut dict = plist::Dictionary::new();
    dict.insert("CFBundleVersion".into(), "9.2.3".into());
    assert_unknown(
        read_live_version_from(
            plist_reader(plist::Value::Dictionary(dict.clone()), false)?,
            "9.2.6",
        ),
        "missing CFBundleShortVersionString",
    );
    for value in [
        plist::Value::Integer(923.into()),
        plist::Value::Boolean(true),
        plist::Value::Array(vec![]),
    ] {
        dict.insert("CFBundleShortVersionString".into(), value);
        assert_unknown(
            read_live_version_from(
                plist_reader(plist::Value::Dictionary(dict.clone()), false)?,
                "9.2.6",
            ),
            "string",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn plist_permission_denied_is_unknown() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        assert_unknown(
            read_live_version_from(
                PermissionDeniedPlist(std::io::Cursor::new(Vec::new())),
                "9.2.6",
            ),
            "read",
        );
        return Ok(());
    }
    let tmp = trusted_tempdir()?;
    let app = tmp.path().join("Example.app");
    write_version(&app, "9.2.3".into(), false)?;
    let path = app.join("Contents/Info.plist");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))?;
    let result = read_fixture(tmp.path());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    assert_unknown(result?, "read");
    Ok(())
}

#[cfg(unix)]
#[test]
fn readonly_appdir_preserves_absence_and_rejects_untrusted_directory() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let missing = tmp.path().join("missing/Applications");
    assert!(open_trusted_appdir_readonly(&missing).is_err());
    assert!(!tmp.path().join("missing").exists());
    let untrusted = tmp.path().join("untrusted");
    std::fs::create_dir(&untrusted)?;
    std::fs::set_permissions(&untrusted, std::fs::Permissions::from_mode(0o777))?;
    assert!(open_trusted_appdir_readonly(&untrusted).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn readonly_appdir_rejects_symlink_and_relative_path() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(tmp.path(), &link)?;
    assert!(open_trusted_appdir_readonly(&link).is_err());
    assert!(open_trusted_appdir_readonly(Path::new("relative/Applications")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn plist_reads_through_bound_descriptor_after_appdir_is_replaced() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let appdir = tmp.path().join("Applications");
    write_version(&appdir.join("Example.app"), "9.2.3".into(), false)?;
    let parent = open_trusted_appdir_readonly(&appdir)?;
    std::fs::rename(&appdir, tmp.path().join("original"))?;
    write_version(&appdir.join("Example.app"), "99.0.0".into(), false)?;
    assert_eq!(
        read_live_version_at(&parent, std::ffi::OsStr::new("Example.app"), "9.2.6")?,
        UpgradeDecision::Older {
            live: "9.2.3".into(),
            available: "9.2.6".into()
        }
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn plist_rejects_symlinks_at_every_bundle_component() -> Result<()> {
    for component in [
        "Example.app",
        "Example.app/Contents",
        "Example.app/Contents/Info.plist",
    ] {
        let tmp = trusted_tempdir()?;
        let appdir = tmp.path().join("Applications");
        let app = appdir.join("Example.app");
        write_version(&app, "9.2.3".into(), false)?;
        let path = appdir.join(component);
        let external = tmp.path().join("external");
        std::fs::rename(&path, &external)?;
        std::os::unix::fs::symlink(&external, &path)?;
        assert_unknown(read_fixture(&appdir)?, "symlink");
    }
    Ok(())
}

#[cfg(unix)]
fn eligible_fixture(root: &Path) -> Result<(Cask, CaskReceipt)> {
    let app = root.join("Example.app");
    write_version(&app, "9.2.3".into(), false)?;
    let mut cask = test_cask("example", "9.2.6");
    cask.auto_updates = true;
    cask.artifacts = vec![serde_json::json!({"app": ["Example.app"]})];
    let receipt = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "version": "9.2.6", "auto_updates": true,
        "apps": [app], "metadata_only_apps": [app]
    }))?;
    Ok((cask, receipt))
}

#[cfg(unix)]
#[test]
fn eligibility_uses_live_version_even_when_receipt_matches_available() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    assert_eq!(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        UpgradeDecision::Older {
            live: "9.2.3".into(),
            available: "9.2.6".into()
        }
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_accepts_adopted_metadata_only_app_with_stale_fingerprint() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, mut receipt) = eligible_fixture(tmp.path())?;
    receipt.auto_updates = false;
    receipt.targets.push(CaskTargetRecord {
        path: receipt.apps[0].clone(),
        fingerprint: CaskTargetFingerprint {
            kind: CaskTargetKind::Directory,
            digest: "old fingerprint".into(),
        },
        uninstall: None,
    });
    assert!(matches!(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        UpgradeDecision::Older { .. }
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_homebrew_precedes_artifact_parsing_and_plist_reading() -> Result<()> {
    let mut cask = test_cask("example", "9.2.6");
    cask.auto_updates = true;
    cask.artifacts = vec![serde_json::json!({"unknown_install_artifact": ["bad"]})];
    assert_unknown(
        assess_auto_update(&cask, None, false, true)?,
        "managed by Homebrew",
    );
    assert!(assess_auto_update(&cask, None, false, false).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_requires_receipt_installed_state_and_matching_app_target() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, mut receipt) = eligible_fixture(tmp.path())?;
    assert_unknown(assess_auto_update(&cask, None, true, false)?, "receipt");
    assert_unknown(
        assess_auto_update(&cask, Some(&receipt), false, false)?,
        "app",
    );
    receipt.apps[0] = tmp.path().join("Other.app");
    assert_unknown(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        "target",
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_zero_or_multiple_apps_in_metadata_or_receipt() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for count in [0, 2] {
        let mut cask = cask.clone();
        cask.artifacts = vec![serde_json::json!({"binary": ["bin/example"]})];
        cask.artifacts
            .extend((0..count).map(|_| serde_json::json!({"app": ["Example.app"]})));
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
        let (cask, mut receipt) = eligible_fixture(tmp.path())?;
        receipt.apps = vec![tmp.path().join("Example.app"); count];
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_historical_side_effect_artifacts() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for field in ["pkg_ids", "generic", "fonts", "flight_directories"] {
        let mut value = serde_json::to_value(&receipt)?;
        value[field] = serde_json::json!(["historical-artifact"]);
        let receipt = serde_json::from_value(value)?;
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_ruby_hooks() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for hook in ["preflight", "postflight"] {
        let mut cask = cask.clone();
        cask.artifacts.push(serde_json::json!({hook: null}));
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_allows_declarative_binary_and_completion_links() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (mut cask, mut receipt) = eligible_fixture(tmp.path())?;
    cask.artifacts.extend([
        serde_json::json!({"binary": ["Example.app/Contents/MacOS/example"]}),
        serde_json::json!({"bash_completion": ["example.bash"]}),
    ]);
    receipt.binaries.push(tmp.path().join("bin/example"));
    receipt
        .completions
        .push(tmp.path().join("completions/example.bash"));
    assert!(matches!(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        UpgradeDecision::Older { .. }
    ));
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_side_effect_artifacts_and_structured_flights() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for artifacts in [
        vec![
            serde_json::json!({"pkg": ["Example.pkg"]}),
            serde_json::json!({"uninstall": [{"pkgutil": "org.example.app"}]}),
        ],
        vec![serde_json::json!({"installer": [{"script": {"executable": "install.sh"}}]})],
        vec![
            serde_json::json!({"artifact": ["data", {"target": "$HOMEBREW_PREFIX/share/example"}]}),
        ],
        vec![serde_json::json!({"font": ["Example.ttf"]})],
        vec![serde_json::json!({"command_wrapper": ["example", {"executable": "example"}]})],
        vec![serde_json::json!({"generate_completions_from_executable": ["example"]})],
        vec![
            serde_json::json!({"preflight_steps": [{"steps": [{"type": "run", "command": {"base": "staged_path", "path": "prepare"}}]}]}),
        ],
        vec![
            serde_json::json!({"postflight_steps": [{"steps": [{"type": "run", "command": {"base": "staged_path", "path": "finish"}}]}]}),
        ],
    ] {
        let mut cask = cask.clone();
        cask.artifacts.extend(artifacts);
        cask_artifacts(&cask)?;
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn assessment_reports_equal_newer_and_unknown() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, mut receipt) = eligible_fixture(tmp.path())?;
    receipt.version = "1.0.0".into();
    for live in ["9.2.6", "9.2.7", "9.2.6,123"] {
        write_version(&tmp.path().join("Example.app"), live.into(), false)?;
        let decision = assess_auto_update(&cask, Some(&receipt), true, false)?;
        match live {
            "9.2.6" => assert_eq!(
                decision,
                UpgradeDecision::Equal {
                    live: live.into(),
                    available: cask.version.clone()
                }
            ),
            "9.2.7" => assert_eq!(
                decision,
                UpgradeDecision::Newer {
                    live: live.into(),
                    available: cask.version.clone()
                }
            ),
            _ => assert_unknown(decision, "compar"),
        }
    }
    Ok(())
}

struct PermissionDeniedPlist(std::io::Cursor<Vec<u8>>);

impl std::io::Read for PermissionDeniedPlist {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    }
}

impl std::io::Seek for PermissionDeniedPlist {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        std::io::Seek::seek(&mut self.0, position)
    }
}

#[test]
fn plist_read_failure_is_unknown_independently_of_runner_permissions() {
    assert_unknown(
        read_live_version_from(
            PermissionDeniedPlist(std::io::Cursor::new(Vec::new())),
            "9.2.6",
        ),
        "read",
    );
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_historical_flight_file_records() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for uninstall in [None, Some(false), Some(true)] {
        let mut receipt = receipt.clone();
        receipt.targets.push(CaskTargetRecord {
            path: tmp.path().join("flight-created-file"),
            fingerprint: CaskTargetFingerprint {
                kind: CaskTargetKind::File,
                digest: "recorded flight content".into(),
            },
            uninstall,
        });
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_historical_command_wrapper_ownership() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, mut receipt) = eligible_fixture(tmp.path())?;
    let wrapper = tmp.path().join("bin/example");
    receipt.binaries.push(wrapper.clone());
    receipt.targets.push(CaskTargetRecord {
        path: wrapper,
        fingerprint: CaskTargetFingerprint {
            kind: CaskTargetKind::Symlink,
            digest: "recorded wrapper link".into(),
        },
        uninstall: None,
    });
    receipt.prune_blocker = Some("command wrapper artifacts are not supported for pruning".into());
    assert_unknown(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        "unsupported auto-updating cask layout",
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn eligibility_rejects_recorded_installer_and_lifecycle_side_effects() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = trusted_tempdir()?;
    let mut env = EnvVarGuard::new();
    env.set(APP_DIR_ENV, tmp.path().to_str().unwrap());
    let (cask, receipt) = eligible_fixture(tmp.path())?;
    for blocker in [
        "installer artifacts may have untracked side effects",
        "install lifecycle actions may have untracked side effects",
    ] {
        let mut receipt = receipt.clone();
        receipt.prune_blocker = Some(blocker.into());
        assert_unknown(
            assess_auto_update(&cask, Some(&receipt), true, false)?,
            "unsupported auto-updating cask layout",
        );
    }
    let mut receipt = receipt;
    receipt.prune_blocker =
        Some("metadata-only app ownership cannot be proven safely during pruning".into());
    assert!(matches!(
        assess_auto_update(&cask, Some(&receipt), true, false)?,
        UpgradeDecision::Older { .. }
    ));
    Ok(())
}
