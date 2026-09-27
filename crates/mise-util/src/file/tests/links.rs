#[cfg(unix)]
use pretty_assertions::assert_eq;

use super::*;

#[cfg(unix)]
#[test]
fn un_dmg_accepts_license_and_extracts_app() -> Result<()> {
    check_dmg_extraction("licensed.dmg")
}

#[cfg(unix)]
#[test]
fn un_dmg_extracts_app_without_license() -> Result<()> {
    check_dmg_extraction("ordinary.dmg")
}

#[cfg(unix)]
fn check_dmg_extraction(archive: &str) -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let hdiutil = tmp.path().join("hdiutil");
    // Emulate the external tool, including a bounded license prompt so a
    // regression fails instead of hanging the test runner.
    write(
        &hdiutil,
        r#"#!/bin/bash
set -eu
case "$1" in
  attach)
if [[ "$6" == *licensed.dmg ]]; then
  [[ "${PAGER:-}" == cat ]] || exit 45
  IFS= read -r -t 2 answer || exit 42
  [[ "$answer" == Y ]] || exit 43
fi
mkdir -p "$5/Example.app/Contents"
printf 'app payload' > "$5/Example.app/Contents/payload"
ln -s payload "$5/Example.app/Contents/link"
;;
  detach)
printf '%s' "$2" > "$0.detached"
;;
  *) exit 44 ;;
esac
"#,
    )?;
    make_executable(&hdiutil)?;
    let mut env = crate::testing::EnvVarGuard::new();
    let mut paths = vec![tmp.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    env.set("PATH", std::env::join_paths(paths)?);
    env.set("PAGER", "less");
    let dest = tmp.path().join("extracted");

    un_dmg(&tmp.path().join(archive), &dest)?;

    assert_eq!(
        read_to_string(dest.join("Example.app/Contents/payload"))?,
        "app payload"
    );
    assert!(dest.join("Example.app/Contents/link").is_symlink());
    let mount = read_to_string(tmp.path().join("hdiutil.detached"))?;
    assert!(
        !Path::new(&mount).exists(),
        "temporary mount directory was not cleaned up"
    );
    Ok(())
}

/// Whether a directory entry is there at all, without resolving it.
///
/// `Path::exists` answers about the *target*, so it is false for a link whose target is gone —
/// which is the entry these tests are about. An assertion written with it would hold before
/// the removal as well as after, and prove nothing.
fn entry_present(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Deliberately not `#[cfg(unix)]`: `make_symlink` writes a real symlink on unix and a
/// junction on Windows, and the two are removed by different system calls. The behaviour under
/// test is what happens to a link mise itself wrote, so it has to be checked on both.
/// Deliberately not `#[cfg(unix)]`, like the removal tests below: `make_symlink` writes a real
/// symlink on unix and a junction on Windows, and only `symlink_metadata` describes both
/// without resolving them.
#[test]
fn entry_exists_sees_what_path_exists_hides() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let live = dir.path().join("live");
    let broken = dir.path().join("broken");
    let plain = dir.path().join("plain.txt");
    create_dir_all(&target).unwrap();
    write(&plain, "x").unwrap();
    make_symlink(&target, &live).unwrap();
    make_symlink(&dir.path().join("nowhere"), &broken).unwrap();

    for p in [&target, &plain, &live, &broken] {
        assert!(entry_exists(p), "{}", p.display());
    }
    assert!(!entry_exists(dir.path().join("never-existed")));

    // The control, and the whole reason this function exists: `Path::exists` answers about the
    // link's target, so it disagrees for exactly the entry that needs finding.
    assert!(!broken.exists());
}

#[test]
fn dir_subdirs_drops_a_link_that_leads_nowhere() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    create_dir_all(&target).unwrap();
    make_symlink(&target, &dir.path().join("live")).unwrap();
    make_symlink(&dir.path().join("nowhere"), &dir.path().join("broken")).unwrap();
    write(dir.path().join("plain.txt"), "x").unwrap();

    let subdirs = dir_subdirs(dir.path()).unwrap();
    assert!(
        subdirs.contains("target") && subdirs.contains("live"),
        "{subdirs:?}"
    );
    assert!(!subdirs.contains("broken"), "{subdirs:?}");
    assert!(!subdirs.contains("plain.txt"), "{subdirs:?}");
}

#[test]
fn remove_all_removes_a_link_whose_target_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    create_dir_all(&target).unwrap();
    write(target.join("keep.txt"), "important").unwrap();
    make_symlink(&target, &link).unwrap();

    remove_all(&target).unwrap();
    // Control: the entry really is a link that no longer resolves. Without this the assertion
    // below could be passing because there was nothing there to begin with.
    assert!(entry_present(&link));
    assert!(!link.exists());

    remove_all(&link).unwrap();
    assert!(!entry_present(&link));
}

#[test]
fn remove_all_removes_a_live_link_without_touching_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    create_dir_all(&target).unwrap();
    write(target.join("keep.txt"), "important").unwrap();
    make_symlink(&target, &link).unwrap();

    remove_all(&link).unwrap();
    assert!(!entry_present(&link));
    // The other half, and the reason the link arm cannot simply recurse: `mise link` points at
    // a directory the user owns, and removing the link must not take its contents with it.
    assert!(target.join("keep.txt").exists());
}

#[test]
fn remove_all_still_removes_ordinary_files_and_directories() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    let subdir = dir.path().join("subdir");
    write(&file, "x").unwrap();
    create_dir_all(subdir.join("nested")).unwrap();
    write(subdir.join("nested/x.txt"), "x").unwrap();

    remove_all(&file).unwrap();
    assert!(!entry_present(&file));
    remove_all(&subdir).unwrap();
    assert!(!entry_present(&subdir));

    // A path that was never there is not an error: most callers remove opportunistically.
    remove_all(dir.path().join("never-existed")).unwrap();
}
