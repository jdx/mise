use pretty_assertions::assert_eq;

use super::*;

#[cfg(unix)]
#[test]
fn test_executable_mode_adds_read_with_execute() {
    // a restrictive tempfile (0o600) must end up readable, not just executable (0o711)
    assert_eq!(executable_mode(0o600), 0o755);
    assert_eq!(executable_mode(0o640), 0o755);
    // owner-only read/write/exec preserved and widened to be world readable+executable
    assert_eq!(executable_mode(0o700), 0o755);
    // already-correct modes are unchanged
    assert_eq!(executable_mode(0o755), 0o755);
    // group/other write bits are preserved
    assert_eq!(executable_mode(0o660), 0o775);
}

#[test]
fn test_run_blocking_outside_runtime() {
    // no tokio runtime at all — must run the closure inline, not panic
    assert_eq!(run_blocking(|| 42), 42);
}

#[test]
fn hard_link_or_copy_reproduces_the_content() {
    let tmp = tempfile::tempdir().unwrap();
    let from = tmp.path().join("libexample.dll");
    let to = tmp.path().join("copy.dll");
    fs::write(&from, b"payload").unwrap();

    hard_link_or_copy(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"payload");
}

/// The fallback has to be reachable, not just present: linking onto a name that is
/// already taken fails, and callers still expect the destination to be usable.
#[test]
fn hard_link_or_copy_falls_back_when_linking_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let from = tmp.path().join("libexample.dll");
    let to = tmp.path().join("existing.dll");
    fs::write(&from, b"new").unwrap();
    fs::write(&to, b"old").unwrap();

    // `fs::hard_link` refuses an existing destination, so this exercises the copy arm.
    hard_link_or_copy(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"new");
}

#[test]
fn write_atomic_replaces_existing_contents_without_temp_files() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state.toml");
    write_atomic(&path, "old").unwrap();

    write_atomic(&path, "new").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn write_atomic_preserves_existing_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("state.toml");
    write_atomic(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

    write_atomic(&path, "new").unwrap();

    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[cfg(unix)]
#[test]
fn write_atomic_updates_symlink_target_without_replacing_link() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.toml");
    let link = tmp.path().join("state.toml");
    fs::write(&target, "old").unwrap();
    symlink(&target, &link).unwrap();

    write_atomic(&link, "new").unwrap();

    assert!(link.is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn write_atomic_creates_dangling_relative_symlink_target() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.toml");
    let link = tmp.path().join("state.toml");
    symlink("target.toml", &link).unwrap();

    write_atomic(&link, "new").unwrap();

    assert!(link.is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn write_atomic_preserves_dangling_symlink_chain() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.toml");
    let middle = tmp.path().join("middle.toml");
    let link = tmp.path().join("state.toml");
    symlink("target.toml", &middle).unwrap();
    symlink("middle.toml", &link).unwrap();

    write_atomic(&link, "new").unwrap();

    assert!(link.is_symlink());
    assert!(middle.is_symlink());
    assert_eq!(fs::read_to_string(target).unwrap(), "new");
}

#[cfg(unix)]
#[test]
fn write_atomic_accepts_forty_symlinks() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.toml");
    for index in 0..40 {
        let link = tmp.path().join(format!("link-{index}.toml"));
        let next = if index == 39 {
            "target.toml".to_string()
        } else {
            format!("link-{}.toml", index + 1)
        };
        symlink(next, link).unwrap();
    }

    write_atomic(tmp.path().join("link-0.toml"), "new").unwrap();

    assert_eq!(fs::read_to_string(target).unwrap(), "new");
}
