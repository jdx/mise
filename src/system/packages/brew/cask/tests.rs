use super::model::{CaskConflicts, CaskDependencies, CaskUrlSpecs};
use super::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use crate::test::EnvVarGuard;

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct BrewPrefixGuard {
    previous: Option<String>,
}

impl BrewPrefixGuard {
    fn set(prefix: &Path) -> Self {
        let previous = crate::env::var("MISE_SYSTEM_BREW_PREFIX").ok();
        crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", prefix);
        Self { previous }
    }
}

impl Drop for BrewPrefixGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(previous) => crate::env::set_var("MISE_SYSTEM_BREW_PREFIX", previous),
            None => crate::env::remove_var("MISE_SYSTEM_BREW_PREFIX"),
        }
    }
}

/// A temporary directory whose ancestors pass `ensure_trusted_appdir`'s
/// trust checks.
///
/// The system temp directory is unusable for these tests on Linux because
/// `/tmp` is world-writable (mode 1777) and the trusted walk correctly
/// refuses to operate through it. Real application directories
/// (`/Applications`, `~/Applications`) are never world-writable, so anchor
/// the fixture under the test home instead.
fn trusted_tempdir() -> Result<tempfile::TempDir> {
    let base = &*crate::env::HOME;
    file::create_dir_all(base)?;
    Ok(tempfile::Builder::new()
        .prefix(".mise-cask-appdir-")
        .tempdir_in(base)?)
}

fn run_cask_shim(
    ruby: &Path,
    shim: &Path,
    cask: &Path,
    staged_path: &Path,
    version: &str,
) -> std::io::Result<std::process::Output> {
    run_cask_shim_hook(ruby, shim, cask, staged_path, version, "preflight")
}

fn run_cask_shim_hook(
    ruby: &Path,
    shim: &Path,
    cask: &Path,
    staged_path: &Path,
    version: &str,
    hook: &str,
) -> std::io::Result<std::process::Output> {
    std::process::Command::new(ruby)
        .arg(shim)
        .env("LANG", "zz_ZZ.UTF-8")
        .env("MISE_BREW_CASK_FILE", cask)
        .env("MISE_BREW_CASK_TOKEN", "example")
        .env("MISE_BREW_CASK_VERSION", version)
        .env("MISE_BREW_CASK_STAGED_PATH", staged_path)
        .env("MISE_BREW_CASK_APPDIR", staged_path)
        .env("MISE_BREW_PREFIX", staged_path)
        .env("MISE_BREW_CASK_HOOK", hook)
        .output()
}

fn test_cask(token: &str, version: &str) -> Cask {
    Cask {
        token: token.to_string(),
        aliases: Vec::new(),
        old_tokens: Vec::new(),
        version: version.to_string(),
        auto_updates: false,
        url: "https://example.com/example.zip".to_string(),
        url_specs: CaskUrlSpecs::default(),
        sha256: Some("no_check".to_string()),
        artifacts: Vec::new(),
        depends_on: CaskDependencies::default(),
        conflicts_with: CaskConflicts::default(),
        ruby_source_path: None,
        ruby_source_checksum: None,
        tap_git_head: None,
        raw_base: None,
    }
}

#[test]
fn cask_dependency_closure_collects_formulae_and_transitive_casks() {
    let mut root = test_cask("root", "1.0.0");
    root.depends_on.formula = vec!["python@3.14".to_string()];
    root.depends_on.cask = vec!["child".to_string()];
    let mut child = test_cask("child", "1.0.0");
    child.depends_on.formula = vec!["openssl@3".to_string(), "python@3.14".to_string()];
    child.depends_on.cask = vec!["root".to_string()];

    let mut closure = CaskDependencyClosure::default();
    let mut pending = Vec::new();
    let root_request = PackageRequest {
        name: "acme/tools/root".to_string(),
        version: None,
        tap_url: Some("https://github.com/acme/custom-tools.git".to_string()),
        desired: crate::system::packages::PackageDesiredState::Present,
    };
    extend_cask_dependency_closure(&mut closure, &mut pending, &root_request, root.clone());
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].tap_url, root_request.tap_url);
    let child_request = pending.pop().unwrap();
    extend_cask_dependency_closure(&mut closure, &mut pending, &child_request, child);
    extend_cask_dependency_closure(&mut closure, &mut pending, &root_request, root);

    assert_eq!(
        closure.casks.values().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(["child".to_string(), "root".to_string()])
    );
    assert!(
        closure
            .formulae
            .values()
            .all(|request| request.tap_url == root_request.tap_url)
    );
    assert_eq!(
        closure
            .formulae
            .values()
            .map(|request| request.name.clone())
            .collect::<Vec<_>>(),
        vec!["openssl@3".to_string(), "python@3.14".to_string()]
    );
    let standard_tap_request = PackageRequest {
        tap_url: None,
        desired: crate::system::packages::PackageDesiredState::Present,
        ..root_request
    };
    assert_eq!(
        dependency_tap_url(&standard_tap_request, "child"),
        Some("https://github.com/acme/homebrew-tools.git".to_string())
    );
    assert_eq!(
        normalize_cask_raw_base(
            "https://raw.githubusercontent.com/acme/homebrew-tools/HEAD".to_string()
        ),
        "https://raw.githubusercontent.com/acme/homebrew-tools"
    );
    assert_eq!(
        normalize_cask_raw_base("https://example.com/custom".to_string()),
        "https://example.com/custom"
    );
}

#[test]
fn cask_dependency_closure_keeps_duplicate_names_from_each_tap() {
    let tap_urls = [
        "https://github.com/acme/homebrew-tools.git",
        "https://github.com/other/homebrew-tools.git",
    ];
    let mut closure = CaskDependencyClosure::default();
    let mut pending = Vec::new();
    for (owner, tap_url) in ["acme", "other"].into_iter().zip(tap_urls) {
        let mut root = test_cask("shared", "1.0.0");
        root.depends_on.formula = vec!["shared-formula".to_string()];
        root.depends_on.cask = vec!["shared-child".to_string()];
        let request = PackageRequest {
            name: format!("{owner}/tools/shared"),
            version: None,
            tap_url: Some(tap_url.to_string()),
            desired: crate::system::packages::PackageDesiredState::Present,
        };
        extend_cask_dependency_closure(&mut closure, &mut pending, &request, root);
    }
    for request in std::mem::take(&mut pending) {
        extend_cask_dependency_closure(
            &mut closure,
            &mut pending,
            &request,
            test_cask("shared-child", "1.0.0"),
        );
    }

    assert_eq!(closure.casks.len(), 4);
    assert_eq!(closure.formulae.len(), 2);
    assert_eq!(
        closure.casks.into_values().collect::<BTreeSet<_>>(),
        BTreeSet::from(["shared".to_string(), "shared-child".to_string()])
    );
    assert_eq!(
        closure
            .formulae
            .into_values()
            .filter_map(|request| request.tap_url)
            .collect::<BTreeSet<_>>(),
        tap_urls.into_iter().map(str::to_string).collect()
    );
}

fn write_test_app_receipt(cask: &Cask, app_name: &str) -> Result<PathBuf> {
    let app = AppArtifact {
        source: app_name.to_string(),
        target: Some(format!("$HOMEBREW_PREFIX/Applications/{app_name}")),
    };
    let target = app_target_path(app.target_name())?;
    file::create_dir_all(&target)?;
    file::write(target.join("version"), "1.0.0")?;
    let version_dir = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(version_dir.join(app_name))?;
    file::write(version_dir.join(app_name).join("version"), "1.0.0")?;
    write_receipt_with_flight_targets(
        &version_dir,
        cask,
        &CaskArtifacts {
            apps: vec![app],
            ..Default::default()
        },
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;
    Ok(target)
}

#[test]
fn validates_requested_cask_identity_and_trusted_aliases() -> Result<()> {
    let cask = test_cask("current", "1.0.0");
    validate_cask_identity(&cask, "current", true)?;
    assert!(validate_cask_identity(&cask, "different", true).is_err());

    let mut aliased = cask.clone();
    aliased.old_tokens = vec!["old-name".to_string()];
    validate_cask_identity(&aliased, "old-name", true)?;
    assert!(validate_cask_identity(&aliased, "old-name", false).is_err());
    Ok(())
}

#[test]
fn rejects_unsafe_cask_identity_components() {
    for value in [
        "",
        ".",
        "..",
        ".metadata",
        ".mise-tmp-x",
        "a/b",
        "a\\b",
        "a\\..\\b",
        "a\0b",
    ] {
        assert!(validate_cask_path_component("token", value).is_err());
    }
    assert!(validate_cask_path_component("token", "zed@preview").is_ok());
    assert!(validate_cask_path_component("version", "1.2.3,456").is_ok());
}

#[test]
fn detects_any_foreign_homebrew_metadata_object() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let metadata = tmp.path().join("Caskroom/example/.metadata");
    assert!(!homebrew_metadata_present("example")?);
    file::create_dir_all(&metadata)?;
    assert!(homebrew_metadata_present("example")?);
    file::remove_all(&metadata)?;
    crate::file::write(&metadata, "foreign")?;
    assert!(homebrew_metadata_present("example")?);
    Ok(())
}

#[test]
fn homebrew_version_ignores_mise_working_directories() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.join(".metadata"))?;
    file::create_dir_all(token_dir.join("1.0.0"))?;
    file::create_dir_all(token_dir.join(".mise-tmp-interrupted"))?;
    file::create_dir_all(token_dir.join(".mise-backup-interrupted"))?;

    assert_eq!(
        homebrew_installed_version("example")?,
        Some("1.0.0".to_string())
    );
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn rejects_non_utf8_homebrew_version_name() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.join(".metadata"))?;
    file::create_dir_all(token_dir.join("1.0.0"))?;
    file::create_dir_all(token_dir.join(Path::new(std::ffi::OsStr::from_bytes(b"\xff"))))?;

    let error = homebrew_installed_version("example").unwrap_err();
    assert!(error.to_string().contains("name is not valid UTF-8"));
    assert!(error.to_string().contains("brew-cask:example"));
    Ok(())
}

#[test]
fn homebrew_version_enumeration_error_includes_directory() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.parent().unwrap())?;
    file::write(&token_dir, "not a directory")?;

    let error = homebrew_installed_versions("example").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("failed to read Homebrew Caskroom directory"));
    assert!(message.contains(&token_dir.display().to_string()));
    Ok(())
}

#[cfg(unix)]
#[test]
fn homebrew_metadata_probe_error_is_not_absence() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.parent().unwrap())?;
    file::write(&token_dir, "not a directory")?;

    let error = homebrew_metadata_present("example").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("failed to inspect Homebrew metadata"));
    assert!(message.contains(".metadata"));
    Ok(())
}

#[test]
fn ignores_version_without_homebrew_metadata() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    file::create_dir_all(caskroom_version_dir("example", "1.0.0"))?;

    assert_eq!(homebrew_installed_version("example")?, None);
    Ok(())
}

#[test]
fn rejects_homebrew_metadata_without_installed_version() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    file::create_dir_all(caskroom_token_dir("example").join(".metadata"))?;

    let error = homebrew_installed_version("example").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("repair it with `brew reinstall --cask example`")
    );
    Ok(())
}

#[test]
fn rejects_homebrew_metadata_with_multiple_versions() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.join(".metadata"))?;
    file::create_dir_all(token_dir.join("2.0.0"))?;
    file::create_dir_all(token_dir.join("1.0.0"))?;

    let error = homebrew_installed_version("example").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("multiple Caskroom versions (1.0.0, 2.0.0)")
    );
    Ok(())
}

#[test]
fn externally_managed_version_precedes_artifact_parsing() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.join(".metadata"))?;
    file::create_dir_all(token_dir.join("1.0.0"))?;
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![serde_json::json!({
        "future_artifact": ["unsupported"]
    })];
    let request = PackageRequest {
        name: cask.token.clone(),
        version: None,
        tap_url: None,
        desired: crate::system::packages::PackageDesiredState::Present,
    };

    assert_eq!(
        package_state(&request, &cask)?,
        PackageState::Installed {
            version: "1.0.0".to_string()
        }
    );
    assert!(cask_artifacts(&cask).is_err());
    Ok(())
}

#[test]
fn both_receipt_types_satisfy_installed_state_without_mutation() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("example", "1.0.0");
    write_test_app_receipt(&cask, "Example.app")?;
    let metadata = caskroom_token_dir(&cask.token).join(".metadata/receipt.json");
    file::create_dir_all(metadata.parent().unwrap())?;
    file::write(&metadata, "homebrew")?;
    let mise_receipt = caskroom_version_dir(&cask.token, &cask.version).join(".mise-cask.toml");
    let receipt_before = file::read_to_string(&mise_receipt)?;
    let request = PackageRequest {
        name: cask.token.clone(),
        version: None,
        tap_url: None,
        desired: crate::system::packages::PackageDesiredState::Present,
    };

    assert_eq!(
        package_state(&request, &cask)?,
        PackageState::Installed {
            version: "1.0.0".to_string()
        }
    );
    assert_eq!(file::read_to_string(&mise_receipt)?, receipt_before);
    assert_eq!(file::read_to_string(&metadata)?, "homebrew");

    file::create_dir_all(caskroom_version_dir(&cask.token, "2.0.0"))?;
    let error = package_state(&request, &cask).unwrap_err();
    assert!(error.to_string().contains("multiple Caskroom versions"));
    assert_eq!(file::read_to_string(&mise_receipt)?, receipt_before);
    assert_eq!(file::read_to_string(&metadata)?, "homebrew");
    Ok(())
}

#[test]
fn mise_owned_state_validates_artifacts_before_receipt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let mut cask = test_cask("example", "1.0.0");
    write_test_app_receipt(&cask, "Example.app")?;
    cask.artifacts = vec![serde_json::json!({
        "future_artifact": ["unsupported"]
    })];
    let request = PackageRequest {
        name: cask.token.clone(),
        version: None,
        tap_url: None,
        desired: crate::system::packages::PackageDesiredState::Present,
    };

    let error = package_state(&request, &cask).unwrap_err();
    assert!(error.to_string().contains("unsupported artifact type"));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn mise_owned_state_validates_platform_before_receipt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let mut cask = test_cask("example", "1.0.0");
    write_test_app_receipt(&cask, "Example.app")?;
    cask.artifacts = vec![serde_json::json!({
        "app": ["Example.app"]
    })];
    let request = PackageRequest {
        name: cask.token.clone(),
        version: None,
        tap_url: None,
        desired: crate::system::packages::PackageDesiredState::Present,
    };

    assert!(matches!(
        package_state(&request, &cask)?,
        PackageState::Unavailable { .. }
    ));
    Ok(())
}

#[test]
fn ownership_race_guard_removes_only_mise_stage() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    file::write(stage.join("download"), "mise")?;
    let metadata = caskroom_token_dir("example").join(".metadata/receipt.json");
    file::create_dir_all(metadata.parent().unwrap())?;
    file::write(&metadata, "homebrew")?;

    let error = ensure_homebrew_did_not_take_ownership("example", &stage).unwrap_err();

    assert!(error.to_string().contains("Homebrew took ownership"));
    assert!(!stage.exists());
    assert_eq!(file::read_to_string(metadata)?, "homebrew");
    Ok(())
}

#[cfg(unix)]
#[test]
fn ownership_race_probe_error_removes_mise_stage() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    file::write(stage.join("download"), "mise")?;
    let token_dir = caskroom_token_dir("example");
    file::create_dir_all(token_dir.parent().unwrap())?;
    file::write(&token_dir, "not a directory")?;

    let error = ensure_homebrew_did_not_take_ownership("example", &stage).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("failed to inspect Homebrew metadata")
    );
    assert!(!stage.exists());
    Ok(())
}

#[test]
#[cfg(unix)]
fn directory_fingerprint_tracks_tree_content_and_links() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let root = tmp.path().join("Example.app");
    file::create_dir_all(root.join("Contents/Resources"))?;
    crate::file::write(root.join("Contents/app"), "one")?;
    crate::file::write(root.join("Contents/Resources/config"), "config")?;
    std::os::unix::fs::symlink("app", root.join("Contents/current"))?;
    let original = cask_target_fingerprint(&root)?;
    assert_eq!(original, cask_target_fingerprint(&root)?);

    crate::file::write(root.join("Contents/app"), "two")?;
    assert_ne!(original, cask_target_fingerprint(&root)?);
    crate::file::write(root.join("Contents/app"), "one")?;
    assert_eq!(original, cask_target_fingerprint(&root)?);

    crate::file::write(root.join("Contents/added"), "added")?;
    assert_ne!(original, cask_target_fingerprint(&root)?);
    file::remove_file(root.join("Contents/added"))?;
    assert_eq!(original, cask_target_fingerprint(&root)?);

    file::remove_file(root.join("Contents/current"))?;
    std::os::unix::fs::symlink("Resources/config", root.join("Contents/current"))?;
    assert_ne!(original, cask_target_fingerprint(&root)?);

    file::remove_all(&root)?;
    file::create_dir_all(root.join("Contents/Resources"))?;
    crate::file::write(root.join("Contents/app"), "replacement")?;
    assert_ne!(original, cask_target_fingerprint(&root)?);
    Ok(())
}

#[test]
#[cfg(unix)]
fn staged_app_accepts_target_symlink_and_legacy_copy() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("Applications/Example.app");
    file::create_dir_all(target.join("Contents"))?;
    file::write(target.join("Contents/app"), "content")?;
    let record = CaskTargetRecord {
        path: target.clone(),
        fingerprint: cask_target_fingerprint(&target)?,
        uninstall: None,
    };

    let staged_link = tmp.path().join("Caskroom/example/1.0.0/Example.app");
    file::create_dir_all(staged_link.parent().unwrap())?;
    file::make_symlink(&target, &staged_link)?;
    assert!(staged_app_matches_target(&record, &staged_link));

    file::remove_file(&staged_link)?;
    file::copy_dir_all_preserve_symlinks(&target, &staged_link)?;
    assert!(staged_app_matches_target(&record, &staged_link));

    file::remove_all(&staged_link)?;
    file::make_symlink(&tmp.path().join("Applications/Other.app"), &staged_link)?;
    assert!(!staged_app_matches_target(&record, &staged_link));
    Ok(())
}

#[test]
fn completed_receipt_ignores_app_bundle_content_drift() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("example", "1.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let artifacts = CaskArtifacts {
        apps: vec![app.clone()],
        ..Default::default()
    };
    let app_target = app_target_path(app.target_name())?;
    file::create_dir_all(app_target.join("Contents"))?;
    crate::file::write(app_target.join("Contents/app"), "original")?;
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    write_receipt_with_flight_targets(
        &caskroom,
        &cask,
        &artifacts,
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;
    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some(cask.version.clone())
    );
    crate::file::write(app_target.join("Contents/app"), "changed")?;
    // Content drift must not look like "missing" — that would reinstall the
    // app on the next apply and revoke macOS TCC grants.
    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some(cask.version.clone())
    );
    assert!(!cask_target_record_matches(
        read_receipt(&caskroom)?
            .expect("receipt")
            .targets
            .first()
            .expect("app target")
    )?);
    Ok(())
}

#[test]
fn completed_receipt_missing_app_is_not_installed() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("example", "1.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let artifacts = CaskArtifacts {
        apps: vec![app.clone()],
        ..Default::default()
    };
    let app_target = app_target_path(app.target_name())?;
    file::create_dir_all(app_target.join("Contents"))?;
    crate::file::write(app_target.join("Contents/app"), "original")?;
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    write_receipt_with_flight_targets(
        &caskroom,
        &cask,
        &artifacts,
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;
    file::remove_all(&app_target)?;
    assert_eq!(mise_installed_cask_version(&cask)?, None);
    Ok(())
}

#[test]
fn cask_target_present_checks_symlink_destination_and_kinds() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dest = tmp.path().join("bin/tool");
    file::create_dir_all(dest.parent().unwrap())?;
    crate::file::write(&dest, "tool")?;
    let link = tmp.path().join("prefix/bin/tool");
    file::create_dir_all(link.parent().unwrap())?;
    file::make_symlink(&dest, &link)?;
    let record = CaskTargetRecord {
        path: link.clone(),
        fingerprint: cask_target_fingerprint(&link)?,
        uninstall: None,
    };
    assert!(cask_target_present(&record));

    file::remove_file(&dest)?;
    assert!(
        !cask_target_present(&record),
        "dangling symlink must not count as present"
    );

    crate::file::write(&dest, "tool")?;
    let other = tmp.path().join("bin/other");
    crate::file::write(&other, "other")?;
    file::remove_file(&link)?;
    file::make_symlink(&other, &link)?;
    assert!(
        !cask_target_present(&record),
        "retargeted symlink must not count as present"
    );

    file::remove_file(&link)?;
    crate::file::write(&link, "not a symlink")?;
    assert!(
        !cask_target_present(&record),
        "file replacing a symlink must not count as present"
    );

    let font = tmp.path().join("fonts/Example.ttf");
    file::create_dir_all(font.parent().unwrap())?;
    crate::file::write(&font, "font")?;
    let file_record = CaskTargetRecord {
        path: font.clone(),
        fingerprint: cask_target_fingerprint(&font)?,
        uninstall: None,
    };
    assert!(cask_target_present(&file_record));
    crate::file::write(&font, "changed font bytes")?;
    assert!(
        cask_target_present(&file_record),
        "file content drift is ignored for install health"
    );
    file::remove_file(&font)?;
    file::create_dir_all(&font)?;
    assert!(
        !cask_target_present(&file_record),
        "directory replacing a file must not count as present"
    );
    Ok(())
}

#[test]
fn self_updating_receipt_accepts_app_bundle_drift() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let mut cask = test_cask("self-updating", "1.0.0");
    cask.auto_updates = true;
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let artifacts = CaskArtifacts {
        apps: vec![app.clone()],
        ..Default::default()
    };
    let app_target = app_target_path(app.target_name())?;
    file::create_dir_all(app_target.join("Contents"))?;
    crate::file::write(app_target.join("Contents/app"), "downloaded")?;
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    write_receipt_with_flight_targets(
        &caskroom,
        &cask,
        &artifacts,
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;

    crate::file::write(app_target.join("Contents/app"), "updated by app")?;
    cask.version = "2.0.0".to_string();
    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some("1.0.0".to_string())
    );
    let receipt = read_receipt(&caskroom)?.unwrap();
    assert!(receipt.auto_updates);
    assert!(!receipt.prune_safe);
    assert!(
        receipt
            .prune_blocker
            .as_deref()
            .is_some_and(|reason| reason.contains("metadata-only app ownership"))
    );
    assert!(!caskroom.join("Example.app").exists());

    // Fail closed for schema-3 receipts written before metadata-only apps
    // were marked non-prunable. A replacement bundle at the same path
    // must never become removable merely because it is a directory.
    let mut legacy_receipt = receipt;
    legacy_receipt.prune_safe = true;
    legacy_receipt.prune_blocker = None;
    file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&legacy_receipt)?,
    )?;
    let plan =
        cask_prune_plan_from_tokens(&BTreeSet::new(), &prefix::prefix().join(".mise-test-state"))?;
    assert!(plan.remove.is_empty());
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "self-updating" && skip.reason.contains("metadata-only app ownership")
    }));
    Ok(())
}

#[test]
fn adopts_only_an_identical_existing_app() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = trusted_tempdir()?;
    let root = tmp.path().canonicalize()?;
    let _guard = BrewPrefixGuard::set(&root);
    let stage = root.join("stage");
    let caskroom = root.join("Caskroom/example/1.0.0");
    let source = stage.join("Example.app/Contents");
    let target = root.join("Applications/Example.app/Contents");
    file::create_dir_all(&source)?;
    file::create_dir_all(&target)?;
    crate::file::write(source.join("app"), "identical")?;
    crate::file::write(target.join("app"), "identical")?;
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };

    assert!(install_app(&stage, &caskroom, &app, true, true, true)?);
    assert!(!caskroom.join("Example.app").exists());

    crate::file::write(target.join("app"), "different")?;
    let error = install_app(&stage, &caskroom, &app, true, true, true).unwrap_err();
    assert!(error.to_string().contains("is not identical"));
    Ok(())
}

#[test]
fn self_updating_cask_adopts_a_different_existing_app() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = trusted_tempdir()?;
    let root = tmp.path().canonicalize()?;
    let _guard = BrewPrefixGuard::set(&root);
    let stage = root.join("stage");
    let caskroom = root.join("Caskroom/example/1.0.0");
    let source = stage.join("Example.app/Contents");
    let target = root.join("Applications/Example.app/Contents");
    file::create_dir_all(&source)?;
    file::create_dir_all(&target)?;
    crate::file::write(source.join("app"), "downloaded version")?;
    crate::file::write(target.join("app"), "self-updated version")?;
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };

    assert!(install_app(&stage, &caskroom, &app, false, true, false)?);
    assert_eq!(
        std::fs::read_to_string(target.join("app"))?,
        "self-updated version"
    );
    assert!(!caskroom.join("Example.app").exists());
    Ok(())
}

#[test]
fn parses_firefox_command_wrapper_artifact() -> Result<()> {
    let mut cask = test_cask("firefox", "153.0.1");
    cask.artifacts = vec![
        serde_json::json!({
            "app": ["Firefox.app"],
            "target": "/Applications/Firefox.app"
        }),
        serde_json::json!({
            "command_wrapper": [
                "firefox",
                {"executable": "$APPDIR/Firefox.app/Contents/MacOS/firefox"}
            ],
            "target": "$HOMEBREW_PREFIX/bin/firefox"
        }),
    ];

    let artifacts = cask_artifacts(&cask)?;
    assert_eq!(
        artifacts.command_wrappers,
        vec![CommandWrapperArtifact {
            name: "firefox".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
            content: None,
            executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
        }]
    );
    Ok(())
}

#[test]
fn rejects_command_wrapper_invalid_environment_name() {
    let value = serde_json::json!({
        "command_wrapper": [
            "example",
            {
                "executable": "/usr/bin/example",
                "env": {"INVALID-NAME": "value"}
            }
        ]
    });

    let err = parse_command_wrapper_artifact(&value)
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid command_wrapper environment name 'INVALID-NAME'"));
}

#[test]
#[cfg(unix)]
fn stages_command_wrapper_with_args_env_and_expanded_paths() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("firefox", "153.0.1");
    let caskroom = prefix.join("Caskroom/firefox/.mise-tmp");
    let final_caskroom = caskroom_version_dir(&cask.token, &cask.version);
    let appdir = tmp.path().join("Applications");
    let wrapper = CommandWrapperArtifact {
        name: "firefox".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
        content: None,
        executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
        args: vec![
            "--profile".to_string(),
            "two words".to_string(),
            "{{version}}".to_string(),
        ],
        env: BTreeMap::from([
            ("FIREFOX_MODE".to_string(), "mise test".to_string()),
            ("FIREFOX_ROOT".to_string(), "{{staged_path}}".to_string()),
        ]),
    };

    stage_command_wrapper(&caskroom, &appdir, &cask, &wrapper)?;
    file::rename(&caskroom, &final_caskroom)?;
    link_command_wrapper(&final_caskroom, &wrapper)?;

    let staged = final_caskroom.join(".homebrew-command-wrappers/firefox");
    let contents = file::read_to_string(&staged)?;
    assert!(contents.starts_with("#!/bin/bash\n"));
    assert!(contents.contains("FIREFOX_MODE='mise test'"));
    assert!(contents.contains(&format!(
        "FIREFOX_ROOT={}",
        final_caskroom.to_string_lossy()
    )));
    let executable = appdir.join("Firefox.app/Contents/MacOS/firefox");
    assert!(contents.contains(executable.to_string_lossy().as_ref()));
    assert!(contents.contains("--profile 'two words' 153.0.1 \"$@\""));
    assert!(staged.metadata()?.permissions().mode() & 0o111 != 0);
    assert_eq!(std::fs::read_link(wrapper.target_path()?)?, staged);
    Ok(())
}

