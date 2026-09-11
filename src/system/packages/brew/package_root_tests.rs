use super::*;
use crate::system::packages::brew::package_root;
use std::os::unix::fs::{PermissionsExt, symlink};

fn query_keg(prefix: &Path, name: &str, version: &str) -> Result<PathBuf> {
    let keg = prefix.join("Cellar").join(name).join(version);
    std::fs::create_dir_all(&keg)?;
    std::fs::create_dir_all(prefix.join("opt"))?;
    Ok(keg)
}

fn assert_query_target(name: &str, opt: &Path, keg: &Path) -> Result<()> {
    let root = package_root(name)?;
    assert_eq!(root, opt);
    assert!(root.is_absolute());
    assert_eq!(root.canonicalize()?, keg.canonicalize()?);
    Ok(())
}

#[test]
fn package_root_normalizes_plain_versioned_and_qualified_names() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    for name in ["widget", "openssl@3", "widget@latest", "widget@1.2"] {
        let keg = query_keg(&prefix, name, "opaque-active")?;
        let opt = prefix.join("opt").join(name);
        symlink(&keg, &opt)?;
        for request in [
            name.to_string(),
            format!("homebrew/core/{name}"),
            format!("owner/tap/{name}"),
            format!("another/tap/{name}"),
        ] {
            assert_query_target(&request, &opt, &keg)?;
        }
    }
    Ok(())
}

#[test]
fn package_root_rejects_invalid_identifiers_before_filesystem_lookup() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix.join("missing-prefix"));
    for name in [
        "",
        ".",
        "..",
        "/widget",
        "../widget",
        "owner/widget",
        "a/b/c/d",
        "a//widget",
        "a/b/widget/",
        "a/./widget",
        "../b/widget",
        "a/../widget",
        "a/b/..",
        "a/b/.",
        "widget:name",
        "a:b/c/widget",
        "a/b/widget:name",
        "widget\\name",
        "a\\b/c/widget",
        " widget",
        "wid get",
        "a/ b/widget",
        "widget\n",
        "widget\r",
        "wid\tget",
        "a/b/wid\0get",
        "a/b/wid\u{7f}get",
        "wid\u{a0}get",
    ] {
        let error = format!("{:#}", package_root(name).unwrap_err());
        assert!(error.contains("brew:"), "{name:?}: {error}");
        assert!(!error.contains("missing-prefix"), "{name:?}: {error}");
    }
    Ok(())
}

#[test]
fn package_root_rejects_the_cask_namespace() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    symlink(keg, prefix.join("opt/widget"))?;
    let error = format!("{:#}", package_root("homebrew/cask/widget").unwrap_err());
    assert!(error.contains("cask"), "{error}");
    Ok(())
}

#[test]
fn package_root_accepts_relative_opt_without_receipts_or_public_links() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    let opt = prefix.join("opt/widget");
    symlink("../Cellar/widget/active", &opt)?;
    assert_query_target("widget", &opt, &keg)?;
    assert!(!prefix.join("bin").exists());
    assert!(!prefix.join("var/homebrew/linked/widget").exists());
    assert_eq!(std::fs::read_dir(keg)?.count(), 0);
    Ok(())
}

#[test]
fn package_root_follows_only_the_active_opaque_version_and_preserves_records() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let old = query_keg(&prefix, "widget", "old-channel")?;
    let new = query_keg(&prefix, "widget", "2099.12.31")?;
    let opt = prefix.join("opt/widget");
    std::fs::write(old.join("payload"), "old")?;
    std::fs::write(new.join("payload"), "new")?;
    symlink(&old, &opt)?;
    assert_query_target("widget", &opt, &old)?;
    assert_eq!(std::fs::read_link(&opt)?, old);
    std::fs::remove_file(&opt)?;
    symlink(&new, &opt)?;
    assert_query_target("widget", &opt, &new)?;
    assert_eq!(std::fs::read_link(&opt)?, new);
    assert_eq!(std::fs::read_to_string(old.join("payload"))?, "old");
    assert_eq!(std::fs::read_to_string(new.join("payload"))?, "new");
    Ok(())
}

#[test]
fn package_root_preserves_symlinked_prefix_and_spaces() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, base) = canonical_tempdir()?;
    let prefix = base.join("prefix with spaces");
    let alias = base.join("prefix alias");
    let keg = query_keg(&prefix, "widget", "active")?;
    symlink(&prefix, &alias)?;
    symlink(&keg, prefix.join("opt/widget"))?;
    let _guard = BrewPrefixGuard::set(&alias);
    assert_query_target("widget", &alias.join("opt/widget"), &keg)
}

#[test]
fn package_root_makes_relative_prefix_absolute() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let cwd = std::env::current_dir()?;
    let tmp = tempfile::tempdir_in(&cwd)?;
    let relative = tmp.path().strip_prefix(&cwd)?;
    let _guard = BrewPrefixGuard::set(relative);
    let keg = query_keg(tmp.path(), "widget", "active")?;
    symlink("../Cellar/widget/active", tmp.path().join("opt/widget"))?;
    assert_query_target("widget", &cwd.join(relative).join("opt/widget"), &keg)
}

#[test]
fn package_root_requires_opt_even_with_cellar_or_linked_keg() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, base) = canonical_tempdir()?;
    for state in [
        "missing-prefix",
        "empty-prefix",
        "cellar-only",
        "linked-only",
    ] {
        let prefix = base.join(state);
        let _guard = BrewPrefixGuard::set(&prefix);
        if state != "missing-prefix" {
            std::fs::create_dir_all(&prefix)?;
        }
        if matches!(state, "cellar-only" | "linked-only") {
            let keg = query_keg(&prefix, "widget", "active")?;
            if state == "linked-only" {
                std::fs::create_dir_all(prefix.join("var/homebrew/linked"))?;
                symlink(keg, prefix.join("var/homebrew/linked/widget"))?;
            }
        }
        let error = format!("{:#}", package_root("widget").unwrap_err());
        assert!(error.contains("widget"), "{state}: {error}");
        assert!(
            error.contains(&prefix.display().to_string()),
            "{state}: {error}"
        );
        assert!(error.contains("apply brew:widget"), "{state}: {error}");
        assert!(!prefix.join("opt/widget").exists());
    }
    Ok(())
}