#[test]
#[cfg(unix)]
fn stages_command_wrapper_with_literal_content() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("example", "1.0.0");
    let caskroom = prefix.join("Caskroom/example/1.0.0");
    let wrapper = CommandWrapperArtifact {
            name: "example".to_string(),
            target: None,
            content: Some(
                "#!/bin/sh\nHOME=$HOME\nSTAGE={{staged_path}}\nexec '$HOMEBREW_PREFIX/bin/example' \"$@\"\n"
                    .to_string(),
            ),
            executable: None,
            args: Vec::new(),
            env: BTreeMap::new(),
        };

    stage_command_wrapper(&caskroom, Path::new("/Applications"), &cask, &wrapper)?;

    assert_eq!(
        file::read_to_string(caskroom.join(".homebrew-command-wrappers/example"))?,
        format!(
            "#!/bin/sh\nHOME=$HOME\nSTAGE={{{{staged_path}}}}\nexec '{}/bin/example' \"$@\"\n",
            prefix.display()
        )
    );
    Ok(())
}

#[test]
fn parses_structured_flight_steps() -> Result<()> {
    let mut cask = test_cask("wezterm@nightly", "latest");
    cask.artifacts = vec![
        serde_json::json!({
            "preflight_steps": [{
                "steps": [
                    {
                        "type": "move",
                        "source_glob": true,
                        "source": {
                            "base": "staged_path",
                            "path": "{WezTerm-*,wezterm-*}/WezTerm.app"
                        },
                        "target": {
                            "base": "staged_path",
                            "path": "."
                        }
                    },
                    {
                        "type": "remove",
                        "recursive": true,
                        "paths": [
                            {"base": "staged_path", "path": "WezTerm-*"},
                            {"base": "staged_path", "path": "wezterm-*"}
                        ]
                    }
                ]
            }]
        }),
        serde_json::json!({"app": "WezTerm.app"}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            apps: vec![AppArtifact {
                source: "WezTerm.app".to_string(),
                target: None,
            }],
            preflight_steps: vec![
                FlightStep::Move {
                    source: FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: "{WezTerm-*,wezterm-*}/WezTerm.app".to_string(),
                    },
                    target: FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: ".".to_string(),
                    },
                    source_glob: true,
                },
                FlightStep::Remove {
                    paths: vec![
                        FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: "WezTerm-*".to_string(),
                        },
                        FlightPath {
                            base: FlightPathBase::StagedPath,
                            path: "wezterm-*".to_string(),
                        }
                    ],
                    recursive: true,
                }
            ],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn parses_orbstack_structured_run_step() -> Result<()> {
    let mut cask = test_cask("orbstack", "2.2.1,20628");
    cask.artifacts = vec![
        serde_json::json!({
            "app": ["OrbStack.app"],
            "target": "/Applications/OrbStack.app"
        }),
        serde_json::json!({
            "postflight_steps": [{
                "steps": [{
                    "command": {
                        "base": "appdir",
                        "path": "OrbStack.app/Contents/MacOS/bin/orbctl"
                    },
                    "type": "run",
                    "args": ["_internal", "brew-postflight"]
                }]
            }]
        }),
    ];

    let artifacts = cask_artifacts(&cask)?;
    assert_eq!(
        artifacts.postflight_steps,
        vec![FlightStep::Run {
            command: FlightPath {
                base: FlightPathBase::AppDir,
                path: "OrbStack.app/Contents/MacOS/bin/orbctl".to_string(),
            },
            args: vec!["_internal".to_string(), "brew-postflight".to_string()],
            env: BTreeMap::new(),
            sudo: false,
            guards: Vec::new(),
        }]
    );
    Ok(())
}

#[test]
fn parses_structured_symlink_steps() -> Result<()> {
    let mut cask = test_cask("docker-desktop", "4.86.0,236216");
    cask.artifacts = vec![
        serde_json::json!({"app": "Docker.app"}),
        serde_json::json!({
            "postflight_steps": [{
                "steps": [{
                    "type": "symlink",
                    "source": {"path": "{{appdir}}/Docker.app/Contents/Resources/bin/kubectl"},
                    "target": {"path": "/usr/local/bin/kubectl"},
                    "force": true,
                    "uninstall": true,
                    "sudo": "if_needed",
                    "guards": [{
                        "condition": "unless_exists",
                        "path": "/usr/local/bin/kubectl",
                        "id": "1"
                    }]
                }]
            }]
        }),
    ];

    let artifacts = cask_artifacts(&cask)?;
    assert!(matches!(
        artifacts.postflight_steps.as_slice(),
        [FlightStep::Symlink {
            force: true,
            uninstall: true,
            sudo: FlightSudo::IfNeeded,
            guards,
            ..
        }] if guards.len() == 1
    ));
    Ok(())
}

#[test]
fn parses_gcloud_copy_installer_and_run_metadata() -> Result<()> {
    let mut cask = test_cask("gcloud-cli", "580.0.0");
    cask.artifacts = vec![
        serde_json::json!({
            "preflight_steps": [{"steps": [{
                "type": "copy",
                "source": {"base": "staged_path", "path": "google-cloud-sdk/."},
                "target": {"base": "homebrew_prefix", "path": "share/google-cloud-sdk"},
                "recursive": true
            }]}]
        }),
        serde_json::json!({
            "installer": [{"script": {
                "executable": "google-cloud-sdk/install.sh",
                "args": ["--quiet", "--install-python", "false"]
            }}]
        }),
        serde_json::json!({"binary": "google-cloud-sdk/bin/gcloud"}),
        serde_json::json!({
            "postflight_steps": [{"steps": [{
                "type": "run",
                "command": {"base": "homebrew_prefix", "path": "share/google-cloud-sdk/bin/gcloud"},
                "args": ["version"],
                "network_access": true
            }]}]
        }),
    ];

    let artifacts = cask_artifacts(&cask)?;
    assert!(matches!(
        artifacts.preflight_steps.as_slice(),
        [FlightStep::Copy {
            recursive: true,
            overwrite: true,
            ..
        }]
    ));
    assert_eq!(
        artifacts.installers,
        [InstallerArtifact {
            executable: "google-cloud-sdk/install.sh".to_string(),
            args: vec![
                "--quiet".to_string(),
                "--install-python".to_string(),
                "false".to_string()
            ],
        }]
    );
    assert!(matches!(
        artifacts.postflight_steps.as_slice(),
        [FlightStep::Run { .. }]
    ));
    Ok(())
}

#[test]
#[cfg(unix)]
fn installer_script_is_made_executable_before_running() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let _guard = BrewPrefixGuard::set(&prefix);
    file::create_dir_all(prefix.join("bin"))?;
    file::create_dir_all(prefix.join("sbin"))?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let script = stage.join("install.sh");
    let marker = tmp.path().join("installed");
    file::write(&script, "#!/bin/sh\nprintf '%s' \"$PATH\" > \"$1\"\n")?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644))?;
    let installer = InstallerArtifact {
        executable: "install.sh".to_string(),
        args: vec![marker.display().to_string()],
    };

    run_installer_artifact(&stage, &installer, &BTreeSet::new())?;

    let installed_path = file::read_to_string(marker)?;
    let installed_paths = std::env::split_paths(std::ffi::OsStr::new(&installed_path))
        .take(2)
        .collect::<Vec<_>>();
    assert_eq!(installed_paths, [prefix.join("bin"), prefix.join("sbin")]);
    assert_ne!(script.metadata()?.permissions().mode() & 0o111, 0);
    Ok(())
}

#[test]
#[cfg(unix)]
fn installer_script_rejects_paths_outside_stage() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let outside = tmp.path().join("outside.sh");
    file::write(&outside, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644))?;
    file::make_symlink(&outside, &stage.join("linked.sh"))?;

    for executable in [
        outside.display().to_string(),
        "../outside.sh".to_string(),
        "linked.sh".to_string(),
    ] {
        let err = run_installer_artifact(
            &stage,
            &InstallerArtifact {
                executable,
                args: Vec::new(),
            },
            &BTreeSet::new(),
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("outside trusted installer roots"));
        assert_eq!(outside.metadata()?.permissions().mode() & 0o111, 0);
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn installer_script_accepts_preflight_copied_root() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let _guard = BrewPrefixGuard::set(&prefix);
    file::create_dir_all(prefix.join("bin"))?;
    file::create_dir_all(prefix.join("sbin"))?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let copied = prefix.join("share/example");
    file::create_dir_all(&copied)?;
    let marker = tmp.path().join("installed");
    file::write(
        copied.join("install.sh"),
        "#!/bin/sh\nprintf installed > \"$1\"\n",
    )?;
    file::make_symlink(&copied, &stage.join("payload"))?;

    let copied_files = BTreeSet::from([file::desymlink_path(&copied.join("install.sh"))]);
    run_installer_artifact(
        &stage,
        &InstallerArtifact {
            executable: "payload/install.sh".to_string(),
            args: vec![marker.display().to_string()],
        },
        &copied_files,
    )?;

    assert_eq!(file::read_to_string(marker)?, "installed");
    Ok(())
}

#[test]
#[cfg(unix)]
fn installer_script_rejects_unrecorded_file_beneath_copied_target() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let broad_target = tmp.path().join("prefix");
    file::create_dir_all(broad_target.join("bin"))?;
    let outside = broad_target.join("bin/existing.sh");
    file::write(&outside, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o644))?;
    file::make_symlink(&broad_target, &stage.join("payload"))?;
    let copied_files = BTreeSet::from([broad_target.join("copied.txt")]);

    let err = run_installer_artifact(
        &stage,
        &InstallerArtifact {
            executable: "payload/bin/existing.sh".to_string(),
            args: Vec::new(),
        },
        &copied_files,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("outside trusted installer roots"));
    assert_eq!(outside.metadata()?.permissions().mode() & 0o111, 0);
    Ok(())
}

#[test]
#[cfg(unix)]
fn installer_mutations_are_included_in_durable_symlink_sources() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let _guard = BrewPrefixGuard::set(&prefix);
    file::create_dir_all(prefix.join("bin"))?;
    file::create_dir_all(prefix.join("sbin"))?;
    let stage = tmp.path().join("stage");
    let source = stage.join("payload");
    file::create_dir_all(&source)?;
    let script = stage.join("install.sh");
    file::write(&script, "#!/bin/sh\nprintf mutated > \"$1\"\n")?;
    let installer = InstallerArtifact {
        executable: "install.sh".to_string(),
        args: vec![source.join("generated").display().to_string()],
    };
    let target = tmp.path().join("share/example");
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(&source, &target)?;
    let mut targets = FlightTargetTransaction::default();
    targets.record_installed(target);
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");

    run_installers_before_durabilizing(
        &stage,
        &temporary_caskroom,
        &[installer],
        &mut targets,
        |_| Ok(()),
    )?;

    assert_eq!(
        file::read_to_string(temporary_caskroom.join(".homebrew-staged/payload/generated"))?,
        "mutated"
    );
    Ok(())
}

#[test]
fn structured_copy_restores_external_target_without_status_tracking() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let _guard = BrewPrefixGuard::set(&prefix);
    let stage = tmp.path().join("stage");
    let source = stage.join("google-cloud-sdk");
    file::create_dir_all(&source)?;
    file::write(source.join("gcloud"), "sdk")?;
    let target = prefix.join("share/google-cloud-sdk");
    file::create_dir_all(&target)?;
    file::write(target.join("old"), "old")?;
    let step = FlightStep::Copy {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "google-cloud-sdk/.".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::HomebrewPrefix,
            path: "share/google-cloud-sdk".to_string(),
        },
        recursive: true,
        overwrite: true,
        source_glob: false,
        guards: Vec::new(),
    };
    let mut targets = FlightTargetTransaction::default();
    execute_flight_steps_with_completion(
        &test_cask("gcloud-cli", "580.0.0"),
        &[step],
        &stage,
        Path::new("/Applications"),
        "preflight_steps",
        &mut targets,
        |_, _| Ok(()),
    )?;
    assert!(target.join("gcloud").is_file());
    assert!(!target.join("old").exists());
    assert!(targets.installed_targets().is_empty());
    assert_eq!(
        targets.copied_files(),
        &BTreeSet::from([file::desymlink_path(&target.join("gcloud"))])
    );
    targets.rollback()?;
    assert_eq!(file::read_to_string(target.join("old"))?, "old");
    Ok(())
}

#[test]
fn structured_copy_rollback_removes_target_with_created_parent() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let _guard = BrewPrefixGuard::set(&prefix);
    let stage = tmp.path().join("stage");
    let source = stage.join("payload");
    file::create_dir_all(&source)?;
    file::write(source.join("installed"), "content")?;
    let target = prefix.join("share/new-parent/payload");
    let copy = FlightStep::Copy {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "payload/.".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::HomebrewPrefix,
            path: "share/new-parent/payload".to_string(),
        },
        recursive: true,
        overwrite: true,
        source_glob: false,
        guards: Vec::new(),
    };
    let fail = FlightStep::Copy {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "missing".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::HomebrewPrefix,
            path: "share/unused".to_string(),
        },
        recursive: false,
        overwrite: true,
        source_glob: false,
        guards: Vec::new(),
    };

    let err = execute_flight_steps(
        &test_cask("example", "1.0.0"),
        &[copy, fail],
        &stage,
        Path::new("/Applications"),
        "preflight_steps",
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("was not found"));
    assert!(target.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn cask_metadata_accepts_null_dependencies_and_conflicts() -> Result<()> {
    let cask: Cask = serde_json::from_value(serde_json::json!({
        "token": "example",
        "version": "1.0.0",
        "url": "https://example.com/example.zip",
        "auto_updates": true,
        "depends_on": null,
        "conflicts_with": null
    }))?;
    assert!(cask.depends_on.formula.is_empty());
    assert!(cask.conflicts_with.cask.is_empty());
    assert!(cask.auto_updates);
    Ok(())
}

#[test]
fn cask_metadata_treats_null_auto_updates_as_false() -> Result<()> {
    let cask: Cask = serde_json::from_value(serde_json::json!({
        "token": "example",
        "version": "1.0.0",
        "url": "https://example.com/example.zip",
        "auto_updates": null
    }))?;
    assert!(!cask.auto_updates);
    Ok(())
}

#[test]
fn structured_symlink_preserves_relative_source() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let appdir = tmp.path().join("Applications");
    let target = appdir.join("MeshLab2025.07.app/Contents/MacOS/MeshLab");
    file::create_dir_all(
        target
            .parent()
            .ok_or_else(|| eyre!("missing target parent"))?,
    )?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::Literal,
            path: "meshlab".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::AppDir,
            path: "MeshLab{{version}}.app/Contents/MacOS/MeshLab".to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: vec![FlightGuard::UnlessExists(FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        })],
    };

    execute_flight_steps(
        &test_cask("meshlab", "2025.07"),
        &[step],
        tmp.path(),
        &appdir,
        "postflight_steps",
    )?;

    assert_eq!(std::fs::read_link(target)?, PathBuf::from("meshlab"));
    Ok(())
}

#[test]
fn based_flight_paths_reject_root_escapes() {
    let cask = test_cask("example", "1.0.0");
    let staged = Path::new("/tmp/staged");
    let appdir = Path::new("/Applications");
    for base in [
        FlightPathBase::StagedPath,
        FlightPathBase::AppDir,
        FlightPathBase::HomebrewPrefix,
    ] {
        for path in ["../outside", "/absolute/outside"] {
            let err = resolve_flight_path_with_context(
                &cask,
                &FlightPath {
                    base,
                    path: path.to_string(),
                },
                staged,
                appdir,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains("invalid structured flight path"));
        }
    }
}

#[test]
#[cfg(unix)]
fn structured_symlink_expands_versioned_glob() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let source_dir = tmp.path().join("tool-2/bin");
    file::create_dir_all(&source_dir)?;
    file::write(source_dir.join("tool"), "binary")?;
    let target_dir = tmp.path().join("links");
    file::create_dir_all(&target_dir)?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "tool-{{version.major}}/bin/*".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target_dir.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: true,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    execute_flight_steps(
        &test_cask("tool", "2.3.4"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )?;

    let link = target_dir.join("tool");
    assert_eq!(
        lexically_normalized_path(&resolve_symlink_target(&link, std::fs::read_link(&link)?)),
        source_dir.join("tool")
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_glob_rollback_removes_created_directory() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let staged = tmp.path().join("stage");
    file::create_dir_all(&staged)?;
    file::write(staged.join("one"), "one")?;
    file::write(staged.join("two"), "two")?;
    let target = tmp.path().join("links");
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "*".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: true,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };
    let mut targets = FlightTargetTransaction::default();

    execute_flight_step(
        &test_cask("example", "1.0.0"),
        &step,
        &staged,
        Path::new("/Applications"),
        &mut targets,
    )?;
    assert!(target.is_dir());
    assert_eq!(
        targets.installed_directories(),
        std::slice::from_ref(&target)
    );
    assert!(!targets.installed_targets().contains(&target));
    assert_eq!(
        targets.uninstall_targets(),
        &BTreeMap::from([(target.join("one"), false), (target.join("two"), false),])
    );

    targets.rollback()?;

    assert!(target.symlink_metadata().is_err());

    file::create_dir_all(&target)?;
    let mut upgrade_targets = FlightTargetTransaction::default();
    upgrade_targets.previous_directories.insert(target.clone());
    execute_flight_step(
        &test_cask("example", "2.0.0"),
        &step,
        &staged,
        Path::new("/Applications"),
        &mut upgrade_targets,
    )?;
    assert_eq!(upgrade_targets.installed_directories(), [target]);
    upgrade_targets.commit()?;
    Ok(())
}

#[test]
fn obsolete_flight_directories_remove_only_empty_unclaimed_directories() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let empty = tmp.path().join("empty");
    let occupied = tmp.path().join("occupied");
    let current = tmp.path().join("current");
    for directory in [&empty, &occupied, &current] {
        file::create_dir_all(directory)?;
    }
    file::write(occupied.join("user-file"), "keep")?;
    let previous = BTreeSet::from([empty.clone(), occupied.clone(), current.clone()]);

    remove_obsolete_flight_directories(&previous, std::slice::from_ref(&current))?;

    assert!(empty.symlink_metadata().is_err());
    assert!(occupied.join("user-file").is_file());
    assert!(current.is_dir());
    Ok(())
}

#[test]
fn structured_symlink_rejects_empty_glob() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("links");
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "missing/*".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: true,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    let err = execute_flight_steps(
        &test_cask("tool", "1.0.0"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )
    .unwrap_err();
    let err = format!("{err:#}");

    assert!(err.contains("did not match any paths"));
    assert!(target.symlink_metadata().is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_glob_replaces_dangling_target() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let source_dir = tmp.path().join("bin");
    file::create_dir_all(&source_dir)?;
    file::write(source_dir.join("one"), "one")?;
    file::write(source_dir.join("two"), "two")?;
    let target = tmp.path().join("links");
    file::make_symlink(Path::new("missing"), &target)?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "bin/*".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: true,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    execute_flight_steps(
        &test_cask("tool", "1.0.0"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )?;

    assert!(target.is_dir());
    for name in ["one", "two"] {
        let link = target.join(name);
        assert_eq!(
            lexically_normalized_path(&resolve_symlink_target(&link, std::fs::read_link(&link)?)),
            source_dir.join(name)
        );
    }
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_replaces_directory_symlink_without_following_it() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("source");
    let unrelated = tmp.path().join("unrelated");
    let target = tmp.path().join("target");
    file::write(&source, "source")?;
    file::create_dir_all(&unrelated)?;
    file::make_symlink(&unrelated, &target)?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::Literal,
            path: source.to_string_lossy().to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: true,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    execute_flight_steps(
        &test_cask("tool", "1.0.0"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )?;

    assert_eq!(
        lexically_normalized_path(&resolve_symlink_target(
            &target,
            std::fs::read_link(&target)?
        )),
        source
    );
    assert!(std::fs::read_dir(unrelated)?.next().is_none());
    Ok(())
}

#[test]
fn forced_symlink_command_uses_replacement_flags() {
    let no_dereference = if cfg!(target_os = "macos") {
        "-h"
    } else {
        "-n"
    };
    assert_eq!(
        symlink_command_args(Path::new("source"), Path::new("target")),
        ["-s", "-f", no_dereference, "--", "source", "target"]
    );
}

#[test]
#[cfg(unix)]
fn flight_guards_match_homebrew_for_dangling_symlinks() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dangling = tmp.path().join("dangling");
    std::os::unix::fs::symlink("missing", &dangling)?;
    let cask = test_cask("example", "1.0.0");
    let path = FlightPath {
        base: FlightPathBase::Literal,
        path: dangling.to_string_lossy().to_string(),
    };

    assert!(!flight_guard_matches(
        &cask,
        &FlightGuard::IfExists(path.clone()),
        tmp.path(),
        Path::new("/Applications"),
    )?);
    assert!(flight_guard_matches(
        &cask,
        &FlightGuard::UnlessExists(path),
        tmp.path(),
        Path::new("/Applications"),
    )?);
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_replaces_dangling_target_without_force() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    file::write(&source, "source")?;
    file::make_symlink(Path::new("missing"), &target)?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::Literal,
            path: source.to_string_lossy().to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: vec![FlightGuard::UnlessExists(FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        })],
    };

    execute_flight_steps(
        &test_cask("example", "1.0.0"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )?;

    assert_eq!(
        lexically_normalized_path(&resolve_symlink_target(
            &target,
            std::fs::read_link(&target)?
        )),
        source
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_replaces_previous_owned_target_without_force() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let old_source = tmp.path().join("old");
    let new_source = stage.join("new");
    let target = tmp.path().join("target");
    file::create_dir_all(&stage)?;
    file::write(&old_source, "old")?;
    file::write(&new_source, "new")?;
    file::make_symlink(&old_source, &target)?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "new".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };
    let mut targets = FlightTargetTransaction::default();
    targets.previous_symlinks.insert(target.clone());

    execute_flight_step(
        &test_cask("example", "2.0.0"),
        &step,
        &stage,
        Path::new("/Applications"),
        &mut targets,
    )?;
    assert_eq!(std::fs::read_link(&target)?, new_source);

    targets.rollback()?;
    assert_eq!(std::fs::read_link(target)?, old_source);
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_rollback_removes_link_with_created_parent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let source = stage.join("source");
    let target = tmp.path().join("external/nested/target");
    file::create_dir_all(&stage)?;
    file::create_dir_all(tmp.path().join("external"))?;
    file::write(&source, "source")?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "source".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };
    let mut targets = FlightTargetTransaction::default();

    execute_flight_step(
        &test_cask("example", "1.0.0"),
        &step,
        &stage,
        Path::new("/Applications"),
        &mut targets,
    )?;
    assert!(target.symlink_metadata()?.file_type().is_symlink());

    targets.rollback()?;
    assert!(target.symlink_metadata().is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_force_refuses_to_replace_directory() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let source = tmp.path().join("source");
    let target = tmp.path().join("target");
    let link = target.join("source");
    file::write(&source, "source")?;
    file::create_dir_all(&link)?;
    file::write(link.join("keep"), "keep")?;
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::Literal,
            path: source.to_string_lossy().to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::Literal,
            path: target.to_string_lossy().to_string(),
        },
        force: true,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    let err = execute_flight_steps(
        &test_cask("example", "1.0.0"),
        &[step],
        tmp.path(),
        Path::new("/Applications"),
        "postflight_steps",
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("refusing to replace structured symlink directory"));
    assert_eq!(file::read_to_string(link.join("keep"))?, "keep");
    Ok(())
}

#[test]
fn flight_target_transaction_restores_replaced_target() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    file::write(&target, "original")?;
    {
        let mut transaction = FlightTargetTransaction::default();
        transaction.protect(&target)?;
        let backup = transaction.backups[0].backup.as_ref().unwrap();
        let recovery = flight_backup_recovery_path(backup);
        assert!(recovery.is_file());
        assert_ne!(recovery.parent(), backup.parent());
        file::write(&target, "replacement")?;
    }
    assert_eq!(file::read_to_string(target)?, "original");
    Ok(())
}

#[test]
fn flight_target_transaction_retries_failed_restore() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("missing/target");
    let backup = tmp.path().join("backup");
    file::write(&backup, "original")?;
    let recovery = flight_backup_recovery_path(&backup);
    let target_parent = resolved_parent(&target)?;
    let backup_parent = Some(resolved_parent(&backup)?);
    let record = FlightRecoveryRecord {
        target: target.clone(),
        backup: Some(backup.clone()),
        target_parent: target_parent.clone(),
        backup_parent: backup_parent.clone(),
        receipt_caskroom: None,
        elevate: true,
    };
    write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;
    let mut transaction = FlightTargetTransaction {
        backups: vec![ArtifactLinkBackup {
            target: target.clone(),
            backup: Some(backup.clone()),
            target_parent,
            backup_parent,
            elevate: true,
        }],
        receipt_caskroom: None,
        installed: Vec::new(),
        uninstall: BTreeMap::new(),
        previous_symlinks: BTreeSet::new(),
        copied_files: BTreeSet::new(),
        previous_directories: BTreeSet::new(),
        installed_directories: Vec::new(),
        committed: false,
    };

    assert!(transaction.rollback().is_err());
    assert_eq!(transaction.backups.len(), 1);
    assert!(backup.is_file());
    assert!(recovery.is_file());

    file::create_dir_all(target.parent().unwrap())?;
    transaction.rollback()?;
    assert!(transaction.backups.is_empty());
    assert_eq!(file::read_to_string(target)?, "original");
    assert!(recovery.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn recovers_interrupted_flight_target_transaction() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    file::write(&target, "original")?;
    let mut transaction = FlightTargetTransaction::default();
    transaction.protect(&target)?;
    let backup = transaction.backups[0].backup.as_ref().unwrap();
    let recovery = flight_backup_recovery_path(backup);
    std::mem::forget(transaction);

    recover_flight_backup(&recovery)?;

    assert_eq!(file::read_to_string(&target)?, "original");
    assert!(recovery.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn recovers_interrupted_new_flight_target() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    let mut transaction = FlightTargetTransaction::default();
    transaction.protect(&target)?;
    let recovery = flight_absent_recovery_path(&target);
    assert!(recovery.is_file());
    file::make_symlink(Path::new("source"), &target)?;
    std::mem::forget(transaction);

    recover_flight_backup(&recovery)?;

    assert!(target.symlink_metadata().is_err());
    assert!(recovery.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn interrupted_new_flight_target_preserves_completed_receipt() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    let caskroom = tmp.path().join("Caskroom/example/1.0.0");
    file::create_dir_all(&caskroom)?;
    let mut transaction = FlightTargetTransaction::default();
    transaction.receipt_caskroom = Some(caskroom.clone());
    transaction.protect(&target)?;
    let recovery = flight_absent_recovery_path(&target);
    file::make_symlink(Path::new("source"), &target)?;
    let receipt = CaskReceipt {
        schema_version: 3,
        version: "1.0.0".to_string(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: Vec::new(),
        binaries: Vec::new(),
        fonts: Vec::new(),
        completions: Vec::new(),
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets: vec![CaskTargetRecord {
            path: target.clone(),
            fingerprint: cask_target_fingerprint(&target)?,
            uninstall: Some(true),
        }],
        prune_safe: false,
        prune_blocker: None,
    };
    file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;
    std::mem::forget(transaction);

    recover_flight_backup(&recovery)?;

    assert!(target.is_symlink());
    assert!(recovery.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn interrupted_flight_recovery_preserves_recreated_target_and_backup() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    file::write(&target, "original")?;
    let mut transaction = FlightTargetTransaction::default();
    transaction.protect(&target)?;
    let backup = transaction.backups[0].backup.as_ref().unwrap().clone();
    let recovery = flight_backup_recovery_path(&backup);
    file::write(&target, "recreated")?;
    std::mem::forget(transaction);

    recover_flight_backup(&recovery)?;

    assert_eq!(file::read_to_string(&target)?, "recreated");
    assert_eq!(file::read_to_string(&backup)?, "original");
    assert!(recovery.is_file());

    let mut retry = FlightTargetTransaction::default();
    let err = retry.protect(&target).unwrap_err().to_string();
    assert!(err.contains("unresolved recovery"));
    assert_eq!(file::read_to_string(&backup)?, "original");

    file::remove_all(&target)?;
    recover_flight_backup(&recovery)?;
    assert_eq!(file::read_to_string(&target)?, "original");
    assert!(backup.symlink_metadata().is_err());
    assert!(recovery.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn invalid_flight_recovery_does_not_block_later_installs() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("missing/target");
    let backup = tmp.path().join("backup");
    file::write(&backup, "original")?;
    let recovery = flight_backup_recovery_path(&backup);
    let record = FlightRecoveryRecord {
        target,
        backup: Some(backup.clone()),
        target_parent: tmp.path().join("unexpected-target-parent"),
        backup_parent: Some(resolved_parent(&backup)?),
        receipt_caskroom: None,
        elevate: true,
    };
    write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;

    recover_flight_backup_or_warn(&recovery);

    assert!(recovery.is_file());
    assert_eq!(file::read_to_string(backup)?, "original");
    file::remove_all(recovery)?;
    Ok(())
}

#[test]
fn flight_recovery_rejects_backup_without_recorded_parent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    let backup = tmp.path().join("backup");
    file::write(&backup, "original")?;
    let recovery = flight_backup_recovery_path(&backup);
    let record = FlightRecoveryRecord {
        target: target.clone(),
        backup: Some(backup.clone()),
        target_parent: resolved_parent(&target)?,
        backup_parent: None,
        receipt_caskroom: None,
        elevate: true,
    };
    write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;

    let err = recover_flight_backup(&recovery).unwrap_err().to_string();

    assert!(err.contains("refusing to restore flight target through a changed parent"));
    assert!(backup.is_file());
    assert!(recovery.is_file());
    Ok(())
}

#[test]
fn flight_commit_reports_backup_without_recorded_parent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let target = tmp.path().join("target");
    let backup = tmp.path().join("backup");
    file::write(&backup, "original")?;
    let mut transaction = FlightTargetTransaction {
        backups: vec![ArtifactLinkBackup {
            target: target.clone(),
            backup: Some(backup.clone()),
            target_parent: resolved_parent(&target)?,
            backup_parent: None,
            elevate: false,
        }],
        receipt_caskroom: None,
        installed: Vec::new(),
        uninstall: BTreeMap::new(),
        previous_symlinks: BTreeSet::new(),
        copied_files: BTreeSet::new(),
        previous_directories: BTreeSet::new(),
        installed_directories: Vec::new(),
        committed: false,
    };

    let err = transaction.commit().unwrap_err().to_string();

    assert!(err.contains("has no recorded parent"));
    assert!(backup.is_file());
    Ok(())
}

#[test]
#[cfg(unix)]
fn stale_flight_recovery_temp_file_does_not_block_recovery() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let tmp = tempfile::tempdir()?;
    let recovery_root = tmp.path().join("recovery");
    let stale = recovery_root.join("stale.tmp");
    file::create_dir_all(&stale)?;
    file::write(stale.join("locked"), "content")?;
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o000))?;

    let target = tmp.path().join("target");
    let backup = tmp.path().join("backup");
    file::write(&backup, "original")?;
    let recovery = recovery_root.join("valid.recovery");
    let record = FlightRecoveryRecord {
        target: target.clone(),
        backup: Some(backup.clone()),
        target_parent: resolved_parent(&target)?,
        backup_parent: Some(resolved_parent(&backup)?),
        receipt_caskroom: None,
        elevate: true,
    };
    write_durable_file(&recovery, &serde_json::to_vec_pretty(&record)?)?;

    recover_flight_backups_in(&recovery_root)?;

    assert_eq!(file::read_to_string(target)?, "original");
    assert!(recovery.symlink_metadata().is_err());
    assert!(stale.symlink_metadata().is_ok());
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn flight_target_rollback_rejects_swapped_parent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("prefix");
    let target = prefix.join("bin/example");
    file::create_dir_all(target.parent().unwrap())?;
    file::write(&target, "original")?;
    let mut transaction = FlightTargetTransaction::default();
    transaction.protect(&target)?;

    let saved_prefix = tmp.path().join("saved-prefix");
    file::rename(&prefix, &saved_prefix)?;
    let external = tmp.path().join("external");
    file::create_dir_all(external.join("bin"))?;
    let external_target = external.join("bin/example");
    file::write(&external_target, "external")?;
    file::make_symlink(&external, &prefix)?;

    assert!(transaction.rollback().is_err());
    assert_eq!(file::read_to_string(&external_target)?, "external");
    assert_eq!(transaction.backups.len(), 1);

    file::remove_file(&prefix)?;
    file::rename(&saved_prefix, &prefix)?;
    transaction.rollback()?;
    assert_eq!(file::read_to_string(&target)?, "original");
    Ok(())
}

#[test]
#[cfg(unix)]
fn flight_target_backup_survives_app_replacement() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let app = tmp.path().join("Example.app");
    let target = app.join("Contents/MacOS/example-link");
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(Path::new("original"), &target)?;
    let mut transaction = FlightTargetTransaction::default();

    transaction.protect(&target)?;
    let backup = transaction.backups[0].backup.as_ref().unwrap().clone();
    assert_eq!(backup.parent(), app.parent());
    file::remove_all(&app)?;
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(Path::new("replacement"), &target)?;

    transaction.rollback()?;
    assert_eq!(std::fs::read_link(target)?, PathBuf::from("original"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn receipt_flight_symlinks_exclude_standard_and_drifted_targets() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let binary = tmp.path().join("bin/example");
    let flight = tmp.path().join("share/example");
    let retained = tmp.path().join("share/retained");
    let drifted = tmp.path().join("share/drifted");
    file::create_dir_all(binary.parent().unwrap())?;
    file::create_dir_all(flight.parent().unwrap())?;
    file::make_symlink(Path::new("binary-source"), &binary)?;
    file::make_symlink(Path::new("flight-source"), &flight)?;
    file::make_symlink(Path::new("retained-source"), &retained)?;
    file::make_symlink(Path::new("original-source"), &drifted)?;
    let records = [&binary, &flight, &retained, &drifted]
        .into_iter()
        .map(|path| {
            Ok(CaskTargetRecord {
                path: path.clone(),
                fingerprint: cask_target_fingerprint(path)?,
                uninstall: if path == &retained {
                    Some(false)
                } else if path == &binary {
                    None
                } else {
                    Some(true)
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    file::remove_file(&drifted)?;
    file::make_symlink(Path::new("changed-source"), &drifted)?;
    let receipt = CaskReceipt {
        schema_version: 3,
        version: "1.0.0".to_string(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: Vec::new(),
        binaries: vec![binary],
        fonts: Vec::new(),
        completions: Vec::new(),
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets: records,
        prune_safe: false,
        prune_blocker: None,
    };

    assert_eq!(receipt_flight_symlink_targets(&receipt)?, vec![flight]);
    Ok(())
}

#[test]
fn staged_symlink_sources_become_caskroom_owned() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let source = stage.join("AndroidNDK.app/Contents/NDK");
    file::create_dir_all(&source)?;
    file::write(source.join("ndk-build"), "binary")?;
    let temporary_caskroom = tmp.path().join("Caskroom/android-ndk/.mise-tmp");
    let target = tmp.path().join("share/android-ndk");
    let mut targets = FlightTargetTransaction::default();
    targets.protect(&target)?;
    file::create_dir_all(
        target
            .parent()
            .ok_or_else(|| eyre!("missing target parent"))?,
    )?;
    file::make_symlink(&source, &target)?;
    targets.record_installed(target.clone());

    durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

    assert_eq!(
        std::fs::read_link(&target)?,
        temporary_caskroom.join(".homebrew-staged/AndroidNDK.app/Contents/NDK")
    );
    assert!(
        temporary_caskroom
            .join(".homebrew-staged/AndroidNDK.app/Contents/NDK/ndk-build")
            .is_file()
    );
    targets.commit()?;
    Ok(())
}

#[test]
fn staged_symlink_source_copies_reachable_internal_links() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let source = stage.join("pkg/bin");
    let shared = stage.join("shared/data");
    file::create_dir_all(&source)?;
    file::create_dir_all(&shared)?;
    file::write(shared.join("value"), "content")?;
    file::make_symlink(&shared, &source.join("absolute"))?;
    file::make_symlink(Path::new("../../shared/data"), &source.join("relative"))?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let target = tmp.path().join("share/example");
    let mut targets = FlightTargetTransaction::default();
    targets.protect(&target)?;
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(&source, &target)?;
    targets.record_installed(target.clone());

    durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

    let owned_stage = temporary_caskroom.join(".homebrew-staged");
    assert_eq!(
        std::fs::read_link(owned_stage.join("pkg/bin/absolute"))?,
        owned_stage.join("shared/data")
    );
    assert_eq!(
        std::fs::read_link(owned_stage.join("pkg/bin/relative"))?,
        PathBuf::from("../../shared/data")
    );
    assert_eq!(
        file::read_to_string(owned_stage.join("pkg/bin/absolute/value"))?,
        "content"
    );
    assert_eq!(
        file::read_to_string(owned_stage.join("pkg/bin/relative/value"))?,
        "content"
    );
    targets.commit()?;
    Ok(())
}

#[test]
fn staged_symlink_source_preserves_link_to_external_referent() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let external = tmp.path().join("external");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&external)?;
    file::write(external.join("value"), "content")?;
    let staged_link = stage.join("external-link");
    file::make_symlink(&external, &staged_link)?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let target = tmp.path().join("share/example");
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(&staged_link, &target)?;
    let mut targets = FlightTargetTransaction::default();
    targets.protect(&target)?;
    file::make_symlink(&staged_link, &target)?;
    targets.record_installed(target.clone());

    durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

    let durable = temporary_caskroom.join(".homebrew-staged/external-link");
    assert_eq!(std::fs::read_link(&target)?, durable);
    assert_eq!(std::fs::read_link(&durable)?, external);
    file::remove_all(&stage)?;
    assert_eq!(file::read_to_string(target.join("value"))?, "content");
    targets.commit()?;
    Ok(())
}

#[test]
fn staged_symlink_source_accepts_canonical_stage_spelling() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let real_parent = tmp.path().join("real");
    let real_stage = real_parent.join("stage");
    file::create_dir_all(&real_stage)?;
    file::write(real_stage.join("value"), "content")?;
    let alias_parent = tmp.path().join("alias");
    file::make_symlink(&real_parent, &alias_parent)?;
    let stage = alias_parent.join("stage");
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let target = tmp.path().join("share/example");
    file::create_dir_all(target.parent().unwrap())?;
    let mut targets = FlightTargetTransaction::default();
    targets.protect(&target)?;
    file::make_symlink(&real_stage, &target)?;
    targets.record_installed(target.clone());

    durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

    assert_eq!(file::read_to_string(target.join("value"))?, "content");
    targets.commit()?;
    Ok(())
}

#[test]
fn staged_artifact_closure_merges_a_parent_after_its_child() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let parent = stage.join("parent");
    file::create_dir_all(&parent)?;
    file::write(parent.join("first"), "first")?;
    file::write(parent.join("second"), "second")?;
    let owned = tmp.path().join("owned");

    copy_staged_artifact_closure(&stage, &owned, &parent.join("first"))?;
    copy_staged_artifact_closure(&stage, &owned, &parent)?;

    assert_eq!(file::read_to_string(owned.join("parent/first"))?, "first");
    assert_eq!(file::read_to_string(owned.join("parent/second"))?, "second");
    Ok(())
}

#[test]
#[cfg(unix)]
fn staged_artifact_closure_rejects_intermediate_symlink_escape() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let outside = tmp.path().join("outside");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&outside)?;
    file::write(outside.join("secret"), "secret")?;
    file::make_symlink(&outside, &stage.join("link"))?;
    let owned = tmp.path().join("owned");

    let err = copy_staged_artifact_closure(&stage, &owned, &stage.join("link/secret"))
        .unwrap_err()
        .to_string();

    assert!(err.contains("escaped extraction root"));
    assert!(owned.symlink_metadata().is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn structured_symlink_inside_stage_uses_relative_source() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(stage.join("source"))?;
    let link = stage.join("nested/link");
    let step = FlightStep::Symlink {
        source: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "source".to_string(),
        },
        target: FlightPath {
            base: FlightPathBase::StagedPath,
            path: "nested/link".to_string(),
        },
        force: false,
        uninstall: false,
        source_glob: false,
        sudo: FlightSudo::Never,
        guards: Vec::new(),
    };

    execute_flight_steps(
        &test_cask("example", "1.0.0"),
        &[step],
        &stage,
        Path::new("/Applications"),
        "preflight_steps",
    )?;

    assert_eq!(std::fs::read_link(link)?, PathBuf::from("../source"));
    Ok(())
}

#[test]
fn internal_staged_symlinks_remain_relative_and_untracked() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let source = stage.join("Example.app/Contents/Resources/data");
    let link = stage.join("Example.app/Contents/data");
    file::create_dir_all(&source)?;
    file::create_dir_all(link.parent().unwrap())?;
    file::make_symlink(Path::new("Resources/data"), &link)?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let mut targets = FlightTargetTransaction::default();
    targets.record_installed(link.clone());

    durabilize_staged_symlink_targets(&stage, &temporary_caskroom, &mut targets)?;

    assert_eq!(std::fs::read_link(&link)?, PathBuf::from("Resources/data"));
    assert!(temporary_caskroom.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn temporary_caskroom_symlink_sources_follow_activation() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let final_caskroom = tmp.path().join("Caskroom/example/1.0.0");
    let source = temporary_caskroom.join("bin/example");
    file::create_dir_all(source.parent().unwrap())?;
    file::write(&source, "binary")?;
    let target = tmp.path().join("bin/example");
    file::create_dir_all(target.parent().unwrap())?;
    let mut targets = FlightTargetTransaction::default();
    targets.protect(&target)?;
    file::make_symlink(&source, &target)?;
    targets.record_installed(target.clone());
    file::rename(&temporary_caskroom, &final_caskroom)?;

    retarget_transient_symlinks(
        &temporary_caskroom,
        &final_caskroom,
        &final_caskroom,
        &targets,
    )?;

    assert_eq!(
        std::fs::read_link(target)?,
        final_caskroom.join("bin/example")
    );
    targets.commit()?;
    Ok(())
}

#[test]
fn internal_temporary_caskroom_symlinks_follow_activation() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let final_caskroom = tmp.path().join("Caskroom/example/1.0.0");
    let source = temporary_caskroom.join("share/example/source");
    let target = temporary_caskroom.join("share/example/target");
    file::create_dir_all(source.parent().unwrap())?;
    file::write(&source, "content")?;
    file::make_symlink(&source, &target)?;
    file::rename(&temporary_caskroom, &final_caskroom)?;
    let installed_target = final_caskroom.join("share/example/target");

    retarget_transient_symlinks(
        &temporary_caskroom,
        &final_caskroom,
        &final_caskroom,
        &FlightTargetTransaction::default(),
    )?;

    assert_eq!(
        std::fs::read_link(installed_target)?,
        final_caskroom.join("share/example/source")
    );
    Ok(())
}

#[test]
fn parses_zoom_terminate_process_step() -> Result<()> {
    let mut cask = test_cask("zoom", "7.1.5.84650");
    cask.artifacts = vec![
        serde_json::json!({"uninstall": [{"pkgutil": "us.zoom.pkg.videomeeting"}]}),
        serde_json::json!({"pkg": ["zoomusInstallerFull.pkg"]}),
        serde_json::json!({
            "postflight_steps": [{
                "steps": [{
                    "type": "terminate_process",
                    "name": "/Applications/zoom.us.app",
                    "match": "full",
                    "attempts": 3,
                    "notices": [
                        "The Zoom package postinstall script launches the Zoom app",
                        "Attempting to close zoom.us.app to avoid unwanted user intervention"
                    ],
                    "failure_message": "Unable to forcibly close zoom.us.app"
                }]
            }]
        }),
    ];

    assert_eq!(
        cask_artifacts(&cask)?.postflight_steps,
        vec![FlightStep::TerminateProcess {
            name: "/Applications/zoom.us.app".to_string(),
            match_mode: ProcessMatch::Full,
            sudo: false,
            attempts: 3,
            must_succeed: false,
            notices: vec![
                "The Zoom package postinstall script launches the Zoom app".to_string(),
                "Attempting to close zoom.us.app to avoid unwanted user intervention".to_string(),
            ],
            failure_message: Some("Unable to forcibly close zoom.us.app".to_string()),
        }]
    );
    Ok(())
}

#[test]
fn completed_flight_action_names_are_receipt_stable() -> Result<()> {
    let cask = test_cask("example", "1.0.0");
    let stage = tempfile::tempdir()?;
    let source = stage.path().join("obsolete");
    std::fs::write(&source, "remove me")?;
    let steps = vec![FlightStep::Remove {
        paths: vec![FlightPath {
            path: "obsolete".to_string(),
            base: FlightPathBase::StagedPath,
        }],
        recursive: false,
    }];
    let mut completed = Vec::new();
    let mut targets = FlightTargetTransaction::default();

    execute_flight_steps_with_completion(
        &cask,
        &steps,
        stage.path(),
        Path::new("/Applications"),
        "postflight_steps",
        &mut targets,
        |index, step| {
            completed.push(format!("postflight_steps[{index}]:{}", step.kind()));
            Ok(())
        },
    )?;

    assert_eq!(completed, ["postflight_steps[0]:remove"]);
    assert!(!source.exists());
    Ok(())
}

#[test]
fn terminate_process_has_explicit_completed_action_kind() {
    let step = FlightStep::TerminateProcess {
        name: "zoom.us.app".to_string(),
        match_mode: ProcessMatch::Name,
        sudo: false,
        attempts: 1,
        must_succeed: false,
        notices: Vec::new(),
        failure_message: None,
    };

    assert_eq!(step.kind(), "terminate_process");
}

#[test]
fn terminate_process_defaults_match_homebrew() -> Result<()> {
    let cask = test_cask("example", "1.0.0");
    let step = parse_flight_step(
        &cask,
        "postflight_steps",
        &serde_json::json!({"type": "terminate_process", "name": "Example"}),
    )?;
    assert_eq!(
        step,
        FlightStep::TerminateProcess {
            name: "Example".to_string(),
            match_mode: ProcessMatch::Name,
            sudo: false,
            attempts: 1,
            must_succeed: false,
            notices: Vec::new(),
            failure_message: None,
        }
    );
    Ok(())
}

#[test]
fn terminate_process_rejects_malformed_metadata() {
    let cask = test_cask("example", "1.0.0");
    let invalid = [
        serde_json::json!({"type": "terminate_process"}),
        serde_json::json!({"type": "terminate_process", "name": ""}),
        serde_json::json!({"type": "terminate_process", "name": "x", "match": "prefix"}),
        serde_json::json!({"type": "terminate_process", "name": "x", "attempts": 0}),
        serde_json::json!({"type": "terminate_process", "name": "x", "attempts": 1.5}),
        serde_json::json!({"type": "terminate_process", "name": "x", "sudo": "yes"}),
        serde_json::json!({"type": "terminate_process", "name": "x", "must_succeed": 1}),
        serde_json::json!({"type": "terminate_process", "name": "x", "notices": [1]}),
        serde_json::json!({"type": "terminate_process", "name": "x", "failure_message": 1}),
        serde_json::json!({"type": "terminate_process", "name": "x", "unknown": true}),
    ];
    for value in invalid {
        assert!(parse_flight_step(&cask, "postflight_steps", &value).is_err());
    }
}

#[test]
fn terminate_process_retries_with_direct_argv_and_nonfatal_exhaustion() -> Result<()> {
    let step = FlightStep::TerminateProcess {
        name: "{{appdir}}/Example.app".to_string(),
        match_mode: ProcessMatch::Full,
        sudo: true,
        attempts: 3,
        must_succeed: false,
        notices: vec!["Closing {{version}}".to_string()],
        failure_message: Some("Unable to close {{version}}".to_string()),
    };
    let mut calls = Vec::new();
    let mut sleeps = Vec::new();
    execute_terminate_process(
        &step,
        Path::new("/tmp/stage"),
        Path::new("/Applications"),
        "1.2.3",
        |command, args, sudo| {
            calls.push((command.to_path_buf(), args.to_vec(), sudo));
            Err(eyre!("still running"))
        },
        |duration| sleeps.push(duration),
    )?;
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|(command, args, sudo)| {
        command == Path::new("/usr/bin/pkill")
            && args == &["-f", "/Applications/Example.app"]
            && *sudo
    }));
    assert_eq!(sleeps, vec![std::time::Duration::from_secs(1); 2]);
    Ok(())
}

#[test]
fn terminate_process_name_mode_stops_after_success() -> Result<()> {
    let step = FlightStep::TerminateProcess {
        name: "Example".to_string(),
        match_mode: ProcessMatch::Name,
        sudo: false,
        attempts: 3,
        must_succeed: true,
        notices: Vec::new(),
        failure_message: None,
    };
    let mut attempts = 0;
    execute_terminate_process(
        &step,
        Path::new("/tmp/stage"),
        Path::new("/Applications"),
        "1.0.0",
        |command, args, sudo| {
            attempts += 1;
            assert_eq!(command, Path::new("/usr/bin/killall"));
            assert_eq!(args, &["Example"]);
            assert!(!sudo);
            if attempts == 1 {
                Err(eyre!("retry"))
            } else {
                Ok(())
            }
        },
        |_| {},
    )?;
    assert_eq!(attempts, 2);
    Ok(())
}

#[test]
fn terminate_process_must_succeed_returns_final_error() {
    let step = FlightStep::TerminateProcess {
        name: "Example".to_string(),
        match_mode: ProcessMatch::Name,
        sudo: false,
        attempts: 1,
        must_succeed: true,
        notices: Vec::new(),
        failure_message: None,
    };
    let err = execute_terminate_process(
        &step,
        Path::new("/tmp/stage"),
        Path::new("/Applications"),
        "1.0.0",
        |_, _, _| Err(eyre!("still running")),
        |_| {},
    )
    .unwrap_err();
    assert!(err.to_string().contains("still running"));
}

#[test]
fn structured_run_expands_paths_args_and_env() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let staged = tmp.path().join("stage");
    let appdir = tmp.path().join("Applications");
    file::create_dir_all(&staged)?;
    file::create_dir_all(&appdir)?;
    let result = staged.join("result");

    execute_flight_steps(
        &test_cask("example", "1.2.3"),
        &[FlightStep::Run {
            command: FlightPath {
                base: FlightPathBase::Literal,
                path: "/bin/sh".to_string(),
            },
            args: vec![
                "-c".to_string(),
                "printf '%s' \"$MISE_TEST:$1:$2:$3\" > \"$4\"".to_string(),
                "_".to_string(),
                "{{appdir}}".to_string(),
                "{{staged_path}}".to_string(),
                "{{HOMEBREW_PREFIX}}".to_string(),
                "{{staged_path}}/result".to_string(),
            ],
            env: BTreeMap::from([("MISE_TEST".to_string(), "version-{{version}}".to_string())]),
            sudo: false,
            guards: Vec::new(),
        }],
        &staged,
        &appdir,
        "postflight_steps",
    )?;

    assert_eq!(
        file::read_to_string(result)?,
        format!(
            "version-1.2.3:{}:{}:{}",
            appdir.display(),
            staged.display(),
            prefix.display()
        )
    );
    Ok(())
}

#[test]
fn structured_flight_steps_move_and_remove_staged_paths() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let staged = tmp.path();
    let bundle_dir = staged.join("WezTerm-nightly");
    let app = bundle_dir.join("WezTerm.app");
    file::create_dir_all(&app)?;

    execute_flight_steps(
        &test_cask("wezterm@nightly", "latest"),
        &[
            FlightStep::Move {
                source: FlightPath {
                    base: FlightPathBase::StagedPath,
                    path: "{WezTerm-*,wezterm-*}/WezTerm.app".to_string(),
                },
                target: FlightPath {
                    base: FlightPathBase::StagedPath,
                    path: ".".to_string(),
                },
                source_glob: true,
            },
            FlightStep::Remove {
                paths: vec![
                    FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: "WezTerm-*".to_string(),
                    },
                    FlightPath {
                        base: FlightPathBase::StagedPath,
                        path: "wezterm-*".to_string(),
                    },
                ],
                recursive: true,
            },
        ],
        staged,
        staged,
        "preflight_steps",
    )?;

    assert!(staged.join("WezTerm.app").is_dir());
    assert!(!bundle_dir.exists());
    Ok(())
}

#[test]
fn rejects_unsupported_structured_flight_steps() {
    let mut cask = test_cask("battle-net", "1.0.0");
    cask.artifacts = vec![
        serde_json::json!({
            "preflight_steps": [{
                "steps": [{
                    "type": "set_permissions",
                    "paths": [{"base": "staged_path", "path": "Battle.net-Setup.app"}],
                    "permissions": "a+x"
                }]
            }]
        }),
        serde_json::json!({"app": "Battle.net.app"}),
    ];

    let err = cask_artifacts(&cask).unwrap_err().to_string();
    assert!(err.contains("unsupported preflight_steps step type set_permissions"));
}