#[test]
fn package_root_rejects_dangling_opt_without_repairing_it() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    std::fs::create_dir_all(prefix.join("opt"))?;
    let opt = prefix.join("opt/widget");
    let target = Path::new("../Cellar/widget/missing");
    symlink(target, &opt)?;
    let error = format!("{:#}", package_root("widget").unwrap_err());
    assert!(error.contains(&opt.display().to_string()), "{error}");
    assert!(error.contains("apply brew:widget"), "{error}");
    assert_eq!(std::fs::read_link(opt)?, target);
    Ok(())
}

#[test]
fn package_root_rejects_regular_directory_and_file_opt_records() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, base) = canonical_tempdir()?;
    for directory in [true, false] {
        let prefix = base.join(directory.to_string());
        let _guard = BrewPrefixGuard::set(&prefix);
        query_keg(&prefix, "widget", "active")?;
        let opt = prefix.join("opt/widget");
        if directory {
            std::fs::create_dir(&opt)?;
        } else {
            std::fs::write(&opt, "untouched")?;
        }
        let error = format!("{:#}", package_root("widget").unwrap_err());
        assert!(error.contains(&opt.display().to_string()), "{error}");
        assert!(!opt.symlink_metadata()?.is_symlink());
        if !directory {
            assert_eq!(std::fs::read_to_string(opt)?, "untouched");
        }
    }
    Ok(())
}

#[test]
fn package_root_rejects_foreign_nested_rack_and_file_targets() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    let foreign = query_keg(&prefix, "foreign", "active")?;
    std::fs::create_dir(keg.join("nested"))?;
    let file = prefix.join("Cellar/widget/file");
    std::fs::write(&file, "untouched")?;
    let opt = prefix.join("opt/widget");
    for target in [
        foreign,
        keg.join("nested"),
        prefix.join("Cellar/widget"),
        file,
    ] {
        symlink(&target, &opt)?;
        let error = format!("{:#}", package_root("widget").unwrap_err());
        assert!(error.contains(&opt.display().to_string()), "{error}");
        assert_eq!(std::fs::read_link(&opt)?, target);
        std::fs::remove_file(&opt)?;
    }
    Ok(())
}

#[test]
fn package_root_accepts_outside_intermediary_resolving_to_its_keg() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    let intermediary = prefix.join("intermediary");
    symlink(&keg, &intermediary)?;
    let opt = prefix.join("opt/widget");
    symlink(intermediary, &opt)?;
    assert_query_target("widget", &opt, &keg)
}

#[test]
fn package_root_uses_filesystem_dotdot_semantics_to_reject_foreign_target() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    let outside = prefix.join("outside");
    std::fs::create_dir_all(outside.join("child"))?;
    std::fs::create_dir(outside.join("active"))?;
    symlink(outside.join("child"), prefix.join("Cellar/widget/jump"))?;
    let opt = prefix.join("opt/widget");
    symlink("../Cellar/widget/jump/../active", &opt)?;
    assert_ne!(opt.canonicalize()?, keg);
    assert!(package_root("widget").is_err());
    Ok(())
}

#[test]
fn package_root_uses_filesystem_dotdot_semantics_to_accept_local_target() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    std::fs::create_dir(prefix.join("outside"))?;
    std::fs::create_dir(prefix.join("Cellar/widget/child"))?;
    symlink(
        prefix.join("Cellar/widget/child"),
        prefix.join("outside/jump"),
    )?;
    let opt = prefix.join("opt/widget");
    symlink("../outside/jump/../active", &opt)?;
    assert_query_target("widget", &opt, &keg)
}

#[test]
fn package_root_preserves_symlink_loop_io_error_and_path() -> Result<()> {
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    query_keg(&prefix, "widget", "active")?;
    let opt = prefix.join("opt/widget");
    symlink("widget", &opt)?;
    let expected = opt.canonicalize().unwrap_err();
    let error = package_root("widget").unwrap_err();
    assert!(format!("{error:#}").contains(&opt.display().to_string()));
    let io = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>());
    assert_eq!(
        io.expect("retain underlying filesystem error")
            .raw_os_error(),
        expected.raw_os_error()
    );
    assert_eq!(std::fs::read_link(opt)?, Path::new("widget"));
    Ok(())
}

#[test]
fn package_root_preserves_permission_denial_and_path() -> Result<()> {
    if nix::unistd::geteuid().is_root() {
        return Ok(());
    }
    let _lock = ENV_LOCK.blocking_lock();
    let (_tmp, prefix) = canonical_tempdir()?;
    let _guard = BrewPrefixGuard::set(&prefix);
    let keg = query_keg(&prefix, "widget", "active")?;
    let opt = prefix.join("opt/widget");
    symlink(&keg, &opt)?;
    let rack = prefix.join("Cellar/widget");
    let permissions = rack.metadata()?.permissions();
    std::fs::set_permissions(&rack, std::fs::Permissions::from_mode(0))?;
    let result = package_root("widget");
    std::fs::set_permissions(&rack, permissions)?;
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains(&opt.display().to_string()));
    let io = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>());
    assert_eq!(
        io.expect("retain underlying filesystem error").kind(),
        std::io::ErrorKind::PermissionDenied
    );
    Ok(())
}