#[test]
fn rejects_structured_flight_step_group_controls() {
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![
        serde_json::json!({
            "preflight_steps": [{
                "if": {"arch": "arm64"},
                "steps": [{
                    "type": "remove",
                    "paths": [{"base": "staged_path", "path": "old"}]
                }]
            }]
        }),
        serde_json::json!({"app": "Example.app"}),
    ];

    let err = cask_artifacts(&cask).unwrap_err().to_string();
    assert!(err.contains("unsupported preflight_steps step group field if"));
}

#[test]
fn rejects_structured_flight_step_controls() {
    let mut cask = test_cask("miniconda", "25.5.1-1");
    cask.artifacts = vec![
        serde_json::json!({
            "postflight_steps": [{
                "steps": [{
                    "type": "remove",
                    "paths": [{"base": "staged_path", "path": "base/envs"}],
                    "recursive": true,
                    "guards": [{"condition": "if_exists", "path": "{{temp}}/miniconda-envs"}]
                }]
            }]
        }),
        serde_json::json!({"pkg": ["Miniconda.pkg"]}),
        serde_json::json!({"uninstall": [{"pkgutil": "com.anaconda.pkg"}]}),
    ];

    let err = cask_artifacts(&cask).unwrap_err().to_string();
    assert!(err.contains("unsupported postflight_steps remove step field guards"));
}

#[test]
fn structured_flight_boole_report_their_actual_context() {
    let cask = test_cask("example", "1.0.0");
    let copy = serde_json::json!({
        "type": "copy",
        "source": {"path": "source"},
        "target": {"path": "target"},
        "recursive": "yes"
    });
    let err = parse_flight_step(&cask, "preflight_steps", &copy)
        .unwrap_err()
        .to_string();
    assert!(err.contains("preflight_steps recursive must be a boolean"));
    assert!(!err.contains("terminate_process"));

    let run = serde_json::json!({
        "type": "run",
        "command": {"base": "staged_path", "path": "tool"},
        "sudo": "if_needed"
    });
    let err = parse_flight_step(&cask, "postflight_steps", &run)
        .unwrap_err()
        .to_string();
    assert!(err.contains("postflight_steps sudo must be a boolean"));
}

#[test]
fn rejects_baseless_relative_run_command_paths() {
    let cask = test_cask("example", "1.0.0");
    for path in ["bin/tool", "../tool", "./tool"] {
        let value = serde_json::json!({"path": path});
        let err = parse_run_command(&cask, "preflight_steps", Some(&value))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("invalid preflight_steps run command path"),
            "{err}"
        );
    }
}

#[test]
fn accepts_baseless_bare_and_absolute_run_commands() -> Result<()> {
    let cask = test_cask("example", "1.0.0");
    for path in ["xattr", "/usr/bin/xattr"] {
        let value = serde_json::json!({"path": path});
        assert_eq!(
            parse_run_command(&cask, "preflight_steps", Some(&value))?,
            FlightPath {
                base: FlightPathBase::Literal,
                path: path.to_string(),
            }
        );
    }
    Ok(())
}

#[test]
fn ensure_cask_shim_creates_parent_dir() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let shim_path = tmp.path().join("missing").join("cask_shim.rb");

    ensure_cask_shim(&shim_path)?;

    assert_eq!(file::read_to_string(&shim_path)?, CASK_SHIM_RB);
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_supports_language_and_system_conditionals() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    let result = tmp.path().join("result");
    file::write(&shim, CASK_SHIM_RB)?;
    file::write(
        &cask,
        r##"cask "example" do
  version "1.0.0"
  language "fr" do
    "fr"
  end
  language "en", default: true do
    "en-US"
  end
  suffix = on_system_conditional linux: "-linux", macos: "-macos"
  preflight do
    File.write staged_path/"result", "#{language}#{suffix}"
  end
end
"##,
    )?;

    let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let suffix = if cfg!(target_os = "macos") {
        "-macos"
    } else {
        "-linux"
    };
    assert_eq!(file::read_to_string(result)?, format!("en-US{suffix}"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_supports_csv_version_array_helpers() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    let result = tmp.path().join("result");
    file::write(&shim, CASK_SHIM_RB)?;
    file::write(
        &cask,
        r#"cask "example" do
  version "2.2.1,20628"
  url "https://example.com/OrbStack_v#{version.csv.first}_#{version.csv.second}.dmg"
  auto_updates true
  preflight do
    File.write staged_path/"result", version.csv.second
  end
end
"#,
    )?;

    let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "2.2.1,20628")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(file::read_to_string(result)?, "20628");
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_supports_completion_stanzas_and_system_command() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    file::write(&shim, CASK_SHIM_RB)?;
    crate::file::write(tmp.path().join("kubectl"), "kubectl")?;
    // Modeled on the docker-desktop cask: completion stanzas plus a
    // postflight that symlinks kubectl via system_command.
    file::write(
        &cask,
        r##"cask "example" do
  version "1.0.0"
  app "Example.app"
  binary "#{appdir}/Example.app/Contents/Resources/bin/example"
  bash_completion "#{appdir}/Example.app/Contents/Resources/etc/example.bash-completion"
  zsh_completion "#{appdir}/Example.app/Contents/Resources/etc/example.zsh-completion"
  fish_completion "#{appdir}/Example.app/Contents/Resources/etc/example.fish-completion"
  manpage "#{appdir}/Example.app/Contents/Resources/man/example.1"
  postflight do
    kubectl_target = staged_path/"kubectl-link"
    next if kubectl_target.exist?
    system_command "/bin/ln", args: ["-sfn", staged_path/"kubectl", kubectl_target],
                              sudo: false
    echoed = system_command "/bin/echo", args: ["-n", "hello"], print_stderr: false
    File.write staged_path/"result", echoed.stdout if echoed.success?
    # A no-args executable whose path contains spaces and shell
    # metacharacters must run via argv, not a shell command line.
    spaced = system_command staged_path/"my tool $HOME"
    File.write staged_path/"spaced-result", spaced.stdout
  end
end
"##,
    )?;
    let spaced_tool = tmp.path().join("my tool $HOME");
    crate::file::write(&spaced_tool, "#!/bin/sh\nprintf spaced-ok\n")?;
    file::make_executable(&spaced_tool)?;

    let output = run_cask_shim_hook(&ruby, &shim, &cask, tmp.path(), "1.0.0", "postflight")?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_link(tmp.path().join("kubectl-link"))?,
        tmp.path().join("kubectl")
    );
    assert_eq!(file::read_to_string(tmp.path().join("result"))?, "hello");
    assert_eq!(
        file::read_to_string(tmp.path().join("spaced-result"))?,
        "spaced-ok"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_system_command_reports_denied_sudo() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    file::write(&shim, CASK_SHIM_RB)?;
    file::write(
        &cask,
        r#"cask "example" do
  version "1.0.0"
  preflight do
    system_command "/usr/bin/true", args: ["--flag"], sudo: true
  end
end
"#,
    )?;

    // MISE_BREW_CASK_SUDO is unset, which must behave as "deny".
    let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
    if nix::unistd::geteuid().is_root() {
        // root never needs to elevate, so the hook succeeds
        assert!(output.status.success());
        return Ok(());
    }
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("needs sudo"), "{stderr}");
    assert!(stderr.contains("sudo /usr/bin/true --flag"), "{stderr}");
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_system_command_reports_failed_commands() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    file::write(&shim, CASK_SHIM_RB)?;
    file::write(
        &cask,
        r#"cask "example" do
  version "1.0.0"
  preflight do
    system_command "/usr/bin/false"
  end
end
"#,
    )?;

    let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("command failed (exit 1): /usr/bin/false"),
        "{stderr}"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn cask_shim_reports_missing_system_conditional() -> Result<()> {
    let Some(ruby) = file::which("ruby") else {
        return Ok(());
    };
    let tmp = tempfile::tempdir()?;
    let shim = tmp.path().join("cask_shim.rb");
    let cask = tmp.path().join("example.rb");
    let (conditional, platform) = if cfg!(target_os = "macos") {
        ("linux: \"-linux\"", "macos")
    } else {
        ("macos: \"-macos\"", "linux")
    };
    file::write(&shim, CASK_SHIM_RB)?;
    file::write(
        &cask,
        format!("cask \"example\" do\n  on_system_conditional {conditional}\nend\n"),
    )?;

    let output = run_cask_shim(&ruby, &shim, &cask, tmp.path(), "1.0.0")?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "Error: cask uses `on_system_conditional without {platform}`"
    )));
    Ok(())
}

#[test]
fn detects_suffixless_zip_archives() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("stable");
    std::fs::write(&archive, b"PK\x03\x04suffixless zip")?;

    assert_eq!(
        cask_extraction_format(&archive, "visual-studio-code-1.127.0-stable")?,
        ExtractionFormat::Zip
    );
    Ok(())
}

#[test]
fn detects_suffixless_dmg_archives() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("download");
    let mut contents = vec![0; 1024];
    contents[512..524].copy_from_slice(b"koly\0\0\0\x04\0\0\x02\0");
    std::fs::write(&archive, contents)?;

    assert!(is_dmg_archive(&archive, "raycast-1.104.24-download")?);
    Ok(())
}

#[test]
fn rejects_malformed_suffixless_dmg_trailers() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("download");
    let mut contents = vec![0; 1024];
    contents[512..520].copy_from_slice(b"koly\0\0\0\x04");
    std::fs::write(&archive, contents)?;

    assert!(!is_dmg_archive(&archive, "raycast-1.104.24-download")?);
    Ok(())
}

#[test]
fn does_not_sniff_named_archives_as_dmg() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("archive.zip");
    let mut contents = vec![0; 1024];
    contents[..4].copy_from_slice(b"PK\x03\x04");
    contents[512..524].copy_from_slice(b"koly\0\0\0\x04\0\0\x02\0");
    std::fs::write(&archive, contents)?;

    assert!(!is_dmg_archive(&archive, "archive.zip")?);
    assert_eq!(
        cask_extraction_format(&archive, "archive.zip")?,
        ExtractionFormat::Zip
    );
    Ok(())
}

#[test]
fn leaves_suffixless_raw_binaries_raw() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("claude");
    let mut contents = vec![0; 1024];
    contents[..10].copy_from_slice(b"#!/bin/sh\n");
    std::fs::write(&archive, contents)?;

    assert!(!is_dmg_archive(&archive, "claude-1.0.0-claude")?);
    assert_eq!(
        cask_extraction_format(&archive, "claude-1.0.0-claude")?,
        ExtractionFormat::Raw
    );
    Ok(())
}

#[test]
fn stages_bare_pkg_using_declared_artifact_name() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("2026.7.1343.0");
    std::fs::write(&archive, b"xar!bare pkg")?;
    let mut cask = test_cask("cloudflare-warp", "2026.7.1343.0");
    cask.url = "https://example.com/version/2026.7.1343.0".to_string();
    cask.artifacts = vec![
        serde_json::json!({"pkg": ["Cloudflare_WARP_2026.7.1343.0.pkg"]}),
        serde_json::json!({"uninstall": [{"pkgutil": "com.cloudflare.warp"}]}),
    ];

    assert_eq!(
        raw_cask_artifact_name(&cask, &archive, "cached-name")?,
        ("Cloudflare_WARP_2026.7.1343.0.pkg".to_string(), false)
    );
    Ok(())
}

#[test]
fn detects_a_single_nested_dmg() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let dmg = tmp.path().join("Display Pilot 2Setup.dmg");
    std::fs::write(&dmg, b"nested dmg")?;

    assert_eq!(single_nested_cask_archive(tmp.path())?, Some(dmg));
    Ok(())
}

#[test]
fn raw_executable_keeps_url_filename() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("cached-name");
    std::fs::write(&archive, b"#!/bin/sh\n")?;
    let mut cask = test_cask("claude", "1.0.0");
    cask.url = "https://example.com/claude".to_string();

    assert_eq!(
        raw_cask_artifact_name(&cask, &archive, "cached-name")?,
        ("claude".to_string(), true)
    );
    Ok(())
}

#[test]
fn detects_a_single_suffixless_nested_archive() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("download");
    std::fs::write(&archive, b"PK\x03\x04nested zip")?;

    assert_eq!(single_nested_cask_archive(tmp.path())?, Some(archive));
    Ok(())
}

#[test]
fn detects_a_single_nested_archive_with_macos_metadata() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("nested.zip");
    std::fs::write(&archive, b"PK\x03\x04nested zip")?;
    file::create_dir_all(tmp.path().join("__MACOSX"))?;
    std::fs::write(tmp.path().join("__MACOSX/._nested.zip"), b"metadata")?;

    assert_eq!(single_nested_cask_archive(tmp.path())?, Some(archive));
    Ok(())
}

#[test]
fn does_not_expand_unsupported_nested_formats() -> Result<()> {
    for filename in [
        "payload.gz",
        "archive.rar",
        "archive.tar.br",
        "archive.tar.lz4",
        "archive.tar.sz",
    ] {
        let tmp = tempfile::tempdir()?;
        std::fs::write(tmp.path().join(filename), b"unsupported")?;
        assert_eq!(single_nested_cask_archive(tmp.path())?, None);
    }
    Ok(())
}

#[test]
fn does_not_expand_multiple_or_raw_nested_files() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let archive = tmp.path().join("nested.zip");
    std::fs::write(&archive, b"PK\x03\x04nested zip")?;
    std::fs::write(tmp.path().join("readme.txt"), b"readme")?;
    assert_eq!(single_nested_cask_archive(tmp.path())?, None);

    file::remove_file(tmp.path().join("readme.txt"))?;
    file::remove_file(&archive)?;
    std::fs::write(tmp.path().join("binary"), b"#!/bin/sh\n")?;
    assert_eq!(single_nested_cask_archive(tmp.path())?, None);
    Ok(())
}

#[test]
fn artifact_lookup_ignores_macos_metadata_directories() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let metadata_app = tmp.path().join("__MACOSX/Pearcleaner.app");
    file::create_dir_all(&metadata_app)?;

    assert_eq!(find_app(tmp.path(), "Pearcleaner.app"), None);

    let app = tmp.path().join("Pearcleaner.app");
    file::create_dir_all(&app)?;

    assert_eq!(find_app(tmp.path(), "Pearcleaner.app"), Some(app));
    Ok(())
}

#[test]
fn artifact_lookup_matches_app_bundle_case_insensitively() -> Result<()> {
    // Homebrew cask `yaak` declares `app "yaak.app"` but the DMG ships
    // `Yaak.app`. Default macOS APFS is case-insensitive; exact match must
    // not be required.
    let tmp = tempfile::tempdir()?;
    let app = tmp.path().join("Yaak.app");
    file::create_dir_all(&app)?;

    assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app.clone()));
    assert_eq!(find_app(tmp.path(), "Yaak.app"), Some(app));
    assert_eq!(find_app(tmp.path(), "Other.app"), None);
    Ok(())
}

#[test]
fn artifact_lookup_prefers_exact_case_over_earlier_fallback() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let fallback = tmp.path().join("Yaak.app");
    let exact = fallback.join("Contents/yaak.app");
    file::create_dir_all(&exact)?;

    assert_eq!(find_app(tmp.path(), "yaak.app"), Some(exact));
    Ok(())
}

#[test]
fn artifact_lookup_skips_macos_metadata_for_case_insensitive_match() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    file::create_dir_all(tmp.path().join("__MACOSX/Yaak.app"))?;
    let app = tmp.path().join("Yaak.app");
    file::create_dir_all(&app)?;

    assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app));
    Ok(())
}

#[test]
fn find_app_ignores_file_that_matches_app_name() -> Result<()> {
    // A same-named regular file must not shadow a later .app directory.
    let tmp = tempfile::tempdir()?;
    std::fs::write(tmp.path().join("yaak.app"), b"not a bundle")?;
    let app = tmp.path().join("nested/Yaak.app");
    file::create_dir_all(&app)?;

    assert_eq!(find_app(tmp.path(), "yaak.app"), Some(app));
    Ok(())
}

#[cfg(unix)]
#[test]
fn artifact_lookup_resolves_through_a_flight_created_symlink() -> Result<()> {
    // gcloud-cli's last preflight step symlinks
    // `staged_path/google-cloud-sdk` at the SDK copied into the prefix, so
    // every `binary` source resolves only by traversing that link. The walk
    // cannot enter it.
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let installed = tmp.path().join("share/google-cloud-sdk");
    file::create_dir_all(&stage)?;
    file::create_dir_all(installed.join("bin"))?;
    crate::file::write(
        installed.join("bin/git-credential-gcloud.sh"),
        "credential helper",
    )?;
    std::os::unix::fs::symlink(&installed, stage.join("google-cloud-sdk"))?;

    // The artifact's real location, not the path through the link: callers
    // decide copy-vs-symlink from it, and the stage does not outlive the
    // install.
    assert_eq!(
        find_file_artifact(&stage, "google-cloud-sdk/bin/git-credential-gcloud.sh"),
        Some(file::desymlink_path(
            &installed.join("bin/git-credential-gcloud.sh")
        ))
    );
    Ok(())
}

#[test]
fn artifact_lookup_rejects_sources_that_escape_the_root() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    crate::file::write(tmp.path().join("outside"), "not ours")?;

    assert_eq!(find_file_artifact(&stage, "../outside"), None);
    assert_eq!(
        find_file_artifact(&stage, &tmp.path().join("outside").to_string_lossy()),
        None
    );
    Ok(())
}

#[test]
fn relative_artifact_path_refuses_names_it_cannot_contain() {
    let root = Path::new("/stage");

    assert_eq!(
        relative_artifact_path(root, Path::new("bin/op")),
        Some(PathBuf::from("/stage/bin/op"))
    );
    assert_eq!(
        relative_artifact_path(root, Path::new("./bin/op")),
        Some(PathBuf::from("/stage/./bin/op"))
    );
    // Names that would resolve to `root` itself, which `find_app`'s
    // directory predicate would accept as the bundle.
    assert_eq!(relative_artifact_path(root, Path::new("")), None);
    assert_eq!(relative_artifact_path(root, Path::new(".")), None);
    assert_eq!(relative_artifact_path(root, Path::new("./")), None);
    // Escapes.
    assert_eq!(relative_artifact_path(root, Path::new("../op")), None);
    assert_eq!(
        relative_artifact_path(root, Path::new("bin/../../op")),
        None
    );
    assert_eq!(relative_artifact_path(root, Path::new("/etc/passwd")), None);
    // Resource-fork copies the walk skips.
    assert_eq!(
        relative_artifact_path(root, Path::new("__MACOSX/Yaak.app")),
        None
    );
    assert_eq!(
        relative_artifact_path(root, Path::new("payload/__MACOSX/op")),
        None
    );
}

#[test]
fn path_ends_with_ignore_ascii_case_matches_components() {
    assert!(path_ends_with_ignore_ascii_case(
        Path::new("payload/Yaak.app"),
        Path::new("yaak.app")
    ));
    assert!(path_ends_with_ignore_ascii_case(
        Path::new("Yaak.app"),
        Path::new("yaak.app")
    ));
    assert!(!path_ends_with_ignore_ascii_case(
        Path::new("Yaak.app"),
        Path::new("Other.app")
    ));
    assert!(!path_ends_with_ignore_ascii_case(
        Path::new("Yaak.app"),
        Path::new("")
    ));
    assert!(!path_ends_with_ignore_ascii_case(
        Path::new("Yaak.app"),
        Path::new("/Yaak.app")
    ));
}

#[test]
fn maps_preflight_generated_wrapper_from_extract_stage() -> Result<()> {
    // VLC: preflight writes `#{staged_path}/vlc.wrapper.sh` while preflight
    // staged_path is the extract stage, not the temp Caskroom. API binary
    // source is `$HOMEBREW_PREFIX/Caskroom/vlc/<ver>/vlc.wrapper.sh`.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("vlc", "3.0.23");
    let stage = tmp.path().join("extract");
    let tmp_caskroom = tmp.path().join("tmp-caskroom");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&tmp_caskroom)?;
    let wrapper = stage.join("vlc.wrapper.sh");
    std::fs::write(&wrapper, "#!/bin/sh\n")?;

    let binary = BinaryArtifact {
        source: "$HOMEBREW_PREFIX/Caskroom/vlc/3.0.23/vlc.wrapper.sh".to_string(),
        target: Some("vlc".to_string()),
    };

    assert_eq!(
        find_binary_source(&stage, &tmp_caskroom, &cask, &binary)?,
        wrapper
    );
    Ok(())
}

#[test]
fn prefers_temp_caskroom_wrapper_over_extract_stage() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("vlc", "3.0.23");
    let stage = tmp.path().join("extract");
    let tmp_caskroom = tmp.path().join("tmp-caskroom");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&tmp_caskroom)?;
    std::fs::write(stage.join("vlc.wrapper.sh"), "#!/bin/sh\necho stage\n")?;
    let preferred = tmp_caskroom.join("vlc.wrapper.sh");
    std::fs::write(&preferred, "#!/bin/sh\necho caskroom\n")?;

    let binary = BinaryArtifact {
        source: "$HOMEBREW_PREFIX/Caskroom/vlc/3.0.23/vlc.wrapper.sh".to_string(),
        target: Some("vlc".to_string()),
    };

    assert_eq!(
        find_binary_source(&stage, &tmp_caskroom, &cask, &binary)?,
        preferred
    );
    Ok(())
}

#[test]
fn parses_app_artifact_targets() {
    let value: Value =
        serde_json::json!({"app": ["Firefox.app", {"target": "Firefox Nightly.app"}]});
    assert_eq!(
        parse_app_artifact(&value),
        Some(AppArtifact {
            source: "Firefox.app".to_string(),
            target: Some("Firefox Nightly.app".to_string())
        })
    );
}

#[test]
fn parses_binary_artifact_targets() {
    let value: Value = serde_json::json!({"binary": ["op"], "target": "$HOMEBREW_PREFIX/bin/op"});
    assert_eq!(
        parse_binary_artifact(&value),
        Some(BinaryArtifact {
            source: "op".to_string(),
            target: Some("$HOMEBREW_PREFIX/bin/op".to_string())
        })
    );
}

#[test]
fn parses_binary_artifacts_and_generated_completions() -> Result<()> {
    let mut cask = test_cask("1password-cli", "2.34.1");
    cask.artifacts = vec![
        serde_json::json!({"binary": ["op"], "target": "$HOMEBREW_PREFIX/bin/op"}),
        serde_json::json!({
            "generate_completions_from_executable": [
                "op",
                "completion",
                {"shells": ["bash", "zsh", "fish"]}
            ]
        }),
        serde_json::json!({"zap": [{"trash": "~/.config/op"}]}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            binaries: vec![BinaryArtifact {
                source: "op".to_string(),
                target: Some("$HOMEBREW_PREFIX/bin/op".to_string())
            }],
            generated_completions: vec![GeneratedCompletionArtifact {
                executable: "op".to_string(),
                args: vec!["completion".to_string()],
                base_name: None,
                shell_parameter_format: None,
                shells: vec![
                    CompletionShell::Bash,
                    CompletionShell::Zsh,
                    CompletionShell::Fish,
                ],
            }],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn rejects_generated_completions_with_no_shells() {
    let value = serde_json::json!({
        "generate_completions_from_executable": ["op", {"shells": []}]
    });

    let err = parse_generated_completion_artifact(&value)
        .unwrap_err()
        .to_string();

    assert!(err.contains("requires at least one shell"));
}

#[test]
fn rejects_generated_completions_with_unknown_options() {
    let value = serde_json::json!({
        "generate_completions_from_executable": ["op", {"shell": "bash"}]
    });

    let err = parse_generated_completion_artifact(&value)
        .unwrap_err()
        .to_string();

    assert!(err.contains("unsupported generate_completions_from_executable field shell"));
}

#[test]
fn parses_declared_completion_artifacts() -> Result<()> {
    let mut cask = test_cask("ghostty", "1.2.0");
    cask.artifacts = vec![
        serde_json::json!({"app": "Ghostty.app"}),
        serde_json::json!({
            "bash_completion": [
                "$APPDIR/Ghostty.app/Contents/Resources/bash-completion/completions/ghostty.bash"
            ],
            "target": "$HOMEBREW_PREFIX/etc/bash_completion.d/ghostty"
        }),
        serde_json::json!({
            "fish_completion": [
                "$APPDIR/Ghostty.app/Contents/Resources/fish/vendor_completions.d/ghostty.fish"
            ],
            "target": "$HOMEBREW_PREFIX/share/fish/vendor_completions.d/ghostty.fish"
        }),
        serde_json::json!({
            "zsh_completion": [
                "$APPDIR/Ghostty.app/Contents/Resources/zsh/site-functions/_ghostty"
            ],
            "target": "$HOMEBREW_PREFIX/share/zsh/site-functions/_ghostty"
        }),
    ];

    assert_eq!(
            cask_artifacts(&cask)?.completions,
            vec![
                CompletionArtifact {
                    shell: CompletionShell::Bash,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/bash-completion/completions/ghostty.bash"
                        .to_string(),
                    target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/ghostty".to_string()),
                },
                CompletionArtifact {
                    shell: CompletionShell::Fish,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/fish/vendor_completions.d/ghostty.fish"
                        .to_string(),
                    target: Some(
                        "$HOMEBREW_PREFIX/share/fish/vendor_completions.d/ghostty.fish"
                            .to_string()
                    ),
                },
                CompletionArtifact {
                    shell: CompletionShell::Zsh,
                    source: "$APPDIR/Ghostty.app/Contents/Resources/zsh/site-functions/_ghostty"
                        .to_string(),
                    target: Some("$HOMEBREW_PREFIX/share/zsh/site-functions/_ghostty".to_string()),
                },
            ]
        );
    Ok(())
}

#[test]
fn completion_target_paths_match_homebrew_names() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());

    assert_eq!(
        completion_target_path(CompletionShell::Bash, "ghostty.bash")?,
        tmp.path().join("etc/bash_completion.d/ghostty")
    );
    assert_eq!(
        completion_target_path(CompletionShell::Fish, "ghostty")?,
        tmp.path()
            .join("share/fish/vendor_completions.d/ghostty.fish")
    );
    assert_eq!(
        completion_target_path(CompletionShell::Zsh, "ghostty")?,
        tmp.path().join("share/zsh/site-functions/_ghostty")
    );
    assert_eq!(
        generated_completion_target_path(CompletionShell::Pwsh, "ghostty")?,
        tmp.path().join("share/pwsh/completions/_ghostty.ps1")
    );
    Ok(())
}

#[test]
fn stages_and_links_declared_completion() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(stage.join("completions"))?;
    file::create_dir_all(&caskroom)?;
    crate::file::write(stage.join("completions/ghostty.bash"), "complete")?;
    let cask = test_cask("ghostty", "1.0.0");
    let completion = CompletionArtifact {
        shell: CompletionShell::Bash,
        source: "completions/ghostty.bash".to_string(),
        target: None,
    };
    let artifacts = CaskArtifacts {
        completions: vec![completion.clone()],
        ..Default::default()
    };
    let target = completion.target_path()?;

    stage_completion(&stage, &caskroom, &cask, &[], &completion)?;
    link_completion(&cask, &artifacts, &caskroom, &target)?;

    assert_eq!(
        crate::file::read_to_string(caskroom.join("etc/bash_completion.d/ghostty"))?,
        "complete"
    );
    assert_eq!(
        std::fs::read_link(&target)?,
        caskroom.join("etc/bash_completion.d/ghostty")
    );
    assert_eq!(crate::file::read_to_string(target)?, "complete");
    Ok(())
}

#[test]
fn declared_completion_source_maps_caskroom_path_to_temp_caskroom() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("tmp-caskroom");
    let cask = test_cask("foo", "1.0.0");
    file::create_dir_all(&stage)?;
    file::create_dir_all(caskroom.join("etc/bash_completion.d"))?;
    crate::file::write(caskroom.join("etc/bash_completion.d/foo"), "complete")?;
    let completion = CompletionArtifact {
        shell: CompletionShell::Bash,
        source: "$HOMEBREW_PREFIX/Caskroom/foo/1.0.0/etc/bash_completion.d/foo".to_string(),
        target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
    };

    stage_completion(&stage, &caskroom, &cask, &[], &completion)?;

    assert_eq!(
        crate::file::read_to_string(caskroom.join("etc/bash_completion.d/foo"))?,
        "complete"
    );
    Ok(())
}

#[test]
fn declared_completion_source_maps_caskroom_path_to_extract_stage() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("tmp-caskroom");
    let cask = test_cask("foo", "1.0.0");
    file::create_dir_all(stage.join("share/completions"))?;
    file::create_dir_all(&caskroom)?;
    crate::file::write(stage.join("share/completions/foo.bash"), "complete")?;
    let completion = CompletionArtifact {
        shell: CompletionShell::Bash,
        source: "$HOMEBREW_PREFIX/Caskroom/foo/1.0.0/share/completions/foo.bash".to_string(),
        target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
    };

    stage_completion(&stage, &caskroom, &cask, &[], &completion)?;

    assert_eq!(
        crate::file::read_to_string(caskroom.join("etc/bash_completion.d/foo"))?,
        "complete"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn link_completion_adopts_homebrew_app_symlink() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("docker-desktop", "2.0.0");
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    let app = AppArtifact {
        source: "Docker.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Docker.app".to_string()),
    };
    let completion = CompletionArtifact {
        shell: CompletionShell::Bash,
        source: "$APPDIR/Docker.app/Contents/Resources/etc/docker.bash-completion".to_string(),
        target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/docker".to_string()),
    };
    let artifacts = CaskArtifacts {
        apps: vec![app.clone()],
        completions: vec![completion.clone()],
        ..Default::default()
    };
    let relative = Path::new("etc/bash_completion.d/docker");
    let target = tmp.path().join(relative);
    let caskroom_completion = caskroom.join(relative);
    let app_completion =
        app_target_path(app.target_name())?.join("Contents/Resources/etc/docker.bash-completion");
    file::create_dir_all(caskroom_completion.parent().unwrap())?;
    file::create_dir_all(app_completion.parent().unwrap())?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(&caskroom_completion, "new")?;
    crate::file::write(&app_completion, "homebrew")?;
    file::make_symlink(&app_completion, &target)?;

    link_completion(&cask, &artifacts, &caskroom, &target)?;

    assert_eq!(std::fs::read_link(&target)?, caskroom_completion);
    assert_eq!(crate::file::read_to_string(target)?, "new");
    Ok(())
}

#[cfg(unix)]
#[test]
fn link_completion_rejects_other_file_in_declared_app() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("foo", "2.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let completion = CompletionArtifact {
        shell: CompletionShell::Bash,
        source: "$APPDIR/Example.app/Contents/Resources/etc/expected.bash".to_string(),
        target: Some("$HOMEBREW_PREFIX/etc/bash_completion.d/foo".to_string()),
    };
    let artifacts = CaskArtifacts {
        apps: vec![app.clone()],
        completions: vec![completion.clone()],
        ..Default::default()
    };
    let target = completion.target_path()?;
    let app_resources = app_target_path(app.target_name())?.join("Contents/Resources/etc");
    let expected = app_resources.join("expected.bash");
    let other = app_resources.join("other.bash");
    file::create_dir_all(&app_resources)?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(expected, "expected")?;
    crate::file::write(&other, "other")?;
    file::make_symlink(&other, &target)?;

    let err = ensure_completion_target_replaceable(&cask, &artifacts, &target)
        .unwrap_err()
        .to_string();

    assert!(err.contains("is not owned by cask 'foo'"));
    assert_eq!(std::fs::read_link(&target)?, other);
    Ok(())
}

#[cfg(unix)]
#[test]
fn link_completion_rejects_target_owned_by_another_cask() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("foo", "2.0.0");
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    let other_caskroom = caskroom_version_dir("other", "1.0.0");
    let relative = Path::new("etc/bash_completion.d/foo");
    let target = tmp.path().join(relative);
    file::create_dir_all(caskroom.join("etc/bash_completion.d"))?;
    file::create_dir_all(other_caskroom.join("etc/bash_completion.d"))?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(caskroom.join(relative), "new")?;
    crate::file::write(other_caskroom.join(relative), "other")?;
    file::make_symlink(&other_caskroom.join(relative), &target)?;

    let err = link_completion(&cask, &CaskArtifacts::default(), &caskroom, &target)
        .unwrap_err()
        .to_string();

    assert!(err.contains("is not owned by cask 'foo'"));
    assert_eq!(std::fs::read_link(&target)?, other_caskroom.join(relative));
    Ok(())
}

#[cfg(unix)]
#[test]
fn stages_generated_completion_output() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&caskroom)?;
    let executable = stage.join("op");
    crate::file::write(
        &executable,
        "#!/bin/sh\nprintf '%s|%s|%s' \"$1\" \"$2\" \"$SHELL\"\n",
    )?;
    let cask = test_cask("1password-cli", "2.34.1");
    let completion = GeneratedCompletionArtifact {
        executable: "op".to_string(),
        args: vec!["completion".to_string()],
        base_name: None,
        shell_parameter_format: None,
        shells: vec![CompletionShell::Bash],
    };

    stage_generated_completions(&stage, &caskroom, &cask, &[], &completion)?;

    assert_eq!(
        crate::file::read_to_string(caskroom.join("etc/bash_completion.d/op"))?,
        "completion|bash|bash"
    );
    Ok(())
}

#[test]
fn generated_completion_executable_expands_appdir() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&caskroom)?;
    let app_executable = tmp.path().join("Applications/Foo.app/Contents/MacOS/foo");
    file::create_dir_all(app_executable.parent().unwrap())?;
    crate::file::write(&app_executable, "app cli")?;
    let cask = test_cask("foo", "1.0.0");
    let completion = GeneratedCompletionArtifact {
        executable: "$APPDIR/Foo.app/Contents/MacOS/foo".to_string(),
        args: vec![],
        base_name: None,
        shell_parameter_format: None,
        shells: vec![CompletionShell::Bash],
    };
    let apps = [AppArtifact {
        source: "Foo.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Foo.app".to_string()),
    }];

    assert_eq!(
        find_generated_completion_executable(&stage, &caskroom, &cask, &apps, &completion,)?,
        app_executable
    );
    Ok(())
}

#[test]
fn appdir_artifact_source_matches_app_case_insensitively() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let prefix_appdir = tmp.path().join("Applications");
    let relative = "foo.app/Contents/MacOS/foo";
    file::create_dir_all(prefix_appdir.join(relative).parent().unwrap())?;
    crate::file::write(prefix_appdir.join(relative), "prefix")?;
    let apps = [
        AppArtifact {
            source: "Other.app".to_string(),
            target: None,
        },
        AppArtifact {
            source: "foo.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/foo.app".to_string()),
        },
    ];

    assert_eq!(
        appdir_artifact_source("$APPDIR/Foo.app/Contents/MacOS/foo", &apps)?,
        Some(prefix_appdir.join(relative)),
    );
    Ok(())
}

#[test]
fn generated_completion_executable_prefers_staged_prefix_binary() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(tmp.path().join("bin"))?;
    file::create_dir_all(caskroom.join("bin"))?;
    crate::file::write(tmp.path().join("bin/op"), "old")?;
    crate::file::write(caskroom.join("bin/op"), "new")?;
    let cask = test_cask("1password-cli", "2.34.1");
    let completion = GeneratedCompletionArtifact {
        executable: "$HOMEBREW_PREFIX/bin/op".to_string(),
        args: vec![],
        base_name: None,
        shell_parameter_format: None,
        shells: vec![CompletionShell::Bash],
    };

    assert_eq!(
        find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion,)?,
        caskroom.join("bin/op")
    );
    Ok(())
}

#[test]
fn rejects_ambiguous_generated_completion_bare_executable() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(stage.join("a"))?;
    file::create_dir_all(stage.join("b"))?;
    file::create_dir_all(&caskroom)?;
    crate::file::write(stage.join("a/tool"), "a")?;
    crate::file::write(stage.join("b/tool"), "b")?;
    let cask = test_cask("tool", "1.0.0");
    let completion = GeneratedCompletionArtifact {
        executable: "tool".to_string(),
        args: vec![],
        base_name: None,
        shell_parameter_format: None,
        shells: vec![CompletionShell::Bash],
    };

    let err = find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion)
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("completion executable 'tool' is ambiguous")
    );
    Ok(())
}

#[test]
fn rejects_ambiguous_generated_completion_nested_executable() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let stage = tmp.path().join("stage");
    let caskroom = tmp.path().join("caskroom");
    file::create_dir_all(stage.join("a/bin"))?;
    file::create_dir_all(stage.join("b/bin"))?;
    file::create_dir_all(&caskroom)?;
    crate::file::write(stage.join("a/bin/tool"), "a")?;
    crate::file::write(stage.join("b/bin/tool"), "b")?;
    let cask = test_cask("tool", "1.0.0");
    let completion = GeneratedCompletionArtifact {
        executable: "bin/tool".to_string(),
        args: vec![],
        base_name: None,
        shell_parameter_format: None,
        shells: vec![CompletionShell::Bash],
    };

    let err = find_generated_completion_executable(&stage, &caskroom, &cask, &[], &completion)
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("completion executable 'bin/tool' is ambiguous")
    );
    Ok(())
}

#[test]
fn remove_obsolete_completions_removes_only_caskroom_symlinks() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("foo", "2.0.0");
    let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
    let other_caskroom = caskroom_version_dir("other", "1.0.0");
    let relative = Path::new("etc/bash_completion.d/foo");
    let target = tmp.path().join(relative);
    let dangling_target = tmp.path().join("etc/bash_completion.d/dangling-foo");
    let other_target = tmp.path().join("etc/bash_completion.d/other-foo");
    let regular_target = tmp.path().join("etc/bash_completion.d/regular-foo");
    file::create_dir_all(old_caskroom.join("etc/bash_completion.d"))?;
    file::create_dir_all(other_caskroom.join("etc/bash_completion.d"))?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(old_caskroom.join(relative), "old")?;
    crate::file::write(other_caskroom.join(relative), "old")?;
    crate::file::write(&regular_target, "old")?;
    file::make_symlink(&old_caskroom.join(relative), &target)?;
    file::make_symlink(
        &old_caskroom.join("etc/bash_completion.d/dangling"),
        &dangling_target,
    )?;
    file::make_symlink(&other_caskroom.join(relative), &other_target)?;

    remove_obsolete_completions(
        &cask,
        &[
            target.clone(),
            dangling_target.clone(),
            other_target.clone(),
            regular_target.clone(),
        ],
        &[],
    )?;

    assert!(target.symlink_metadata().is_err());
    assert!(dangling_target.symlink_metadata().is_err());
    assert!(other_target.symlink_metadata().is_ok());
    assert!(regular_target.symlink_metadata().is_ok());
    Ok(())
}

#[cfg(unix)]
#[test]
fn remove_obsolete_completions_removes_dangling_symlinks_with_symlinked_prefix() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let real_prefix = tmp.path().join("homebrew-real");
    let prefix = tmp.path().join("homebrew");
    file::create_dir_all(&real_prefix)?;
    file::make_symlink(&real_prefix, &prefix)?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("foo", "2.0.0");
    let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
    let relative = Path::new("etc/bash_completion.d/dangling");
    let target = prefix.join("etc/bash_completion.d/foo");
    file::create_dir_all(old_caskroom.join("etc/bash_completion.d"))?;
    file::create_dir_all(target.parent().unwrap())?;
    file::make_symlink(&old_caskroom.join(relative), &target)?;

    remove_obsolete_completions(&cask, std::slice::from_ref(&target), &[])?;

    assert!(target.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn completion_shell_parameter_formats_match_homebrew() {
    let (args, env) =
        completion_shell_parameter(Some("cobra"), CompletionShell::Zsh, Path::new("tool"));
    assert_eq!(args, vec!["completion".to_string(), "zsh".to_string()]);
    assert_eq!(env, Vec::<(String, String)>::new());

    let (args, env) =
        completion_shell_parameter(Some("click"), CompletionShell::Fish, Path::new("my-tool"));
    assert!(args.is_empty());
    assert_eq!(
        env,
        vec![("_MY_TOOL_COMPLETE".to_string(), "fish_source".to_string())]
    );

    let (args, env) =
        completion_shell_parameter(Some("clap"), CompletionShell::Bash, Path::new("tool"));
    assert!(args.is_empty());
    assert_eq!(env, vec![("COMPLETE".to_string(), "bash".to_string())]);

    let (args, env) = completion_shell_parameter(
        Some("--autocomplete=init:"),
        CompletionShell::Pwsh,
        Path::new("tool"),
    );
    assert_eq!(args, vec!["--autocomplete=init:powershell".to_string()]);
    assert_eq!(env, Vec::<(String, String)>::new());
}

#[test]
fn detects_lifecycle_hooks() {
    let mut cask = test_cask("gimp", "3.2.4");
    cask.artifacts = vec![
        serde_json::json!({"preflight": null}),
        serde_json::json!({"app": ["GIMP.app"]}),
    ];

    assert!(has_lifecycle_hook(&cask, "preflight"));
    assert!(!has_lifecycle_hook(&cask, "postflight"));
}

#[test]
fn maps_generated_caskroom_binary_to_temp_caskroom() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("gimp", "3.2.4");
    let tmp_caskroom = tmp.path().join("tmp-caskroom");
    let generated = tmp_caskroom.join("gimp.wrapper.sh");
    file::create_dir_all(&tmp_caskroom)?;
    std::fs::write(&generated, "#!/bin/sh\n")?;

    let source = "$HOMEBREW_PREFIX/Caskroom/gimp/3.2.4/gimp.wrapper.sh";

    assert_eq!(
        generated_caskroom_artifact(&tmp_caskroom, &cask, source),
        Some(generated)
    );
    Ok(())
}

#[test]
fn rejects_generated_caskroom_binary_parent_dirs() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let _guard = BrewPrefixGuard::set(&prefix);
    let cask = test_cask("gimp", "3.2.4");
    let tmp_caskroom = tmp.path().join("tmp-caskroom");
    let source = "$HOMEBREW_PREFIX/Caskroom/gimp/3.2.4/../escape";

    assert_eq!(
        generated_caskroom_artifact(&tmp_caskroom, &cask, source),
        None
    );
    Ok(())
}

#[test]
fn parses_pkg_artifacts() {
    let value: Value = serde_json::json!({"pkg": ["OpenJDK.pkg"]});
    assert_eq!(
        parse_pkg_artifact(&value).unwrap(),
        Some(PkgArtifact {
            source: "OpenJDK.pkg".to_string()
        })
    );
}

#[test]
fn parses_and_installs_generic_artifact() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let source = stage.join("libcblite-4.1.0/include/cbl");
    file::create_dir_all(&source)?;
    file::write(source.join("CouchbaseLite.h"), "header")?;
    let value = serde_json::json!({
        "artifact": [
            "libcblite-4.1.0/include/cbl",
            {"target": "$HOMEBREW_PREFIX/include/cbl"}
        ],
        "target": "$HOMEBREW_PREFIX/include/cbl"
    });
    let artifact = parse_generic_artifact(&value)?.ok_or_else(|| eyre!("missing artifact"))?;
    assert_eq!(
        artifact,
        GenericArtifact {
            source: "libcblite-4.1.0/include/cbl".to_string(),
            target: "$HOMEBREW_PREFIX/include/cbl".to_string(),
        }
    );

    let mut targets = FlightTargetTransaction::default();
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)?;
    assert_eq!(
        file::read_to_string(tmp.path().join("include/cbl/CouchbaseLite.h"))?,
        "header"
    );
    assert_eq!(
        file::read_to_string(
            temporary_caskroom.join("libcblite-4.1.0/include/cbl/CouchbaseLite.h")
        )?,
        "header"
    );
    assert_eq!(
        std::fs::read_link(temporary_caskroom.join("libcblite-4.1.0/include/cbl"))?,
        tmp.path().join("include/cbl")
    );
    assert_eq!(
        targets.installed_targets(),
        [tmp.path().join("include/cbl")]
    );
    assert_eq!(targets.backups.len(), 1);
    assert!(!targets.backups[0].elevate);
    targets.commit()?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn generic_artifact_rejects_extraction_source_symlink_escape() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let outside = tmp.path().join("outside");
    file::create_dir_all(&stage)?;
    file::create_dir_all(&outside)?;
    file::write(outside.join("secret"), "external")?;
    file::make_symlink(&outside, &stage.join("payload"))?;
    let artifact = GenericArtifact {
        source: "payload".to_string(),
        target: "$HOMEBREW_PREFIX/share/example".to_string(),
    };
    let mut targets = FlightTargetTransaction::default();

    let err = install_generic_artifact(
        &stage,
        &tmp.path().join("Caskroom/example/.mise-tmp"),
        &artifact,
        &mut targets,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("outside the extraction root"));
    assert!(tmp.path().join("share/example").symlink_metadata().is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn generic_artifact_rejects_caskroom_source_symlink_escape() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let source = stage.join("payload/include/example");
    file::create_dir_all(&source)?;
    file::write(source.join("example.h"), "header")?;
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    let outside = tmp.path().join("outside");
    file::create_dir_all(&temporary_caskroom)?;
    file::create_dir_all(&outside)?;
    file::make_symlink(&outside, &temporary_caskroom.join("payload"))?;
    let artifact = GenericArtifact {
        source: "payload/include/example".to_string(),
        target: "$HOMEBREW_PREFIX/include/example".to_string(),
    };
    let mut targets = FlightTargetTransaction::default();

    let err = install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)
        .unwrap_err()
        .to_string();

    assert!(err.contains("outside the caskroom"));
    assert!(
        tmp.path()
            .join("include/example")
            .symlink_metadata()
            .is_err()
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn rejects_generic_artifact_target_through_external_symlink() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let external = tmp.path().join("external");
    file::create_dir_all(&prefix)?;
    file::create_dir_all(&external)?;
    std::os::unix::fs::symlink(&external, prefix.join("lib"))?;
    let _guard = BrewPrefixGuard::set(&prefix);

    let err = generic_artifact_target_path("$HOMEBREW_PREFIX/lib/example")
        .unwrap_err()
        .to_string();
    assert!(err.contains("must stay below"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn generic_copy_revalidates_target_after_symlink_swap() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let external = tmp.path().join("external");
    let library = prefix.join("lib");
    file::create_dir_all(&library)?;
    file::create_dir_all(&external)?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let target = generic_artifact_target_path("$HOMEBREW_PREFIX/lib/example")?;

    std::fs::remove_dir(&library)?;
    std::os::unix::fs::symlink(&external, &library)?;

    let err = validate_generic_copy_target(&target)
        .unwrap_err()
        .to_string();
    assert!(err.contains("refusing generic artifact copy outside Homebrew prefix"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn trusted_operation_parent_stays_bound_after_ancestor_swap() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let library = prefix.join("lib");
    file::create_dir_all(&library)?;
    let external = tmp.path().join("external");
    file::create_dir_all(external.join("lib"))?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let target = library.join("example");
    let parent = open_trusted_operation_parent(&target, true, false)?;
    let source = tmp.path().join("source");
    file::create_dir_all(&source)?;
    file::write(source.join("payload"), "installed")?;

    let saved_prefix = tmp.path().join("saved-homebrew");
    file::rename(&prefix, &saved_prefix)?;
    file::make_symlink(&external, &prefix)?;
    copy_cask_artifact_at(&source, &parent.fd, std::ffi::OsStr::new("example"))?;

    assert_eq!(
        file::read_to_string(saved_prefix.join("lib/example/payload"))?,
        "installed"
    );
    assert!(external.join("lib/example").symlink_metadata().is_err());
    remove_all_at(&parent.fd, std::ffi::OsStr::new("example"))?;
    assert!(saved_prefix.join("lib/example").symlink_metadata().is_err());

    file::remove_file(&prefix)?;
    file::rename(&saved_prefix, &prefix)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn trusted_operation_parent_accepts_symlinked_prefix() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let real_prefix = tmp.path().join("real-homebrew");
    file::create_dir_all(real_prefix.join("lib"))?;
    let configured_prefix = tmp.path().join("homebrew");
    file::make_symlink(&real_prefix, &configured_prefix)?;
    let _guard = BrewPrefixGuard::set(&configured_prefix);
    let target = configured_prefix.join("lib/example");

    let parent = open_trusted_operation_parent(&target, true, false)?;
    assert_eq!(
        parent.stable_path()?,
        std::fs::canonicalize(real_prefix.join("lib"))?
    );
    file::write(parent.path()?.join("example"), "installed")?;

    assert_eq!(
        file::read_to_string(real_prefix.join("lib/example"))?,
        "installed"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn trusted_operation_parent_creates_missing_directories() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    file::create_dir_all(&prefix)?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let target = prefix.join("share/example/include/example.h");

    let parent = open_trusted_operation_parent(&target, true, true)?;

    file::write(parent.path()?.join("example.h"), "header")?;
    assert_eq!(file::read_to_string(&target)?, "header");
    Ok(())
}

#[test]
#[cfg(unix)]
fn sudo_invoking_ids_are_trusted_only_for_effective_root() {
    assert_eq!(sudo_invoking_id_from(0, Some("501")), Some(501));
    assert_eq!(sudo_invoking_id_from(1000, Some("501")), None);
    assert_eq!(sudo_invoking_id_from(0, Some("0")), None);
    assert_eq!(sudo_invoking_id_from(0, Some("invalid")), None);
}

#[test]
fn permission_detection_follows_wrapped_error_sources() {
    let err =
        eyre::Report::from(nix::errno::Errno::EACCES).wrap_err("cannot create operation directory");

    assert!(is_permission_denied(&err));
}

#[test]
#[cfg(unix)]
fn elevated_generic_target_allows_only_group_writable_prefix() {
    let prefix = Path::new("/usr/local");

    assert!(strict_elevated_directory_is_trusted(
        prefix, prefix, 0, 0o775
    ));
    assert!(!strict_elevated_directory_is_trusted(
        Path::new("/usr/local/include"),
        prefix,
        0,
        0o775,
    ));
    assert!(!strict_elevated_directory_is_trusted(
        prefix, prefix, 0, 0o777
    ));
    assert!(!strict_elevated_directory_is_trusted(
        prefix, prefix, 501, 0o755
    ));
}

#[test]
#[cfg(unix)]
fn unprivileged_generic_rollback_restores_backup_when_target_is_absent() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let target = prefix.join("include/example.h");
    file::create_dir_all(target.parent().unwrap())?;
    file::write(&target, "original")?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let mut transaction = FlightTargetTransaction::default();

    transaction.protect_generic(&target)?;
    assert!(target.symlink_metadata().is_err());
    transaction.rollback()?;

    assert_eq!(file::read_to_string(&target)?, "original");
    Ok(())
}

#[test]
#[cfg(unix)]
fn trusted_generic_rename_rejects_swapped_prefix() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let library = prefix.join("lib");
    file::create_dir_all(&library)?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let target = library.join("example");
    let backup = library.join("example.backup");
    let expected_parent = resolved_parent(&target)?;
    file::write(&backup, "original")?;

    let saved_prefix = tmp.path().join("saved-homebrew");
    file::rename(&prefix, &saved_prefix)?;
    let external = tmp.path().join("external");
    file::create_dir_all(external.join("lib"))?;
    file::write(external.join("lib/example"), "external")?;
    file::write(external.join("lib/example.backup"), "attacker")?;
    file::make_symlink(&external, &prefix)?;

    let err = rename_trusted_generic_target(&backup, &target, &expected_parent)
        .unwrap_err()
        .to_string();

    assert!(err.contains("changed generic artifact parent"));
    assert_eq!(
        file::read_to_string(external.join("lib/example"))?,
        "external"
    );
    assert_eq!(
        file::read_to_string(external.join("lib/example.backup"))?,
        "attacker"
    );
    file::remove_file(&prefix)?;
    file::rename(&saved_prefix, &prefix)?;
    Ok(())
}

#[test]
#[cfg(unix)]
fn private_staging_cleanup_rejects_replaced_directory() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let library = prefix.join("lib");
    file::create_dir_all(&library)?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let parent = open_trusted_operation_parent(&library.join("target"), true, false)?;
    let staging_name = std::ffi::OsStr::new(".mise-copy-test");
    let staging_path = library.join(staging_name);
    file::create_dir_all(&staging_path)?;
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o700))?;
    let staging = TrustedOperationParent {
        fd: nix::fcntl::openat(
            &parent.fd,
            staging_name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW,
            nix::sys::stat::Mode::empty(),
        )?,
    };
    let saved = library.join("saved-staging");
    file::rename(&staging_path, &saved)?;
    file::create_dir_all(&staging_path)?;

    let err = remove_private_staging_dir(&parent, &staging, staging_name)
        .unwrap_err()
        .to_string();

    assert!(err.contains("was replaced"));
    assert!(staging_path.is_dir());
    assert!(saved.is_dir());
    Ok(())
}

#[test]
#[cfg(unix)]
fn obsolete_generic_cleanup_skips_mutable_parent_directories() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let obsolete = tmp.path().join("lib/obsolete");
    let modified = tmp.path().join("lib/modified");
    file::create_dir_all(obsolete.parent().unwrap())?;
    std::fs::set_permissions(
        obsolete.parent().unwrap(),
        std::fs::Permissions::from_mode(0o777),
    )?;
    file::write(&obsolete, "owned")?;
    file::write(&modified, "owned")?;
    let records = vec![
        CaskTargetRecord {
            path: obsolete.clone(),
            fingerprint: cask_target_fingerprint(&obsolete)?,
            uninstall: None,
        },
        CaskTargetRecord {
            path: modified.clone(),
            fingerprint: cask_target_fingerprint(&modified)?,
            uninstall: None,
        },
    ];
    file::write(&modified, "user change")?;

    remove_obsolete_generic_artifacts(&records, &[])?;

    assert_eq!(file::read_to_string(obsolete)?, "owned");
    assert_eq!(file::read_to_string(modified)?, "user change");
    Ok(())
}

#[test]
#[cfg(unix)]
fn obsolete_generic_cleanup_allows_owner_group_writable_prefix() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let prefix = tmp.path().join("homebrew");
    let library = prefix.join("lib");
    file::create_dir_all(&library)?;
    std::fs::set_permissions(&prefix, std::fs::Permissions::from_mode(0o775))?;
    std::fs::set_permissions(&library, std::fs::Permissions::from_mode(0o775))?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let obsolete = library.join("obsolete");
    file::write(&obsolete, "owned")?;
    let records = vec![CaskTargetRecord {
        path: obsolete.clone(),
        fingerprint: cask_target_fingerprint(&obsolete)?,
        uninstall: None,
    }];

    remove_obsolete_generic_artifacts(&records, &[])?;

    assert!(obsolete.symlink_metadata().is_err());
    Ok(())
}

#[test]
fn rejects_pkg_installer_choices() {
    let value: Value = serde_json::json!({
        "pkg": [
            "VirtualBox.pkg",
            {"choices": [{"choiceIdentifier": "choiceVBox", "attributeSetting": 1}]}
        ]
    });
    assert!(parse_pkg_artifact(&value).is_err());
}

#[test]
fn parses_uninstall_pkgutil_ids() -> Result<()> {
    let mut cask = test_cask("temurin", "26.0.1,8");
    cask.artifacts = vec![
        serde_json::json!({"uninstall": [{"pkgutil": "net.temurin.26.jdk"}]}),
        serde_json::json!({"pkg": ["OpenJDK26U-jdk.pkg"]}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            pkgs: vec![PkgArtifact {
                source: "OpenJDK26U-jdk.pkg".to_string()
            }],
            pkg_ids: vec!["net.temurin.26.jdk".to_string()],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn detects_pkgutil_query_matches() {
    assert!(pkgutil_output_has_match(
        b"com.pioneer.rekordbox.7.2.14.0323\n"
    ));
    assert!(!pkgutil_output_has_match(b""));
    assert!(!pkgutil_output_has_match(b"\n"));
}

#[test]
fn ignores_zap_pkgutil_ids_for_pkg_receipts() -> Result<()> {
    let mut cask = test_cask("google-japanese-ime", "3.33.6130");
    cask.artifacts = vec![
        serde_json::json!({"uninstall": [{"pkgutil": "com.google.pkg.GoogleJapaneseInput"}]}),
        serde_json::json!({"pkg": ["GoogleJapaneseInput.pkg"]}),
        serde_json::json!({"zap": [{"pkgutil": "com.google.pkg.Keystone"}]}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            pkgs: vec![PkgArtifact {
                source: "GoogleJapaneseInput.pkg".to_string()
            }],
            pkg_ids: vec!["com.google.pkg.GoogleJapaneseInput".to_string()],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn rejects_pkg_artifacts_without_pkgutil_ids() {
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![serde_json::json!({"pkg": ["Example.pkg"]})];

    let err = cask_artifacts(&cask).unwrap_err().to_string();
    assert!(err.contains("pkg artifacts require pkgutil ids"));
}

#[test]
fn rejects_empty_pkgutil_patterns() {
    for pkgutil in [
        serde_json::json!(""),
        serde_json::json!(" \n\t"),
        serde_json::json!(["", " \t"]),
    ] {
        let mut cask = test_cask("example", "1.0.0");
        cask.artifacts = vec![
            serde_json::json!({"uninstall": [{"pkgutil": pkgutil}]}),
            serde_json::json!({"pkg": ["Example.pkg"]}),
        ];

        let err = cask_artifacts(&cask).unwrap_err().to_string();
        assert!(err.contains("pkg artifacts require pkgutil ids"));
    }
}

#[test]
fn rejects_pkg_artifacts_with_only_zap_pkgutil_ids() {
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![
        serde_json::json!({"pkg": ["Example.pkg"]}),
        serde_json::json!({"zap": [{"pkgutil": "com.example.cleanup"}]}),
    ];

    let err = cask_artifacts(&cask).unwrap_err().to_string();
    assert!(err.contains("pkg artifacts require pkgutil ids in uninstall metadata"));
}

#[test]
fn parses_font_artifact() {
    let value: Value = serde_json::json!({"font": "SauceCodeProNerdFont-Regular.ttf"});
    assert_eq!(
        parse_font_artifact(&value),
        Some(FontArtifact {
            source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
            target: None,
        })
    );
}

#[test]
fn parses_font_artifact_with_target() {
    let value: Value = serde_json::json!({"font": ["SauceCodeProNerdFont-Regular.ttf", {"target": "CustomName.ttf"}]});
    assert_eq!(
        parse_font_artifact(&value),
        Some(FontArtifact {
            source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
            target: Some("CustomName.ttf".to_string()),
        })
    );
}

#[test]
fn parses_font_cask_artifacts() -> Result<()> {
    let mut cask = test_cask("font-sauce-code-pro-nerd-font", "3.4.0");
    cask.artifacts = vec![
        serde_json::json!({"font": "SauceCodeProNerdFont-Regular.ttf"}),
        serde_json::json!({"font": "SauceCodeProNerdFont-Bold.ttf"}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            fonts: vec![
                FontArtifact {
                    source: "SauceCodeProNerdFont-Regular.ttf".to_string(),
                    target: None,
                },
                FontArtifact {
                    source: "SauceCodeProNerdFont-Bold.ttf".to_string(),
                    target: None,
                },
            ],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn parses_completion_artifacts_and_skips_manpage_artifacts() -> Result<()> {
    let mut cask = test_cask("ghostty", "1.2.0");
    cask.artifacts = vec![
        serde_json::json!({"app": "Ghostty.app"}),
        serde_json::json!({"manpage": ["ghostty.1"]}),
        serde_json::json!({"bash_completion": ["ghostty"]}),
        serde_json::json!({"fish_completion": ["ghostty"]}),
        serde_json::json!({"zsh_completion": ["ghostty"]}),
    ];

    let artifacts = cask_artifacts(&cask)?;
    assert_eq!(artifacts.apps.len(), 1);
    assert_eq!(artifacts.completions.len(), 3);
    assert_eq!(artifacts.fonts.len(), 0);
    Ok(())
}

#[test]
fn font_only_cask_is_valid() -> Result<()> {
    let mut cask = test_cask("font-test", "1.0.0");
    cask.artifacts = vec![serde_json::json!({"font": "TestFont.ttf"})];

    let artifacts = cask_artifacts(&cask)?;
    assert_eq!(artifacts.fonts.len(), 1);
    Ok(())
}

#[test]
fn font_filename_from_source() -> Result<()> {
    let font = FontArtifact {
        source: "MyFont-Regular.ttf".to_string(),
        target: None,
    };
    assert_eq!(font_filename(&font)?, "MyFont-Regular.ttf");
    Ok(())
}

#[test]
fn font_filename_simple_target() -> Result<()> {
    let font = FontArtifact {
        source: "MyFont.ttf".to_string(),
        target: Some("RenamedFont.ttf".to_string()),
    };
    assert_eq!(font_filename(&font)?, "RenamedFont.ttf");
    Ok(())
}

#[test]
fn font_filename_target_with_home_and_absolute_fonts_path() -> Result<()> {
    // Simulates the JetBrainsMono pattern:
    // target: "/$HOME/Library/Fonts/JetBrainsMonoNerdFontPropo-ThinItalic.ttf"
    let target = "/$HOME/Library/Fonts/JetBrainsMonoNerdFontPropo-ThinItalic.ttf".to_string();
    let font = FontArtifact {
        source: "JetBrainsMonoNerdFontPropo-ThinItalic.ttf".to_string(),
        target: Some(target),
    };
    assert_eq!(
        font_filename(&font)?,
        "JetBrainsMonoNerdFontPropo-ThinItalic.ttf"
    );
    Ok(())
}

#[test]
fn font_filename_target_with_home_expansion() -> Result<()> {
    // $HOME without leading slash: "$HOME/Library/Fonts/Font.ttf"
    let target = "$HOME/Library/Fonts/SomeFont.ttf";
    let font = FontArtifact {
        source: "SomeFont.ttf".to_string(),
        target: Some(target.to_string()),
    };
    assert_eq!(font_filename(&font)?, "SomeFont.ttf");
    Ok(())
}

#[test]
fn font_filename_target_with_tilde_expansion() -> Result<()> {
    // ~/Library/Fonts/Font.ttf should expand to <home>/Library/Fonts/Font.ttf
    let target = "~/Library/Fonts/TildeFont.ttf";
    let font = FontArtifact {
        source: "TildeFont.ttf".to_string(),
        target: Some(target.to_string()),
    };
    assert_eq!(font_filename(&font)?, "TildeFont.ttf");
    Ok(())
}

#[test]
fn font_target_path_from_simple_target() -> Result<()> {
    let font = FontArtifact {
        source: "MyFont.ttf".to_string(),
        target: Some("MyFont.ttf".to_string()),
    };
    let expected = font_dir().join("MyFont.ttf");
    assert_eq!(font_target_path(&font)?, expected);
    Ok(())
}

#[test]
fn font_target_path_from_source_only() -> Result<()> {
    let font = FontArtifact {
        source: "FontAwesome.otf".to_string(),
        target: None,
    };
    let expected = font_dir().join("FontAwesome.otf");
    assert_eq!(font_target_path(&font)?, expected);
    Ok(())
}

#[test]
fn font_target_path_with_home_absolute_target() -> Result<()> {
    // Regression: absolute target with $HOME under ~/Library/Fonts
    // should resolve to the correct path
    let target = "/$HOME/Library/Fonts/JetBrainsMono.ttf".to_string();
    let font = FontArtifact {
        source: "JetBrainsMono.ttf".to_string(),
        target: Some(target),
    };
    let expected = font_dir().join("JetBrainsMono.ttf");
    assert_eq!(font_target_path(&font)?, expected);
    Ok(())
}

#[test]
fn font_target_path_with_tilde_target() -> Result<()> {
    // ~/Library/Fonts/Font.ttf should resolve to correct path
    let target = "~/Library/Fonts/TildeFont.ttf".to_string();
    let font = FontArtifact {
        source: "TildeFont.ttf".to_string(),
        target: Some(target),
    };
    let expected = font_dir().join("TildeFont.ttf");
    assert_eq!(font_target_path(&font)?, expected);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_font_dir_uses_xdg_data_home() {
    assert_eq!(font_dir(), crate::env::XDG_DATA_HOME.join("fonts"));
}

#[cfg(target_os = "linux")]
#[test]
fn linux_font_target_preserves_xdg_subdirectories() -> Result<()> {
    let target = font_dir().join("nerd-fonts").join("NestedFont.ttf");
    let font = FontArtifact {
        source: "NestedFont.ttf".to_string(),
        target: Some(target.to_string_lossy().to_string()),
    };

    assert_eq!(font_filename(&font)?, "nerd-fonts/NestedFont.ttf");
    assert_eq!(font_target_path(&font)?, target);
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn linux_supports_font_only_casks() -> Result<()> {
    let mut cask = test_cask("font-test", "1.0.0");
    cask.artifacts = vec![serde_json::json!({"font": "TestFont.ttf"})];
    let artifacts = cask_artifacts(&cask)?;

    validate_platform_support(&cask, &artifacts)
}

#[cfg(target_os = "linux")]
#[test]
fn linux_rejects_non_font_casks() -> Result<()> {
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![serde_json::json!({"app": "Example.app"})];
    let artifacts = cask_artifacts(&cask)?;

    let err = validate_platform_support(&cask, &artifacts)
        .unwrap_err()
        .to_string();
    assert!(err.contains("only font-only casks"));
    assert!(matches!(
        platform_unavailable_state(&cask, &artifacts),
        Some(PackageState::Unavailable { .. })
    ));
    Ok(())
}

#[test]
fn app_only_casks_ignore_pkgutil_ids() -> Result<()> {
    let mut cask = test_cask("example", "1.0.0");
    cask.artifacts = vec![
        serde_json::json!({"uninstall": [{"pkgutil": "com.example.helper"}]}),
        serde_json::json!({"app": "Example.app"}),
    ];

    assert_eq!(
        cask_artifacts(&cask)?,
        CaskArtifacts {
            apps: vec![AppArtifact {
                source: "Example.app".to_string(),
                target: None,
            }],
            ..Default::default()
        }
    );
    Ok(())
}

#[test]
fn binary_targets_default_to_prefix_bin() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());

    assert_eq!(
        binary_target_path("op", Path::new("/Applications"))?,
        tmp.path().join("bin/op")
    );
    assert_eq!(
        binary_target_path("sbin/op", Path::new("/Applications"))?,
        tmp.path().join("sbin/op")
    );
    assert_eq!(
        binary_target_path("$HOMEBREW_PREFIX/bin/op", Path::new("/Applications"))?,
        tmp.path().join("bin/op")
    );
    Ok(())
}

#[test]
fn binary_targets_must_stay_under_an_allowed_root() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());

    // Targets outside both the prefix and /usr/local are rejected.
    let err = binary_target_path("/opt/elsewhere/bin/op", Path::new("/Applications"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("must be under"));
    let err = binary_target_path("../op", Path::new("/Applications"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("must not contain '..'"));
    Ok(())
}

#[test]
fn binary_targets_allow_absolute_usr_local() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());

    // Casks like docker-desktop hardcode absolute /usr/local targets; these
    // are honored even when the prefix is elsewhere (arm64 /opt/homebrew).
    assert_eq!(
        binary_target_path("/usr/local/bin/docker", Path::new("/Applications"))?,
        PathBuf::from("/usr/local/bin/docker")
    );
    assert_eq!(
        binary_target_path(
            "/usr/local/cli-plugins/docker-compose",
            Path::new("/Applications")
        )?,
        PathBuf::from("/usr/local/cli-plugins/docker-compose")
    );
    Ok(())
}

#[test]
fn appdir_binary_targets_are_contained() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    assert_eq!(
        binary_target_path("$APPDIR/Surge Dashboard.app", Path::new("/Applications"))?,
        PathBuf::from("/Applications/Surge Dashboard.app")
    );
    let prefix_appdir = tmp.path().join("Applications");
    assert_eq!(
        binary_target_path("$APPDIR/Surge Dashboard.app", &prefix_appdir)?,
        prefix_appdir.join("Surge Dashboard.app")
    );
    for target in [
        "$APPDIR/../secret",
        "$APPDIR//absolute",
        "$APPDIR/Surge.app/../../secret",
        "prefix/$APPDIR/secret",
    ] {
        assert!(
            binary_target_path(target, Path::new("/Applications")).is_err(),
            "accepted {target}"
        );
    }
    Ok(())
}

#[test]
fn caskroom_binary_paths_support_contained_appdir_targets() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let caskroom = tmp.path().join("Caskroom/surge/1.0.0");
    let appdir = tmp.path().join("Applications");
    let binary = BinaryArtifact {
        source: "$APPDIR/Surge.app/Contents/Applications/Surge Dashboard.app".to_string(),
        target: Some("$APPDIR/Surge Dashboard.app".to_string()),
    };
    assert_eq!(
        caskroom_binary_path(&caskroom, &appdir, &binary)?,
        caskroom.join("Surge Dashboard.app")
    );
    Ok(())
}

#[test]
fn caskroom_binary_paths_preserve_prefix_relative_target() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let caskroom = tmp.path().join("Caskroom/example/1.0.0");
    let binary = BinaryArtifact {
        source: "op".to_string(),
        target: Some("$HOMEBREW_PREFIX/sbin/op".to_string()),
    };

    assert_eq!(
        caskroom_binary_path(&caskroom, Path::new("/Applications"), &binary)?,
        caskroom.join("sbin/op")
    );
    Ok(())
}

#[test]
fn caskroom_binary_paths_strip_usr_local_root() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let caskroom = tmp.path().join("Caskroom/docker-desktop/1.0.0");
    let binary = BinaryArtifact {
        source: "$APPDIR/Docker.app/Contents/Resources/bin/docker".to_string(),
        target: Some("/usr/local/bin/docker".to_string()),
    };

    assert_eq!(
        caskroom_binary_path(&caskroom, Path::new("/Applications"), &binary)?,
        caskroom.join("bin/docker")
    );
    Ok(())
}

#[test]
fn installed_cask_version_uses_only_recorded_legacy_targets() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("app-only", "1.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    file::create_dir_all(app_target_path(app.target_name())?)?;
    let receipt = CaskReceipt {
        schema_version: 0,
        version: cask.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: vec![app_target_path(app.target_name())?],
        binaries: vec![],
        fonts: vec![],
        completions: vec![],
        flight_directories: vec![],
        generic: vec![],
        pkg_ids: vec![],
        targets: Vec::new(),
        prune_safe: false,
        prune_blocker: None,
    };
    crate::file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;

    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some("1.0.0".to_string())
    );
    Ok(())
}

#[test]
fn installed_cask_version_rejects_unknown_receipt_schema() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("future", "1.0.0");
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    let receipt = CaskReceipt {
        schema_version: 4,
        version: cask.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: Vec::new(),
        binaries: Vec::new(),
        fonts: Vec::new(),
        completions: Vec::new(),
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets: Vec::new(),
        prune_safe: false,
        prune_blocker: None,
    };
    file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);
    Ok(())
}

#[test]
fn cask_prune_removes_only_receipt_owned_direct_artifacts() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    let cask = test_cask("example", "1.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    let target = app_target_path(app.target_name())?;
    file::create_dir_all(&target)?;
    file::write(target.join("version"), "1.0.0")?;
    let version_dir = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(version_dir.join("Example.app"))?;
    file::write(version_dir.join("Example.app/version"), "1.0.0")?;
    write_receipt_with_flight_targets(
        &version_dir,
        &cask,
        &CaskArtifacts {
            apps: vec![app],
            ..Default::default()
        },
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;

    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
    assert_eq!(plan.remove.len(), 1);
    assert!(plan.skipped.is_empty());
    assert_eq!(apply_cask_prune_plan_in(&plan, true, &state_dir)?, 0);
    assert!(target.exists());

    assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 1);
    assert!(!target.exists());
    assert!(!caskroom_token_dir(&cask.token).exists());
    assert!(!cask_journal_pending_in(&state_dir, &cask.token));
    Ok(())
}

#[test]
fn cask_prune_keeps_nonempty_token_directory_and_continues() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    let staged_target = write_test_app_receipt(&test_cask("a-staged", "1.0.0"), "Staged.app")?;
    let clean_target = write_test_app_receipt(&test_cask("b-clean", "1.0.0"), "Clean.app")?;
    let staged_token_dir = caskroom_token_dir("a-staged");
    file::create_dir_all(staged_token_dir.join(".mise-tmp-interrupted"))?;

    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
    assert_eq!(plan.remove.len(), 2);

    assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 2);
    assert!(!staged_target.exists());
    assert!(!clean_target.exists());
    assert!(staged_token_dir.join(".mise-tmp-interrupted").is_dir());
    assert!(!caskroom_token_dir("b-clean").exists());
    assert!(!cask_journal_pending_in(&state_dir, "a-staged"));
    assert!(!cask_journal_pending_in(&state_dir, "b-clean"));
    Ok(())
}

#[test]
fn cask_prune_skips_configured_drifted_and_legacy_casks() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");

    let configured = test_cask("configured", "1.0.0");
    let configured_dir = caskroom_version_dir(&configured.token, &configured.version);
    let configured_target = tmp.path().join("Applications/Configured.app");
    file::create_dir_all(&configured_target)?;
    file::create_dir_all(configured_dir.join("Configured.app"))?;
    write_receipt_with_flight_targets(
        &configured_dir,
        &configured,
        &CaskArtifacts {
            apps: vec![AppArtifact {
                source: "Configured.app".to_string(),
                target: Some("$HOMEBREW_PREFIX/Applications/Configured.app".to_string()),
            }],
            ..Default::default()
        },
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;

    let drifted = test_cask("drifted", "1.0.0");
    let drifted_dir = caskroom_version_dir(&drifted.token, &drifted.version);
    let drifted_target = tmp.path().join("Applications/Drifted.app");
    file::create_dir_all(&drifted_target)?;
    file::create_dir_all(drifted_dir.join("Drifted.app"))?;
    write_receipt_with_flight_targets(
        &drifted_dir,
        &drifted,
        &CaskArtifacts {
            apps: vec![AppArtifact {
                source: "Drifted.app".to_string(),
                target: Some("$HOMEBREW_PREFIX/Applications/Drifted.app".to_string()),
            }],
            ..Default::default()
        },
        &[],
        &BTreeMap::new(),
        &[],
        &[],
    )?;
    file::write(drifted_target.join("changed"), "changed")?;

    let legacy = test_cask("legacy", "1.0.0");
    let legacy_dir = caskroom_version_dir(&legacy.token, &legacy.version);
    file::create_dir_all(&legacy_dir)?;
    file::write(
        legacy_dir.join(".mise-cask.toml"),
        toml::to_string_pretty(&CaskReceipt {
            schema_version: 2,
            version: legacy.version.clone(),
            auto_updates: false,
            metadata_only_apps: Vec::new(),
            apps: Vec::new(),
            binaries: Vec::new(),
            fonts: Vec::new(),
            completions: Vec::new(),
            flight_directories: Vec::new(),
            generic: Vec::new(),
            pkg_ids: Vec::new(),
            targets: Vec::new(),
            prune_safe: false,
            prune_blocker: None,
        })?,
    )?;

    let plan =
        cask_prune_plan_from_tokens(&BTreeSet::from([configured.token.clone()]), &state_dir)?;
    assert!(plan.remove.is_empty());
    assert_eq!(
        plan.skipped
            .iter()
            .map(|skip| skip.token.as_str())
            .collect::<Vec<_>>(),
        vec!["drifted", "legacy"]
    );
    Ok(())
}

#[test]
fn cask_prune_skips_shared_targets_and_pending_transactions() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");

    write_test_app_receipt(&test_cask("shared-a", "1.0.0"), "Shared.app")?;
    write_test_app_receipt(&test_cask("shared-b", "1.0.0"), "Shared.app")?;
    write_test_app_receipt(&test_cask("single", "1.0.0"), "Multi.app")?;
    write_test_app_receipt(&test_cask("multi", "1.0.0"), "Multi.app")?;
    write_test_app_receipt(&test_cask("multi", "2.0.0"), "Multi.app")?;
    write_test_app_receipt(&test_cask("pending", "1.0.0"), "Pending.app")?;
    let journal_dir = state_dir.join("brew-cask/pending");
    file::create_dir_all(&journal_dir)?;
    file::write(journal_dir.join("1.0.0.json"), "{}")?;

    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
    assert!(plan.remove.is_empty());
    assert_eq!(plan.skipped.len(), 5);
    for token in ["shared-a", "shared-b"] {
        assert!(plan.skipped.iter().any(|skip| {
            skip.token == token && skip.reason.contains("also claimed by another cask")
        }));
    }
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "pending"
            && skip
                .reason
                .contains("incomplete cask transaction is pending")
    }));
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "single" && skip.reason.contains("also claimed by another cask")
    }));
    assert!(
        plan.skipped
            .iter()
            .any(|skip| { skip.token == "multi" && skip.reason.contains("expected exactly one") })
    );
    Ok(())
}

#[test]
fn cask_prune_rechecks_shared_targets_before_removal() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    let target = write_test_app_receipt(&test_cask("planned", "1.0.0"), "Shared.app")?;
    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
    assert_eq!(plan.remove.len(), 1);

    write_test_app_receipt(&test_cask("late-claim", "1.0.0"), "Shared.app")?;

    assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 0);
    assert!(target.exists());
    assert!(caskroom_token_dir("planned").exists());
    Ok(())
}

#[test]
fn cask_prune_rechecks_homebrew_ownership_before_removal() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    let target = write_test_app_receipt(&test_cask("claimed", "1.0.0"), "Claimed.app")?;
    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;
    assert_eq!(plan.remove.len(), 1);

    file::create_dir_all(caskroom_token_dir("claimed").join(".metadata"))?;

    assert_eq!(apply_cask_prune_plan_in(&plan, false, &state_dir)?, 0);
    assert!(target.exists());
    assert!(caskroom_token_dir("claimed").exists());
    Ok(())
}

#[test]
fn prune_containment_rejects_parent_components() {
    let root = Path::new("/Applications");
    assert!(path_is_below(Path::new("/Applications/Example.app"), root));
    assert!(!path_is_below(Path::new("/Applications"), root));
    assert!(!path_is_below(
        Path::new("/Applications/../etc/example"),
        root
    ));
}

#[test]
fn cask_prune_fails_closed_when_a_receipt_is_corrupt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    write_test_app_receipt(&test_cask("clean", "1.0.0"), "Clean.app")?;
    let corrupt_dir = caskroom_version_dir("corrupt", "1.0.0");
    file::create_dir_all(&corrupt_dir)?;
    file::write(corrupt_dir.join(".mise-cask.toml"), "not = [valid")?;

    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir)?;

    assert!(plan.remove.is_empty());
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "corrupt" && skip.reason.contains("receipt could not be read")
    }));
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "clean" && skip.reason.contains("could not be indexed completely")
    }));
    Ok(())
}

#[cfg(unix)]
#[test]
fn cask_prune_fails_closed_when_a_token_directory_is_unreadable() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let state_dir = tmp.path().join("state");
    write_test_app_receipt(&test_cask("clean", "1.0.0"), "Clean.app")?;
    let unreadable = caskroom_token_dir("unreadable");
    file::create_dir_all(&unreadable)?;
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))?;

    let plan = cask_prune_plan_from_tokens(&BTreeSet::new(), &state_dir);
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o755))?;
    let plan = plan?;

    assert!(plan.remove.is_empty());
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "unreadable" && skip.reason.contains("directory could not be read")
    }));
    assert!(plan.skipped.iter().any(|skip| {
        skip.token == "clean" && skip.reason.contains("could not be indexed completely")
    }));
    Ok(())
}

#[test]
fn cask_prune_receipt_rejects_pkg_and_lifecycle_casks() -> Result<()> {
    let mut cask = test_cask("example", "1.0.0");
    let direct = CaskArtifacts {
        apps: vec![AppArtifact {
            source: "Example.app".to_string(),
            target: None,
        }],
        ..Default::default()
    };
    assert_eq!(cask_prune_blocker(&cask, &direct), None);

    cask.artifacts = vec![serde_json::json!({"uninstall": [{"quit": "com.example"}]})];
    assert!(cask_prune_blocker(&cask, &direct).is_some());

    cask.artifacts.clear();
    let pkg = CaskArtifacts {
        pkgs: vec![PkgArtifact {
            source: "Example.pkg".to_string(),
        }],
        pkg_ids: vec!["com.example.pkg".to_string()],
        ..Default::default()
    };
    assert!(cask_prune_blocker(&cask, &pkg).is_some());

    let wrapper = CaskArtifacts {
        command_wrappers: vec![CommandWrapperArtifact {
            name: "example".to_string(),
            target: None,
            content: None,
            executable: Some("$APPDIR/Example.app/Contents/MacOS/example".to_string()),
            args: Vec::new(),
            env: BTreeMap::new(),
        }],
        ..Default::default()
    };
    assert_eq!(
        cask_prune_blocker(&cask, &wrapper).as_deref(),
        Some("command wrapper artifacts are not supported for pruning")
    );
    Ok(())
}

#[test]
fn any_version_journal_marks_token_pending() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let journal_dir = tmp.path().join("brew-cask/example");
    file::create_dir_all(&journal_dir)?;
    file::write(journal_dir.join("0.9.0.json"), "{}")?;
    file::write(journal_dir.join("1.0.0.json"), "{}")?;

    assert!(cask_journal_pending_in(tmp.path(), "example"));
    assert!(!cask_journal_pending_in(tmp.path(), "other"));
    remove_cask_journals_in(tmp.path(), "example")?;
    assert!(!cask_journal_pending_in(tmp.path(), "example"));
    Ok(())
}

#[test]
fn installed_cask_version_rejects_binary_state_without_receipt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("binary-only", "1.0.0");
    let binary = BinaryArtifact {
        source: "op".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
    };
    file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);

    let target = binary.target_path(Path::new("/Applications"))?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(&target, "binary")?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);
    Ok(())
}

#[test]
fn installed_cask_version_does_not_invent_wrapper_from_current_api() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("firefox", "153.0.1");
    let app = AppArtifact {
        source: "Firefox.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Firefox.app".to_string()),
    };
    let wrapper = CommandWrapperArtifact {
        name: "firefox".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/firefox".to_string()),
        content: None,
        executable: Some("$APPDIR/Firefox.app/Contents/MacOS/firefox".to_string()),
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    let app_target = app_target_path(app.target_name())?;
    file::create_dir_all(&app_target)?;
    let receipt = CaskReceipt {
        schema_version: 0,
        version: cask.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: vec![app_target],
        binaries: Vec::new(),
        fonts: Vec::new(),
        completions: Vec::new(),
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets: Vec::new(),
        prune_safe: false,
        prune_blocker: None,
    };
    file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;

    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some(cask.version.clone())
    );

    let target = wrapper.target_path()?;
    file::create_dir_all(target.parent().unwrap())?;
    file::write(target, "wrapper")?;
    assert_eq!(mise_installed_cask_version(&cask)?, Some(cask.version));
    Ok(())
}

#[cfg(unix)]
#[test]
fn stages_and_links_binary_artifact() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    crate::file::write(stage.join("op"), "binary")?;
    let caskroom = caskroom_version_dir("binary-only", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("binary-only", "1.0.0");
    let binary = BinaryArtifact {
        source: "op".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
    link_binary(&caskroom, Path::new("/Applications"), &binary)?;

    let target = binary.target_path(Path::new("/Applications"))?;
    assert_eq!(std::fs::read_link(&target)?, caskroom.join("bin/op"));
    assert_eq!(crate::file::read_to_string(&target)?, "binary");
    Ok(())
}

#[cfg(unix)]
#[test]
fn keeps_the_payload_beside_a_stage_sourced_binary() -> Result<()> {
    // codex ships a package layout: the launcher execs a helper beside it and
    // reads a manifest naming its resource directories. The cask declares only
    // `binary "bin/codex"`, so copying that one file out of the stage leaves a
    // launcher with nothing to exec, and it falls back to whatever stale copy
    // another install left on the machine.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(stage.join("bin"))?;
    file::create_dir_all(stage.join("resources"))?;
    crate::file::write(stage.join("bin/tool"), "launcher")?;
    crate::file::write(stage.join("bin/tool-helper"), "helper")?;
    crate::file::write(stage.join("package.json"), "{}")?;
    crate::file::write(stage.join("resources/data"), "data")?;
    let caskroom = caskroom_version_dir("payload-cask", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("payload-cask", "1.0.0");
    let binary = BinaryArtifact {
        source: "bin/tool".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
    };

    assert!(payload_backed_binary(&stage, &binary));
    durabilize_stage_payload(&stage, &caskroom, &[])?;
    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
    link_binary(&caskroom, Path::new("/Applications"), &binary)?;

    // The decisive check: staging is over, so the stage is gone.
    file::remove_all(&stage)?;
    let target = binary.target_path(Path::new("/Applications"))?;
    assert_eq!(std::fs::read_link(&target)?, caskroom.join("bin/tool"));
    assert_eq!(crate::file::read_to_string(&target)?, "launcher");
    assert!(
        crate::file::is_executable(&caskroom.join("bin/tool")),
        "a payload the cask ships without the bit still has to run"
    );
    for (path, contents) in [
        ("bin/tool-helper", "helper"),
        ("package.json", "{}"),
        ("resources/data", "data"),
    ] {
        assert_eq!(
            crate::file::read_to_string(caskroom.join(path))?,
            contents,
            "payload entry '{path}' must survive beside the binary"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_a_stage_sourced_binary_into_its_payload_when_the_target_moves_it() -> Result<()> {
    // A payload that nests its binary deeper than the target path cannot rely
    // on the two coinciding, so the caskroom entry has to link back into the
    // tree rather than become a copy standing outside it.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(stage.join("pkg/bin"))?;
    file::create_dir_all(stage.join("pkg/lib"))?;
    crate::file::write(stage.join("pkg/bin/tool"), "launcher")?;
    crate::file::write(stage.join("pkg/lib/support"), "support")?;
    let caskroom = caskroom_version_dir("nested-payload", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("nested-payload", "1.0.0");
    let binary = BinaryArtifact {
        source: "pkg/bin/tool".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
    };

    durabilize_stage_payload(&stage, &caskroom, &[])?;
    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

    let staged = caskroom.join("bin/tool");
    assert_eq!(
        std::fs::read_link(&staged)?,
        caskroom.join("pkg/bin/tool"),
        "must link into the payload, not copy the launcher out of it"
    );
    assert!(crate::file::is_executable(&caskroom.join("pkg/bin/tool")));
    file::remove_all(&stage)?;
    assert_eq!(crate::file::read_to_string(&staged)?, "launcher");
    assert!(
        std::fs::read_link(&staged)?
            .parent()
            .and_then(Path::parent)
            .is_some_and(|root| root.join("lib").is_dir())
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn retargets_a_payload_binary_link_when_the_caskroom_is_renamed() -> Result<()> {
    // The payload link is absolute and points into the temporary caskroom,
    // which only exists until activation renames it. `symlinks_under` takes a
    // *minimum* depth, so the walk reaches a link nested under `bin/` and
    // rewrites it onto the final caskroom rather than leaving it dangling.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(stage.join("pkg/bin"))?;
    crate::file::write(stage.join("pkg/bin/tool"), "launcher")?;
    let final_caskroom = caskroom_version_dir("renamed-payload", "1.0.0");
    let tmp_caskroom = tmp.path().join("tmp-caskroom");
    file::create_dir_all(&tmp_caskroom)?;
    let cask = test_cask("renamed-payload", "1.0.0");
    let binary = BinaryArtifact {
        source: "pkg/bin/tool".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
    };

    durabilize_stage_payload(&stage, &tmp_caskroom, &[])?;
    stage_binary(&stage, &tmp_caskroom, &cask, &[], &binary)?;
    assert_eq!(
        std::fs::read_link(tmp_caskroom.join("bin/tool"))?,
        tmp_caskroom.join("pkg/bin/tool")
    );

    // Activation: the temporary caskroom becomes the installed one.
    file::create_dir_all(final_caskroom.parent().unwrap())?;
    std::fs::rename(&tmp_caskroom, &final_caskroom)?;
    retarget_transient_symlinks(
        &tmp_caskroom,
        &final_caskroom,
        &final_caskroom,
        &FlightTargetTransaction::default(),
    )?;

    let staged = final_caskroom.join("bin/tool");
    assert_eq!(
        std::fs::read_link(&staged)?,
        final_caskroom.join("pkg/bin/tool"),
        "the link must follow the caskroom it was renamed into"
    );
    file::remove_all(&stage)?;
    assert_eq!(crate::file::read_to_string(&staged)?, "launcher");
    Ok(())
}

#[cfg(unix)]
#[test]
fn stages_same_basename_binaries_without_collision() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(stage.join("bin"))?;
    file::create_dir_all(stage.join("sbin"))?;
    crate::file::write(stage.join("bin/op"), "bin")?;
    crate::file::write(stage.join("sbin/op"), "sbin")?;
    let caskroom = caskroom_version_dir("binary-only", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("binary-only", "1.0.0");
    let bin = BinaryArtifact {
        source: "bin/op".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
    };
    let sbin = BinaryArtifact {
        source: "sbin/op".to_string(),
        target: Some("$HOMEBREW_PREFIX/sbin/op".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &bin)?;
    stage_binary(&stage, &caskroom, &cask, &[], &sbin)?;
    link_binary(&caskroom, Path::new("/Applications"), &bin)?;
    link_binary(&caskroom, Path::new("/Applications"), &sbin)?;

    assert_eq!(
        crate::file::read_to_string(bin.target_path(Path::new("/Applications"))?)?,
        "bin"
    );
    assert_eq!(
        crate::file::read_to_string(sbin.target_path(Path::new("/Applications"))?)?,
        "sbin"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn binary_source_prefers_hook_generated_caskroom_file() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    crate::file::write(stage.join("op"), "stage")?;
    let caskroom = caskroom_version_dir("binary-only", "1.0.0");
    file::create_dir_all(&caskroom)?;
    crate::file::write(caskroom.join("op"), "hook")?;
    let cask = test_cask("binary-only", "1.0.0");
    let binary = BinaryArtifact {
        source: "op".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/op".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

    assert_eq!(
        crate::file::read_to_string(caskroom.join("bin/op"))?,
        "hook"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_rather_than_copies_a_binary_behind_a_flight_symlink() -> Result<()> {
    // gcloud-cli's preflight installs the SDK under the prefix and leaves
    // `staged_path/google-cloud-sdk` as a link to it. The launcher derives
    // CLOUDSDK_ROOT_DIR from the resolved path of `$0`, so copying it into
    // the caskroom — out of the tree holding `lib/` — would stage a broken
    // binary. It has to be linked, like Homebrew does.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let installed = tmp.path().join("share/google-cloud-sdk");
    file::create_dir_all(&stage)?;
    file::create_dir_all(installed.join("bin"))?;
    file::create_dir_all(installed.join("lib"))?;
    crate::file::write(installed.join("bin/gcloud"), "launcher")?;
    std::os::unix::fs::symlink(&installed, stage.join("google-cloud-sdk"))?;
    let caskroom = caskroom_version_dir("gcloud-cli", "531.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("gcloud-cli", "531.0.0");
    let binary = BinaryArtifact {
        source: "google-cloud-sdk/bin/gcloud".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/gcloud".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

    let staged = caskroom.join("bin/gcloud");
    assert_eq!(
        std::fs::read_link(&staged)?,
        file::desymlink_path(&installed.join("bin/gcloud")),
        "must link into the SDK tree, not copy the launcher out of it"
    );
    // Still resolves, and `lib/` is a sibling of the link target.
    assert_eq!(crate::file::read_to_string(&staged)?, "launcher");
    assert!(
        std::fs::read_link(&staged)?
            .parent()
            .and_then(Path::parent)
            .is_some_and(|root| root.join("lib").is_dir())
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn links_a_stage_symlink_at_its_target_so_it_survives_teardown() -> Result<()> {
    // The walk matches symlink entries by name and `is_file` follows them,
    // so a stage-local link to a durable binary comes back as a stage path
    // that resolves outside the stage. Linking the caskroom entry at that
    // literal path would dangle the moment staging tears the stage down.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    let durable = tmp.path().join("opt/vendor/tool");
    file::create_dir_all(&stage)?;
    file::create_dir_all(durable.parent().unwrap())?;
    crate::file::write(&durable, "durable")?;
    std::os::unix::fs::symlink(&durable, stage.join("tool"))?;
    let caskroom = caskroom_version_dir("linked-binary", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("linked-binary", "1.0.0");
    let binary = BinaryArtifact {
        source: "tool".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

    let staged = caskroom.join("bin/tool");
    assert_eq!(
        std::fs::read_link(&staged)?,
        file::desymlink_path(&durable),
        "must link the real location, not the path through the stage"
    );
    // The decisive check: staging is over, so the stage is gone.
    file::remove_all(&stage)?;
    assert_eq!(crate::file::read_to_string(&staged)?, "durable");
    Ok(())
}

#[cfg(unix)]
#[test]
fn stages_generic_artifact_through_a_symlinked_stage() -> Result<()> {
    // The lookup resolves links it traverses, so the source can be
    // contained by the stage without sharing its literal prefix. A lexical
    // strip would fail here even though the containment check passes.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let real_stage = tmp.path().join("real-stage");
    let payload = real_stage.join("libcblite-4.1.0/include/cbl");
    file::create_dir_all(&payload)?;
    file::write(payload.join("CouchbaseLite.h"), "header")?;
    std::os::unix::fs::symlink(
        real_stage.join("libcblite-4.1.0"),
        real_stage.join("current"),
    )?;
    let stage = tmp.path().join("stage");
    std::os::unix::fs::symlink(&real_stage, &stage)?;
    let artifact = GenericArtifact {
        source: "current/include/cbl".to_string(),
        target: "$HOMEBREW_PREFIX/include/cbl".to_string(),
    };

    let mut targets = FlightTargetTransaction::default();
    let temporary_caskroom = tmp.path().join("Caskroom/example/.mise-tmp");
    install_generic_artifact(&stage, &temporary_caskroom, &artifact, &mut targets)?;

    assert_eq!(
        file::read_to_string(tmp.path().join("include/cbl/CouchbaseLite.h"))?,
        "header"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn copies_a_binary_whose_link_stays_inside_a_symlinked_stage() -> Result<()> {
    // Mirror of the case above with the link pointing back into the stage,
    // reached through a stage path that is itself a symlink (a symlinked
    // `~/Library/Caches`). The resolved source then differs lexically from
    // `stage`, and treating that as a durable location would leave a
    // dangling binary once the stage is torn down.
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let real_stage = tmp.path().join("real-stage");
    file::create_dir_all(real_stage.join("payload/bin"))?;
    crate::file::write(real_stage.join("payload/bin/tool"), "tool")?;
    std::os::unix::fs::symlink(real_stage.join("payload"), real_stage.join("link"))?;
    let stage = tmp.path().join("stage");
    std::os::unix::fs::symlink(&real_stage, &stage)?;
    let caskroom = caskroom_version_dir("linked-stage", "1.0.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("linked-stage", "1.0.0");
    let binary = BinaryArtifact {
        source: "link/bin/tool".to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/tool".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;

    let staged = caskroom.join("bin/tool");
    assert!(
        !staged.is_symlink(),
        "stage content must be copied, not linked"
    );
    assert_eq!(crate::file::read_to_string(&staged)?, "tool");
    Ok(())
}

#[cfg(unix)]
#[test]
fn stages_absolute_binary_source_from_pkg_install() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let pkg_binary = tmp
        .path()
        .join("Library/Application Support/org.pqrs/Karabiner-Elements/bin/karabiner_cli");
    if let Some(parent) = pkg_binary.parent() {
        file::create_dir_all(parent)?;
    }
    crate::file::write(&pkg_binary, "pkg binary")?;
    let caskroom = caskroom_version_dir("karabiner-elements", "16.1.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("karabiner-elements", "16.1.0");
    let binary = BinaryArtifact {
        source: pkg_binary.to_string_lossy().to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
    link_binary(&caskroom, Path::new("/Applications"), &binary)?;

    let staged = caskroom.join("bin/karabiner_cli");
    assert_eq!(std::fs::read_link(&staged)?, pkg_binary);
    let target = binary.target_path(Path::new("/Applications"))?;
    assert_eq!(std::fs::read_link(&target)?, staged);
    assert_eq!(crate::file::read_to_string(&target)?, "pkg binary");
    Ok(())
}

#[cfg(unix)]
#[test]
fn reports_missing_target_for_dangling_staged_binary_symlink() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let stage = tmp.path().join("stage");
    file::create_dir_all(&stage)?;
    let pkg_binary = tmp
        .path()
        .join("Library/Application Support/org.pqrs/Karabiner-Elements/bin/karabiner_cli");
    if let Some(parent) = pkg_binary.parent() {
        file::create_dir_all(parent)?;
    }
    crate::file::write(&pkg_binary, "pkg binary")?;
    let caskroom = caskroom_version_dir("karabiner-elements", "16.1.0");
    file::create_dir_all(&caskroom)?;
    let cask = test_cask("karabiner-elements", "16.1.0");
    let binary = BinaryArtifact {
        source: pkg_binary.to_string_lossy().to_string(),
        target: Some("$HOMEBREW_PREFIX/bin/karabiner_cli".to_string()),
    };

    stage_binary(&stage, &caskroom, &cask, &[], &binary)?;
    file::remove_file(&pkg_binary)?;
    let err = link_binary(&caskroom, Path::new("/Applications"), &binary)
        .unwrap_err()
        .to_string();

    assert!(err.contains("was staged but symlink target"));
    assert!(err.contains(&pkg_binary.to_string_lossy().to_string()));
    Ok(())
}

#[test]
fn cask_appdir_uses_prefix_for_prefix_targeted_apps() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };

    assert_eq!(cask_appdir(&[app])?, tmp.path().join("Applications"));
    Ok(())
}

#[test]
fn app_target_path_defaults_to_applications() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.remove(APP_DIR_ENV);
    assert_eq!(
        app_target_path("Firefox.app")?,
        PathBuf::from("/Applications/Firefox.app")
    );
    Ok(())
}

#[test]
fn parse_app_artifact_target_without_slash_is_preserved() {
    // The Homebrew API commonly renders `app` targets as a bare bundle
    // name. Parsing must keep it verbatim and must not consult the override.
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, "/tmp/should-not-be-used");
    let value: Value =
        serde_json::json!({"app": ["Firefox.app", {"target": "Firefox Nightly.app"}]});
    assert_eq!(
        parse_app_artifact(&value),
        Some(AppArtifact {
            source: "Firefox.app".to_string(),
            target: Some("Firefox Nightly.app".to_string()),
        })
    );
}

#[test]
fn parse_app_artifact_preserves_prefix_target() {
    // A `$HOMEBREW_PREFIX`-anchored target must survive parsing so that
    // `cask_appdir`/`app_target_path` can route it into the prefix.
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, "/tmp/should-not-be-used");
    let value: Value = serde_json::json!({
        "app": ["Example.app", {"target": "$HOMEBREW_PREFIX/Applications/Example.app"}]
    });
    assert_eq!(
        parse_app_artifact(&value),
        Some(AppArtifact {
            source: "Example.app".to_string(),
            target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
        })
    );
}

#[test]
fn app_target_path_honours_appdir_override() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    // target_app_dir canonicalizes the override, so compare against the
    // resolved base (macOS tempdirs live under the `/var` symlink).
    let base = tmp.path().canonicalize()?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &base);
    assert_eq!(app_target_path("Firefox.app")?, base.join("Firefox.app"));
    Ok(())
}

#[test]
fn app_target_path_accepts_absolute_target_under_override() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &base);
    let target = base.join("Firefox.app");
    assert_eq!(app_target_path(&target.to_string_lossy())?, target);
    Ok(())
}

#[test]
fn app_target_path_relocates_default_applications_target() -> Result<()> {
    // The Homebrew API frequently hardcodes an absolute
    // `/Applications/Foo.app` target (e.g. the firefox cask). With an
    // override configured this must be relocated into it.
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &base);
    assert_eq!(
        app_target_path("/Applications/Firefox.app")?,
        base.join("Firefox.app")
    );
    Ok(())
}

#[test]
fn app_target_path_relocation_preserves_subdirectories() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &base);
    assert_eq!(
        app_target_path("/Applications/JetBrains/IDEA.app")?,
        base.join("JetBrains/IDEA.app")
    );
    Ok(())
}

#[test]
fn app_target_path_defaults_keep_absolute_applications_target() -> Result<()> {
    // Without an override, an absolute `/Applications` target is accepted
    // as-is (no relocation).
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.remove(APP_DIR_ENV);
    assert_eq!(
        app_target_path("/Applications/Firefox.app")?,
        PathBuf::from("/Applications/Firefox.app")
    );
    Ok(())
}

#[test]
fn app_target_path_rejects_target_outside_override() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &base);
    let err = app_target_path("/Users/someone/Evil.app")
        .unwrap_err()
        .to_string();
    assert!(err.contains(&base.to_string_lossy().to_string()), "{err}");
    assert!(!err.contains("/Applications"), "{err}");
    Ok(())
}

#[test]
fn cask_appdir_uses_override() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let _prefix = BrewPrefixGuard::set(&base.join("prefix"));
    let appdir = base.join("appdir");
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &appdir);
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: None,
    };
    assert_eq!(cask_appdir(&[app])?, appdir);
    Ok(())
}

#[test]
fn command_wrapper_target_path_uses_override() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("appdir");
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &appdir);
    // Only `$APPDIR`-anchored targets resolve into the appdir; a bare name
    // lands under the prefix's bin, so anchor the target to exercise the
    // override path.
    let wrapper = CommandWrapperArtifact {
        name: "gimp".to_string(),
        target: Some("$APPDIR/GIMP.app/Contents/MacOS/gimp".to_string()),
        content: None,
        executable: None,
        args: Vec::new(),
        env: BTreeMap::new(),
    };
    assert_eq!(
        wrapper.target_path()?,
        appdir.join("GIMP.app/Contents/MacOS/gimp"),
    );
    Ok(())
}

#[test]
fn allowed_appdir_roots_has_no_duplicates() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let _prefix = BrewPrefixGuard::set(&base.join("prefix"));

    let mut _guard = EnvVarGuard::new();
    _guard.remove(APP_DIR_ENV);
    let roots = allowed_appdir_roots()?;
    let unique: BTreeSet<_> = roots.iter().collect();
    assert_eq!(unique.len(), roots.len(), "{roots:?}");
    assert!(roots.contains(&PathBuf::from("/Applications")));

    let appdir = base.join("appdir");
    _guard.set(APP_DIR_ENV, &appdir);
    let roots = allowed_appdir_roots()?;
    let unique: BTreeSet<_> = roots.iter().collect();
    assert_eq!(unique.len(), roots.len(), "{roots:?}");
    assert!(roots.contains(&appdir));
    Ok(())
}

#[test]
fn binary_target_path_accepts_override_appdir() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("appdir");
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &appdir);
    assert_eq!(
        binary_target_path("$APPDIR/Foo.app/Contents/MacOS/foo", &appdir)?,
        appdir.join("Foo.app/Contents/MacOS/foo"),
    );
    Ok(())
}

#[test]
fn ensure_trusted_appdir_refuses_world_writable_ancestor() -> Result<()> {
    // Regression guard for the CI failure: a world-writable ancestor (as
    // `/tmp` is, mode 1777) must be refused, because any local user could
    // substitute components beneath it. Real application directories are
    // never world-writable.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let shared = base.join("shared");
    file::create_dir_all(&shared)?;
    let mode = std::fs::Permissions::from_mode(0o1777);
    std::fs::set_permissions(&shared, mode)?;
    let err = match ensure_trusted_appdir(&shared.join("Applications")) {
        Ok(_) => panic!("expected world-writable ancestor to be refused"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("untrusted directory"), "{err}");
    Ok(())
}

#[test]
fn ensure_trusted_appdir_creates_missing_tail() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    ensure_trusted_appdir(&appdir)?;
    assert!(appdir.symlink_metadata()?.file_type().is_dir());
    // Idempotent when the directory already exists.
    ensure_trusted_appdir(&appdir)?;
    Ok(())
}

#[test]
fn ensure_trusted_appdir_rejects_symlinked_tail() -> Result<()> {
    // Simulate a symlink planted on the not-yet-existing appdir tail
    // between validation and mutation: it must be rejected, not followed.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let elsewhere = base.join("elsewhere");
    file::create_dir_all(&elsewhere)?;
    let appdir = base.join("Applications");
    std::os::unix::fs::symlink(&elsewhere, &appdir)?;
    let err = match ensure_trusted_appdir(&appdir) {
        Ok(_) => panic!("expected symlinked appdir tail to be rejected"),
        Err(err) => err.to_string(),
    };
    // Must fail because the tail is a symlink, not because an ancestor was
    // untrusted (which is a different guard).
    assert!(err.contains("cannot open operation directory"), "{err}");
    assert!(!err.contains("untrusted directory"), "{err}");
    Ok(())
}

#[test]
fn ensure_trusted_appdir_stays_bound_after_same_uid_replacement() -> Result<()> {
    // The reviewer's scenario: after validation, a same-uid process swaps
    // the accepted appdir for a different directory (or symlink). Because
    // the descriptor is retained and mutations are addressed through it,
    // writes still land in the originally validated directory.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;

    // Swap the validated directory aside and put an attacker-controlled
    // path in its place.
    let stashed = base.join("stashed");
    std::fs::rename(&appdir, &stashed)?;
    let attacker = base.join("attacker");
    file::create_dir_all(&attacker)?;
    std::os::unix::fs::symlink(&attacker, &appdir)?;

    // Writing through the bound descriptor path must reach the original
    // directory (now at `stashed`), never the attacker's directory.
    let bound = parent.path()?;
    crate::file::write(bound.join("canary"), "bound")?;
    assert!(stashed.join("canary").is_file());
    assert!(!attacker.join("canary").exists());
    Ok(())
}

// `ditto` only exists on macOS, and app artifacts are macOS-only in
// practice (Linux cask support is font-only).
#[cfg(target_os = "macos")]
#[test]
fn ditto_into_stays_bound_after_directory_replacement() -> Result<()> {
    // Bind the appdir, then have a same-uid replacement swap the directory
    // pathname for an attacker-controlled one. The fd-bound copy must still
    // land in the originally validated directory.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;

    let source = base.join("payload");
    file::create_dir_all(&source)?;
    crate::file::write(source.join("marker"), "payload")?;

    let stashed = base.join("stashed");
    std::fs::rename(&appdir, &stashed)?;
    let attacker = base.join("attacker");
    file::create_dir_all(&attacker)?;
    std::os::unix::fs::symlink(&attacker, &appdir)?;

    ditto_into(&source, &parent.fd, std::ffi::OsStr::new("Copied.app"))?;
    assert!(stashed.join("Copied.app/marker").is_file());
    assert!(!attacker.join("Copied.app").exists());
    Ok(())
}

#[test]
fn ensure_trusted_appdir_walks_from_unreplaceable_root() -> Result<()> {
    // The appdir is never re-opened via a scanned ancestor pathname: the
    // walk starts at `/` and descends only through verified descriptors.
    // Swapping an intermediate component for another same-uid-owned real
    // directory before the call therefore cannot be reached through a
    // previously-resolved root, and a symlink swap is rejected outright.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let middle = base.join("middle");
    file::create_dir_all(&middle)?;
    let appdir = middle.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;
    assert!(appdir.symlink_metadata()?.file_type().is_dir());

    // Replace the intermediate component with a same-uid symlink: the next
    // walk must refuse it rather than following it.
    let attacker = base.join("attacker");
    file::create_dir_all(&attacker)?;
    std::fs::remove_dir_all(&middle)?;
    std::os::unix::fs::symlink(&attacker, &middle)?;
    assert!(ensure_trusted_appdir(&appdir).is_err());
    // Nothing was created inside the attacker's directory.
    assert!(!attacker.join("Applications").exists());
    drop(parent);
    Ok(())
}

#[test]
fn ditto_into_rejects_preplanted_symlink_destination() -> Result<()> {
    // A same-uid process creates the predictable temporary name as a
    // symlink before the copy. The copy must fail closed rather than follow
    // it, so nothing is written outside the verified directory.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;

    let source = base.join("payload");
    file::create_dir_all(&source)?;
    crate::file::write(source.join("marker"), "payload")?;

    let attacker = base.join("attacker");
    file::create_dir_all(&attacker)?;
    let tmp_name = std::ffi::OsStr::new("Foo.mise-tmp-abc");
    std::os::unix::fs::symlink(&attacker, appdir.join(tmp_name))?;

    // Fails at `mkdirat` (EEXIST) before `ditto` is ever spawned, so this
    // holds on platforms without `ditto` too.
    let err = match ditto_into(&source, &parent.fd, tmp_name) {
        Ok(()) => panic!("expected pre-planted symlink destination to be refused"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("cannot create staging directory"), "{err}");
    assert!(!attacker.join("marker").exists());
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn repair_app_permissions_does_not_traverse_bundle_symlinks() -> Result<()> {
    // A cask bundle may contain a symlink pointing outside the application
    // directory. The recursive flag/permission repair must not follow it and
    // change the referent.
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;

    let outside = base.join("outside.txt");
    crate::file::write(&outside, "keep")?;
    let status = std::process::Command::new("/bin/chmod")
        .args(["644"])
        .arg(&outside)
        .status()?;
    assert!(status.success());

    let bundle = appdir.join("Victim.app");
    file::create_dir_all(&bundle)?;
    std::os::unix::fs::symlink(&outside, bundle.join("link"))?;

    repair_app_permissions_at(&parent, std::ffi::OsStr::new("Victim.app"));

    // The referent keeps its mode and gains no flags.
    let mode = std::process::Command::new("/usr/bin/stat")
        .args(["-f", "%Sp"])
        .arg(&outside)
        .output()?;
    let mode = String::from_utf8_lossy(&mode.stdout).trim().to_string();
    assert_eq!(mode, "-rw-r--r--", "referent mode changed: {mode}");
    let flags = std::process::Command::new("/usr/bin/stat")
        .args(["-f", "%Sf"])
        .arg(&outside)
        .output()?;
    let flags = String::from_utf8_lossy(&flags.stdout).trim().to_string();
    assert!(
        flags.is_empty() || flags == "-",
        "referent flags set: {flags}"
    );
    assert_eq!(crate::file::read_to_string(&outside)?, "keep");
    Ok(())
}

#[test]
fn empty_appdir_override_falls_back_to_applications() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, "");
    assert_eq!(
        app_target_path("Firefox.app")?,
        PathBuf::from("/Applications/Firefox.app")
    );
    assert!(app_target_path("/etc/passwd").is_err());
    Ok(())
}

#[test]
fn relative_appdir_override_is_rejected() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, "relative/apps");
    assert!(app_target_path("Firefox.app").is_err());
    Ok(())
}

#[test]
fn appdir_override_with_parent_dir_is_rejected() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, "/Applications/../etc");
    assert!(app_target_path("Firefox.app").is_err());
    Ok(())
}

#[test]
fn appdir_override_root_alias_is_rejected() -> Result<()> {
    // Alternate spellings of the filesystem root must not become the
    // containment boundary.
    let _lock = ENV_LOCK.lock().unwrap();
    for alias in ["/.", "//", "/./."] {
        let mut _guard = EnvVarGuard::new();
        _guard.set(APP_DIR_ENV, alias);
        assert!(
            app_target_path("Firefox.app").is_err(),
            "expected {alias} to be rejected"
        );
    }
    Ok(())
}

#[test]
fn appdir_override_with_symlink_to_root_is_rejected() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    // An override that resolves to the filesystem root would make `/` the
    // containment boundary for privileged mutations, so it must be
    // rejected — including when reached through a symlink.
    let link = tmp.path().join("link-to-root");
    std::os::unix::fs::symlink("/", &link)?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &link);
    let err = app_target_path("Firefox.app").unwrap_err().to_string();
    assert!(
        err.contains("must not resolve to the filesystem root"),
        "{err}"
    );
    Ok(())
}

#[test]
fn appdir_override_with_benign_symlink_is_resolved() -> Result<()> {
    let _lock = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir()?;
    // A symlink whose target is an ordinary directory (not root) is
    // accepted, but the boundary is the resolved real path so privileged
    // mutations cannot be redirected through the link.
    let real = tmp.path().join("real");
    file::create_dir_all(&real)?;
    let real = real.canonicalize()?;
    let link = tmp.path().join("link");
    std::os::unix::fs::symlink(&real, &link)?;
    let mut _guard = EnvVarGuard::new();
    _guard.set(APP_DIR_ENV, &link);
    assert_eq!(app_target_path("Firefox.app")?, real.join("Firefox.app"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn failed_app_activation_preserves_caskroom_copy() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let base = tmp.path().canonicalize()?;
    let appdir = base.join("Applications");
    let parent = ensure_trusted_appdir(&appdir)?;
    let target = appdir.join("Example.app");
    file::create_dir_all(&target)?;
    file::write(target.join("version"), "old")?;

    let caskroom_app = base.join("Caskroom/example/2.0.0/Example.app");
    file::create_dir_all(&caskroom_app)?;
    file::write(caskroom_app.join("version"), "staged")?;

    let result = activate_app_at(
        &parent,
        std::ffi::OsStr::new("Example.app"),
        std::ffi::OsStr::new("missing.mise-tmp"),
        std::ffi::OsStr::new("Example.mise-old-test"),
        &caskroom_app,
        &target,
    );

    assert!(result.is_err());
    assert!(!caskroom_app.symlink_metadata()?.file_type().is_symlink());
    assert_eq!(
        file::read_to_string(caskroom_app.join("version"))?,
        "staged"
    );
    assert_eq!(file::read_to_string(target.join("version"))?, "old");
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn upgrades_app_with_protected_existing_contents() -> Result<()> {
    let tmp = trusted_tempdir()?;
    // ensure_trusted_appdir walks the appdir with O_NOFOLLOW, so use a
    // canonical base (macOS tempdirs sit under the `/var` symlink).
    let base = tmp.path().canonicalize()?;
    let target = base.join("Docker.app");
    let protected_dir = target.join("Contents/Resources");
    file::create_dir_all(&protected_dir)?;
    crate::file::write(protected_dir.join("docker"), "old")?;
    let status = std::process::Command::new("/bin/chmod")
        .args(["+a", "everyone deny delete_child"])
        .arg(&protected_dir)
        .status()?;
    assert!(status.success());

    let tmp_target = base.join("Docker.mise-tmp-test");
    file::create_dir_all(&tmp_target)?;
    crate::file::write(tmp_target.join("version"), "new")?;

    // swap_app_at addresses entries by name relative to the verified appdir
    // descriptor, so open the containing directory and use bare names.
    let parent = ensure_trusted_appdir(&base)?;
    let old_name = std::ffi::OsString::from("Docker.mise-old-test");
    let old_target = base.join(&old_name);
    let result = swap_app_at(
        &parent,
        std::ffi::OsStr::new("Docker.app"),
        std::ffi::OsStr::new("Docker.mise-tmp-test"),
        &old_name,
    );

    // Remove the ACL so tempfile can clean up even when the repro fails.
    if old_target.exists() {
        let status = std::process::Command::new("/bin/chmod")
            .arg("-RN")
            .arg(&old_target)
            .status()?;
        assert!(status.success());
    }

    result?;
    assert_eq!(crate::file::read_to_string(target.join("version"))?, "new");
    assert!(!old_target.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn remove_obsolete_binary_links_removes_only_caskroom_symlinks() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("binary-only", "2.0.0");
    let old_caskroom = caskroom_version_dir(&cask.token, "1.0.0");
    file::create_dir_all(old_caskroom.join("bin"))?;
    crate::file::write(old_caskroom.join("bin/old"), "old")?;
    let old_target = tmp.path().join("bin/old");
    file::create_dir_all(old_target.parent().unwrap())?;
    file::make_symlink(&old_caskroom.join("bin/old"), &old_target)?;

    let external = tmp.path().join("external/outside");
    file::create_dir_all(external.parent().unwrap())?;
    crate::file::write(&external, "outside")?;
    let external_target = tmp.path().join("bin/outside");
    file::make_symlink(&external, &external_target)?;

    remove_obsolete_binary_links(
        &cask,
        &[old_target.clone(), external_target.clone()],
        &[tmp.path().join("bin/new")],
    )?;

    assert!(old_target.symlink_metadata().is_err());
    assert!(external_target.symlink_metadata().is_ok());
    Ok(())
}

#[test]
fn installed_cask_version_does_not_invent_pkg_ids_from_current_api() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("pkg-only", "1.0.0");
    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    let receipt = CaskReceipt {
        schema_version: 0,
        version: cask.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: vec![],
        binaries: vec![],
        fonts: vec![],
        completions: vec![],
        flight_directories: vec![],
        generic: vec![],
        pkg_ids: vec![],
        targets: Vec::new(),
        prune_safe: false,
        prune_blocker: None,
    };
    crate::file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;

    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some("1.0.0".to_string())
    );
    Ok(())
}

#[test]
fn installed_cask_version_rejects_app_state_without_receipt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("actual-token", "1.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);

    file::create_dir_all(app_target_path(app.target_name())?)?;
    assert_eq!(mise_installed_cask_version(&cask)?, None);
    Ok(())
}

#[test]
fn installed_cask_version_rejects_completion_state_without_receipt() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("completion-only", "1.0.0");
    let completion = CompletionArtifact {
        shell: CompletionShell::Zsh,
        source: "ghostty".to_string(),
        target: None,
    };
    file::create_dir_all(caskroom_version_dir(&cask.token, &cask.version))?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);

    let target = completion.target_path()?;
    file::create_dir_all(target.parent().unwrap())?;
    crate::file::write(target, "complete")?;
    assert_eq!(mise_installed_cask_version(&cask)?, None);
    Ok(())
}

#[test]
fn installed_cask_version_uses_metadata_token() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("metadata-token", "2.0.0");
    let app = AppArtifact {
        source: "Example.app".to_string(),
        target: Some("$HOMEBREW_PREFIX/Applications/Example.app".to_string()),
    };
    file::create_dir_all(caskroom_version_dir("configured-name", &cask.version))?;
    file::create_dir_all(app_target_path(app.target_name())?)?;

    assert_eq!(mise_installed_cask_version(&cask)?, None);

    let caskroom = caskroom_version_dir(&cask.token, &cask.version);
    file::create_dir_all(&caskroom)?;
    let receipt = CaskReceipt {
        schema_version: 0,
        version: cask.version.clone(),
        auto_updates: false,
        metadata_only_apps: Vec::new(),
        apps: vec![app_target_path(
            "$HOMEBREW_PREFIX/Applications/Example.app",
        )?],
        binaries: Vec::new(),
        fonts: Vec::new(),
        completions: Vec::new(),
        flight_directories: Vec::new(),
        generic: Vec::new(),
        pkg_ids: Vec::new(),
        targets: Vec::new(),
        prune_safe: false,
        prune_blocker: None,
    };
    file::write(
        caskroom.join(".mise-cask.toml"),
        toml::to_string_pretty(&receipt)?,
    )?;
    assert_eq!(
        mise_installed_cask_version(&cask)?,
        Some("2.0.0".to_string())
    );
    Ok(())
}

#[test]
fn installed_version_ignores_homebrew_metadata() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("actual-token");
    file::create_dir_all(token_dir.join("2.0.0"))?;
    file::create_dir_all(token_dir.join(".metadata/2.0.0/timestamp/Casks"))?;
    file::create_dir_all(token_dir.join(".mise-tmp-interrupted"))?;
    file::create_dir_all(token_dir.join(".mise-backup-interrupted"))?;

    assert_eq!(installed_version("actual-token"), Some("2.0.0".to_string()));
    Ok(())
}

#[test]
fn installed_versions_preserve_conflict_presence_with_multiple_versions() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("conflicting-cask");
    file::create_dir_all(token_dir.join("1.0.0"))?;
    file::create_dir_all(token_dir.join("2.0.0"))?;

    assert_eq!(installed_version("conflicting-cask"), None);
    assert_eq!(installed_versions("conflicting-cask").len(), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn failed_activation_restores_caskroom_and_external_links() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let cask = test_cask("completion-only", "1.0.0");
    let destination = caskroom_version_dir(&cask.token, &cask.version);
    let staged = caskroom_tmp_dir(&cask);
    let relative = Path::new("etc/bash_completion.d/tool");
    file::create_dir_all(destination.join(relative).parent().unwrap())?;
    file::create_dir_all(staged.join(relative).parent().unwrap())?;
    crate::file::write(destination.join(relative), "previous")?;
    crate::file::write(staged.join(relative), "replacement")?;
    let target = tmp.path().join(relative);
    let new_target = tmp.path().join("bin/new-tool");
    file::create_dir_all(target.parent().unwrap())?;
    file::create_dir_all(new_target.parent().unwrap())?;
    file::make_symlink(&destination.join(relative), &target)?;
    let mut link_transaction =
        ArtifactLinkTransaction::begin(vec![target.clone(), new_target.clone()])?;

    let err = replace_caskroom(&cask, &staged, &destination, || {
        file::make_symlink(&destination.join(relative), &target)?;
        file::make_symlink(&destination.join("bin/new-tool"), &new_target)?;
        Err(eyre!("link failed"))
    })
    .unwrap_err();
    link_transaction.rollback()?;

    assert_eq!(err.to_string(), "link failed");
    assert_eq!(crate::file::read_to_string(&target)?, "previous");
    assert!(new_target.symlink_metadata().is_err());
    assert!(!caskroom_backup_dir(&cask).exists());
    Ok(())
}

#[test]
fn remove_stale_versions_keeps_current_version_and_homebrew_metadata() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;
    let _guard = BrewPrefixGuard::set(tmp.path());
    let token_dir = caskroom_token_dir("actual-token");
    file::create_dir_all(token_dir.join("1.0.0"))?;
    file::create_dir_all(token_dir.join("2.0.0"))?;
    let metadata = token_dir.join(".metadata/2.0.0/timestamp/Casks");
    file::create_dir_all(&metadata)?;
    crate::file::write(metadata.join("actual-token.json"), "metadata")?;

    remove_stale_versions(&token_dir, "2.0.0")?;

    assert!(!token_dir.join("1.0.0").exists());
    assert!(token_dir.join("2.0.0").exists());
    assert_eq!(
        crate::file::read_to_string(metadata.join("actual-token.json"))?,
        "metadata"
    );
    Ok(())
}

#[test]
fn fetch_git_clone_and_stage_clones_and_restructures_only_path() -> Result<()> {
    let _lock = crate::test::lock_ignoring_poison(&ENV_LOCK);
    let tmp = tempfile::tempdir()?;

    // Create a local git repo to clone from
    let repo = tmp.path().join("repo.git");
    std::fs::create_dir_all(repo.join("fonts").join("sample"))?;
    std::fs::write(
        repo.join("fonts").join("sample").join("font.ttf"),
        "initial content",
    )?;
    std::fs::write(
        repo.join("fonts").join("sample").join("font-bold.ttf"),
        "bold",
    )?;

    // Use --initial-branch to avoid depending on the configured default branch name.
    let repo_str = repo.to_string_lossy().to_string();
    let run = |args: &[&str]| -> Result<()> {
        let mut cmd = std::process::Command::new("git");
        if !cmd.args(args).status()?.success() {
            bail!("git {} failed", args.join(" "));
        }
        Ok(())
    };
    run(&["-C", &repo_str, "init", "-q", "--initial-branch=main"])?;
    run(&[
        "-C",
        &repo_str,
        "-c",
        "user.email=test@test",
        "-c",
        "user.name=test",
        "add",
        "-A",
    ])?;
    run(&[
        "-C",
        &repo_str,
        "-c",
        "user.email=test@test",
        "-c",
        "user.name=test",
        "commit",
        "-q",
        "-m",
        "baseline",
    ])?;

    // Create a dedicated branch with different content to verify branch selection.
    run(&["-C", &repo_str, "checkout", "-q", "-b", "fonts-v2"])?;
    std::fs::write(
        repo.join("fonts").join("sample").join("font.ttf"),
        "branch content",
    )?;
    run(&[
        "-C",
        &repo_str,
        "-c",
        "user.email=test@test",
        "-c",
        "user.name=test",
        "commit",
        "-q",
        "-a",
        "-m",
        "updated fonts",
    ])?;

    let url = format!("file://{}", repo.display());

    let cask = Cask {
        token: "font-test".to_string(),
        aliases: vec![],
        old_tokens: vec![],
        version: "latest".to_string(),
        auto_updates: false,
        url,
        url_specs: CaskUrlSpecs {
            branch: Some("fonts-v2".to_string()),
            only_path: Some("fonts/sample".to_string()),
        },
        sha256: Some("no_check".to_string()),
        artifacts: vec![],
        depends_on: CaskDependencies::default(),
        conflicts_with: CaskConflicts::default(),
        ruby_source_path: None,
        ruby_source_checksum: None,
        tap_git_head: None,
        raw_base: None,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let stage = rt.block_on(fetch_git_clone_and_stage(&cask, None))?;

    assert!(stage.join("font.ttf").is_file());
    assert!(stage.join("font-bold.ttf").is_file());
    // Verify the content from the dedicated branch, not the default branch.
    assert_eq!(
        std::fs::read_to_string(stage.join("font.ttf"))?,
        "branch content"
    );
    Ok(())
}

#[test]
fn git_only_path_must_be_a_contained_directory() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let checkout = tmp.path().join("checkout");
    let nested = checkout.join("fonts/sample");
    file::create_dir_all(&nested)?;
    file::write(checkout.join("file"), "not a directory")?;
    let cask = test_cask("font-test", "latest");

    assert_eq!(
        git_only_path_source(&cask, &checkout, Path::new("fonts/sample"))?,
        nested.canonicalize()?
    );
    assert!(git_only_path_source(&cask, &checkout, Path::new("../outside")).is_err());
    assert!(git_only_path_source(&cask, &checkout, Path::new("missing")).is_err());
    assert!(git_only_path_source(&cask, &checkout, Path::new("file")).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn git_only_path_rejects_symlink_escape() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let checkout = tmp.path().join("checkout");
    let outside = tmp.path().join("outside");
    file::create_dir_all(&checkout)?;
    file::create_dir_all(&outside)?;
    file::make_symlink(&outside, &checkout.join("escaped"))?;
    let cask = test_cask("font-test", "latest");

    assert!(git_only_path_source(&cask, &checkout, Path::new("escaped")).is_err());
    Ok(())
}

/// Verifies receipt equality and install mode independently govern skip decisions.
#[test]
fn installed_cask_skip_depends_on_mode_and_receipt_equality() {
    let mut cask = test_cask("example", "release,build");
    for (auto_updates, recorded, mode, expected) in [
        (true, "release", InstallMode::Install, true),
        (true, "release", InstallMode::Upgrade, false),
        (true, "release,build", InstallMode::Install, true),
        (true, "release,build", InstallMode::Upgrade, true),
        (false, "release", InstallMode::Install, false),
        (false, "release", InstallMode::Upgrade, false),
        (false, "release,build", InstallMode::Install, true),
        (false, "release,build", InstallMode::Upgrade, true),
    ] {
        cask.auto_updates = auto_updates;
        assert_eq!(
            should_skip_installed(&cask, recorded, mode),
            expected,
            "auto_updates={auto_updates}, recorded={recorded}, upgrade={}",
            mode == InstallMode::Upgrade,
        );
    }
}

/// Checks Homebrew token precedence, comparability, and symmetric ordering,
/// including trailing tokens reached after the two token indexes diverge.
#[test]
fn auto_updates_compares_homebrew_version_tokens_with_matching_component_counts() {
    use std::cmp::Ordering::{Equal, Greater, Less};

    for (live, current, expected) in [
        ("1", "2", Some(Less)),
        ("1.9", "1.10", Some(Less)),
        ("2.0", "1.99", Some(Greater)),
        ("1.02", "01.2", Some(Equal)),
        ("0.000", "00.0", Some(Equal)),
        (
            "999999999999999999999999999999",
            "1000000000000000000000000000000",
            Some(Less),
        ),
        ("1", "1.0", None),
        ("", "1", None),
        ("1.", "1.0", None),
        ("1..0", "1.0.0", Some(Equal)),
        ("1.0-p1", "1.p1", Some(Equal)),
        ("1.0a", "1a.0-0-1", Some(Less)),
        ("1.0a", "1a.0-0-0", Some(Equal)),
        ("1.2.3alpha4", "1.2.3A4", Some(Equal)),
        ("1.2.3beta2", "1.2.3B2", Some(Equal)),
        ("1.2.3pre9", "1.2.3PRE9", Some(Equal)),
        ("1.2.3rc3", "1.2.3RC3", Some(Equal)),
        ("1.2.3-p34", "1.2.3-P34", Some(Equal)),
        ("1.2.3alpha4", "1.2.3beta2", Some(Less)),
        ("1.2.3beta2", "1.2.3pre3", Some(Less)),
        ("1.2.3pre3", "1.2.3rc2", Some(Less)),
        ("1.2.3rc3", "1.2.3", Some(Less)),
        ("1.2.3", "1.2.3a", Some(Less)),
        ("1.2.3", "1.2.3-p34", Some(Less)),
        ("1.2.3.post34", "1.2.3.post35", Some(Less)),
        ("1.2.3.post34", "1.2.3", None),
        ("HEAD-abcdef", "HEAD-fedcba", Some(Equal)),
        ("HEAD", "2", Some(Greater)),
        ("1.", "1", Some(Equal)),
        ("1.0-beta", "1.0", Some(Less)),
        ("1,2", "1.2", None),
        ("foo", "goo", Some(Less)),
        (" 1", "2", Some(Less)),
        ("+1", "2", Some(Less)),
    ] {
        assert_eq!(
            compare_app_versions(live, current),
            expected,
            "{live:?}, {current:?}"
        );
        assert_eq!(
            compare_app_versions(current, live),
            expected.map(std::cmp::Ordering::reverse)
        );
    }
}

/// Covers upgrade eligibility across short, build, combined, and unavailable
/// bundle versions, including casks with multiple version candidates.
#[test]
fn auto_updates_matches_homebrew_short_build_decisions() {
    for (current, short, build, expected) in [
        ("2.61", Some("2.57"), Some("2057"), true),
        ("2.61", Some("2.62"), Some("2057"), false),
        ("2.61", Some("2.61"), Some("2057"), false),
        ("2057", Some("2.61"), Some("2057"), false),
        ("2.61,3000", Some("2.61"), Some("2057"), false),
        ("2.61,3000,2057", Some("2.61"), Some("2057"), false),
        ("2.61-2057", Some("2.61"), Some("2057"), false),
        ("2.61-2057", Some("2.61"), Some("2058"), false),
        ("2.61-2057", Some("2.61"), Some("2056"), true),
        ("3.6.4-28955b81", Some("3.6.4"), Some("3.6.4"), false),
        ("2.61,2057", Some("2057"), Some("3000"), false),
        ("2.61,2056,2055", Some("2.61"), Some("2057"), false),
        ("2.61,3000", None, Some("2057"), true),
        ("2.61,3000,2057", None, Some("2057"), false),
        ("2.61,3000", None, Some("3001"), false),
        ("1.0", Some("1"), Some("200"), false),
        ("2", None, None, false),
        ("2", Some("0"), Some("0.0"), false),
        ("2", Some("0.0"), Some("1"), true),
        ("2", Some("1"), Some("0"), true),
        ("2", Some(" \t"), Some("1"), true),
        ("2", Some("1"), None, true),
        ("2.5.2,4000", Some("2.5.2(3329)"), Some("3329"), false),
        ("2.5.2,4.4", Some("2.5.2 (3.3)"), Some("3.3"), false),
        ("2.5.2,4000", Some("2.5.2 \t(3329)"), Some("3329"), false),
        ("2.5.3,4000", Some("2.5.2(3329)"), Some("3329"), true),
        ("1.2.3", Some("1.2.3rc3"), None, true),
        ("1.2.3", Some("1.2.3-p34"), None, false),
        ("1a.0-0-1", Some("1.0a"), None, true),
        ("latest", Some("1"), Some("1"), false),
    ] {
        assert_eq!(
            app_version_outdated(current, short, build),
            expected,
            "current={current}, short={short:?}, build={build:?}"
        );
    }
}

/// Verifies both plist encodings preserve optional version strings and that
/// malformed, missing, or directory-backed plist inputs return errors.
#[test]
fn auto_updates_reads_string_versions_from_xml_and_binary_plists() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let app = tmp.path().join("Example.app");
    file::create_dir_all(app.join("Contents"))?;
    let path = app.join("Contents/Info.plist");
    for binary in [false, true] {
        for (short, build) in [
            (Some("1.02"), None),
            (None, Some("2057")),
            (Some("2.5.2(3329)"), Some("3329")),
            (None, None),
        ] {
            let mut dict = plist::Dictionary::new();
            for (key, value) in [
                ("CFBundleShortVersionString", short),
                ("CFBundleVersion", build),
            ] {
                if let Some(value) = value {
                    dict.insert(key.into(), plist::Value::String(value.into()));
                }
            }
            let plist = plist::Value::Dictionary(dict);
            if binary {
                plist.to_file_binary(&path)?;
            } else {
                plist.to_file_xml(&path)?;
            }
            let version = read_app_version(&app)?;
            assert_eq!(version.short.as_deref(), short);
            assert_eq!(version.build.as_deref(), build);
        }
    }
    file::write(&path, "invalid plist")?;
    assert!(read_app_version(&app).is_err());
    std::fs::remove_file(&path)?;
    assert!(read_app_version(&app).is_err());
    file::create_dir_all(&path)?;
    assert!(read_app_version(&app).is_err());
    assert!(read_app_version(&tmp.path().join("Missing.app")).is_err());
    Ok(())
}

/// Checks that symlinked bundle components and FIFO plists are rejected before
/// live version data can be trusted or a FIFO read can block the caller.
#[cfg(unix)]
#[test]
fn auto_updates_refuses_symlinked_and_nonregular_plist_paths() -> Result<()> {
    let tmp = trusted_tempdir()?;
    let original = tmp.path().join("Original.app");
    file::create_dir_all(original.join("Contents"))?;
    let mut dict = plist::Dictionary::new();
    dict.insert(
        "CFBundleShortVersionString".into(),
        plist::Value::String("1".into()),
    );
    plist::Value::Dictionary(dict).to_file_xml(original.join("Contents/Info.plist"))?;
    for component in ["", "Contents", "Contents/Info.plist"] {
        let app = tmp.path().join(format!("Linked-{}.app", component.len()));
        let destination = if component.is_empty() {
            app.clone()
        } else {
            app.join(component)
        };
        file::create_dir_all(destination.parent().unwrap())?;
        std::os::unix::fs::symlink(original.join(component), destination)?;
        assert!(read_app_version(&app).is_err(), "component={component}");
    }
    let fifo_app = tmp.path().join("Fifo.app");
    file::create_dir_all(fifo_app.join("Contents"))?;
    assert!(
        std::process::Command::new("mkfifo")
            .arg(fifo_app.join("Contents/Info.plist"))
            .status()?
            .success()
    );
    assert!(read_app_version(&fifo_app).is_err());
    Ok(())
}
