use pretty_assertions::assert_eq;

use super::*;

#[cfg(unix)]
#[test]
fn test_desymlink_path_resolves_relative_target_from_link_parent() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target_dir = root.path().join("target");
    let link_dir = root.path().join("links");
    fs::create_dir_all(&target_dir).unwrap();
    fs::create_dir_all(&link_dir).unwrap();
    let target = target_dir.join("file");
    fs::write(&target, "test").unwrap();
    let link = link_dir.join("file");
    symlink("../target/file", &link).unwrap();

    assert_eq!(desymlink_path(&link), target.canonicalize().unwrap());
}

#[cfg(unix)]
#[test]
fn test_desymlink_path_cached_matches_uncached_and_rechecks_missing_paths() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, "test").unwrap();
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();
    assert_eq!(desymlink_path_cached(&link), desymlink_path(&link));
    assert_eq!(desymlink_path_cached(&link), target.canonicalize().unwrap());

    // Clearing the cache follows a link that was retargeted since.
    let other = root.path().join("other");
    fs::write(&other, "test").unwrap();
    fs::remove_file(&link).unwrap();
    symlink(&other, &link).unwrap();
    assert_eq!(desymlink_path_cached(&link), target.canonicalize().unwrap());
    clear_desymlink_cache();
    assert_eq!(desymlink_path_cached(&link), other.canonicalize().unwrap());

    // A path that does not exist yet is not cached: once it becomes a
    // link, the next call follows it.
    let later = root.path().join("later");
    let before = desymlink_path_cached(&later);
    assert_eq!(before, desymlink_path(&later));
    symlink(&target, &later).unwrap();
    assert_eq!(
        desymlink_path_cached(&later),
        target.canonicalize().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn test_desymlink_path_normalizes_broken_relative_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let link_dir = root.path().join("links");
    let nested_dir = root.path().join("nested");
    fs::create_dir_all(&link_dir).unwrap();
    fs::create_dir_all(&nested_dir).unwrap();
    let link = link_dir.join("missing");
    symlink("../nested/../missing", &link).unwrap();
    let expected = root.path().canonicalize().unwrap().join("missing");

    assert_eq!(desymlink_path(&link), expected);
    assert_eq!(
        desymlink_path(&link),
        desymlink_path(&root.path().join("missing"))
    );
}

#[cfg(unix)]
#[test]
fn test_desymlink_path_resolves_existing_symlink_prefix_before_parent() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_nested = outside.path().join("nested");
    fs::create_dir_all(&outside_nested).unwrap();
    let link = root.path().join("link");
    symlink(&outside_nested, &link).unwrap();

    let through_link = link.join("../.config/mise/tasks/same");
    let expected = outside
        .path()
        .canonicalize()
        .unwrap()
        .join(".config/mise/tasks/same");
    let false_match = root
        .path()
        .canonicalize()
        .unwrap()
        .join(".config/mise/tasks/same");

    assert_eq!(desymlink_path(&through_link), expected);
    assert_ne!(desymlink_path(&through_link), desymlink_path(&false_match));
}

#[test]
fn test_desymlink_path_preserves_parent_after_missing_component() {
    let root = tempfile::tempdir().unwrap();
    let unresolved = root.path().join("missing/../target");
    let canonical_root = root.path().canonicalize().unwrap();
    #[cfg(windows)]
    let expected = {
        let mut expected = canonical_root.as_os_str().to_os_string();
        expected.push("\\missing\\..\\target");
        PathBuf::from(expected)
    };
    #[cfg(not(windows))]
    let expected = canonical_root.join("missing/../target");

    assert_eq!(desymlink_path(&unresolved), expected);
    assert_ne!(
        desymlink_path(&unresolved),
        desymlink_path(&canonical_root.join("target"))
    );
}

#[cfg(unix)]
#[test]
fn test_desymlink_path_preserves_absolute_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, "test").unwrap();
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();

    assert_eq!(desymlink_path(&link), target.canonicalize().unwrap());
}

#[test]
fn test_retry_remove_all_retries_directory_not_empty() {
    let mut attempts = 0;
    retry_remove_all(|| {
        attempts += 1;
        if attempts < 3 {
            Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty))
                .wrap_err("failed rm -rf")
        } else {
            Ok(())
        }
    })
    .unwrap();

    assert_eq!(attempts, 3);
}

#[test]
fn test_retry_remove_all_does_not_retry_other_errors() {
    let mut attempts = 0;
    let err = retry_remove_all(|| {
        attempts += 1;
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into())
    })
    .unwrap_err();

    assert_eq!(attempts, 1);
    assert_eq!(
        err.downcast_ref::<std::io::Error>().map(|err| err.kind()),
        Some(std::io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn test_retry_remove_all_propagates_final_attempt() {
    let mut attempts = 0;
    let err = retry_remove_all(|| {
        attempts += 1;
        Err(std::io::Error::from(std::io::ErrorKind::DirectoryNotEmpty).into())
    })
    .unwrap_err();

    assert_eq!(attempts, 5);
    assert_eq!(
        err.downcast_ref::<std::io::Error>().map(|err| err.kind()),
        Some(std::io::ErrorKind::DirectoryNotEmpty)
    );
}

#[test]
#[cfg(windows)]
fn test_symlink_prefix_detection_uses_filesystem_casing() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("Provider");
    let target = prefix.join("version");
    let link = dir.path().join("link");
    fs::create_dir_all(&target).unwrap();
    junction::create(&target, &link).unwrap();

    assert!(
        is_symlink_target_within(&link, &dir.path().join("PROVIDER")).unwrap(),
        "Windows path containment should honor filesystem casing semantics"
    );

    fs::remove_dir_all(&prefix).unwrap();
    assert!(
        is_symlink_target_within(&link, &dir.path().join("PROVIDER")).unwrap(),
        "dangling Windows targets should retain case-insensitive containment"
    );
}

#[test]
#[cfg(windows)]
fn test_dangling_symlink_prefix_detection_honors_case_sensitive_directories() {
    let dir = tempfile::tempdir().unwrap();
    let case_sensitive_parent = dir.path().join("case-sensitive");
    fs::create_dir(&case_sensitive_parent).unwrap();
    if enable_directory_case_sensitivity(&case_sensitive_parent).is_err() {
        return;
    }
    assert_eq!(
        directory_is_case_sensitive(&case_sensitive_parent),
        Some(true)
    );

    let prefix = case_sensitive_parent.join("Provider");
    let target = prefix.join("version");
    let link = dir.path().join("link");
    fs::create_dir_all(&target).unwrap();
    junction::create(&target, &link).unwrap();
    fs::remove_dir_all(&prefix).unwrap();

    assert!(
        !is_symlink_target_within(&link, &case_sensitive_parent.join("PROVIDER")).unwrap(),
        "case-sensitive directories must not equate differently-cased dangling paths"
    );
}

#[cfg(windows)]
fn enable_directory_case_sensitivity(path: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_WRITE_ATTRIBUTES,
        FileCaseSensitiveInfo, SetFileInformationByHandle,
    };
    use windows_sys::Win32::System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR;

    let directory = fs::OpenOptions::new()
        .access_mode(FILE_WRITE_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let case_info = FILE_CASE_SENSITIVE_INFO {
        Flags: FILE_CS_FLAG_CASE_SENSITIVE_DIR,
    };
    let updated = unsafe {
        SetFileInformationByHandle(
            directory.as_raw_handle(),
            FileCaseSensitiveInfo,
            std::ptr::from_ref(&case_info).cast(),
            std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
        )
    };
    if updated == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[test]
#[cfg(unix)]
fn test_symlink_prefix_detection_uses_the_immediate_target() {
    let dir = tempfile::tempdir().unwrap();
    let cellar = dir.path().join("Cellar");
    let target = cellar.join("node@22").join("22.0.0");
    let opt = dir.path().join("opt");
    let opt_entry = opt.join("node@22");
    let link = dir.path().join("link");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(&opt).unwrap();
    std::os::unix::fs::symlink(&target, &opt_entry).unwrap();
    std::os::unix::fs::symlink(&opt_entry, &link).unwrap();

    assert!(!is_symlink_target_within(&link, &cellar).unwrap());
    assert!(is_symlink_target_within(&link, &opt).unwrap());

    fs::remove_file(&opt_entry).unwrap();
    assert!(is_symlink_target_within(&link, &opt).unwrap());
}

#[test]
fn test_remove_symlink_or_junction_rejects_non_links() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    let directory = dir.path().join("directory");
    fs::write(&file, "contents").unwrap();
    fs::create_dir(&directory).unwrap();

    assert!(remove_symlink_or_junction(&file).is_err());
    assert!(remove_symlink_or_junction(&directory).is_err());
    assert!(file.is_file());
    assert!(directory.is_dir());
}

#[test]
#[cfg(windows)]
fn test_remove_symlink_or_junction_removes_a_directory_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    fs::create_dir(&target).unwrap();
    match std::os::windows::fs::symlink_dir(&target, &link) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return,
        Err(err) => panic!("failed to create directory symlink: {err}"),
    }

    remove_symlink_or_junction(&link).unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(target.is_dir());
}

#[test]
#[cfg(windows)]
fn test_remove_open_link_preserves_path_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    let moved_link = dir.path().join("moved-link");
    fs::create_dir(&target).unwrap();
    junction::create(&target, &link).unwrap();

    let link_handle = open_link_for_removal(&link).unwrap();
    fs::rename(&link, &moved_link).unwrap();
    fs::create_dir(&link).unwrap();
    remove_open_link(link_handle).unwrap();

    assert!(link.is_dir());
    assert!(fs::symlink_metadata(&moved_link).is_err());
    assert!(target.is_dir());
}

#[test]
#[cfg(unix)]
fn test_symlink_prefix_detection_rejects_escapes() {
    let dir = tempfile::tempdir().unwrap();
    let prefix = dir.path().join("source");
    let foreign = dir.path().join("foreign");
    let escaped = dir.path().join("escaped");
    fs::create_dir_all(&prefix).unwrap();
    fs::create_dir_all(&foreign).unwrap();

    std::os::unix::fs::symlink(prefix.join("..").join("foreign"), &escaped).unwrap();
    assert!(!is_symlink_target_within(&escaped, &prefix).unwrap());

    let relative_inside = dir.path().join("relative-inside");
    std::os::unix::fs::symlink("source/version", &relative_inside).unwrap();
    assert!(is_symlink_target_within(&relative_inside, &prefix).unwrap());

    let relative_escape = dir.path().join("relative-escape");
    std::os::unix::fs::symlink("source/../foreign", &relative_escape).unwrap();
    assert!(!is_symlink_target_within(&relative_escape, &prefix).unwrap());
}

#[test]
#[cfg(unix)]
fn test_copy_dir_all_preserve_symlinks_does_not_follow_loops() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let dest = dir.path().join("dest");
    fs::create_dir_all(source.join("Example.app/Contents")).unwrap();
    fs::write(source.join("Example.app/Contents/example"), "example").unwrap();
    std::os::unix::fs::symlink(".", source.join("Applications")).unwrap();

    copy_dir_all_preserve_symlinks(&source, &dest).unwrap();

    assert_eq!(
        fs::read_to_string(dest.join("Example.app/Contents/example")).unwrap(),
        "example"
    );
    assert_eq!(
        fs::read_link(dest.join("Applications")).unwrap(),
        Path::new(".")
    );
}

#[test]
#[cfg(unix)]
fn test_copy_dmg_volume_skips_unreadable_volume_metadata() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let volume = dir.path().join("volume");
    let dest = dir.path().join("dest");
    fs::create_dir_all(volume.join("Example.app/Contents/.Trashes")).unwrap();
    fs::write(volume.join("Example.app/Contents/.Trashes/kept"), "kept").unwrap();
    fs::write(volume.join(".DS_Store"), "metadata").unwrap();
    fs::create_dir_all(volume.join(".fseventsd")).unwrap();
    fs::write(volume.join(".fseventsd/log"), "metadata").unwrap();
    // mysql-workbench-community-8.0.47-macos-arm64.dmg ships `.Trashes`
    // with mode 0333, which a non-root user cannot list.
    fs::create_dir(volume.join(".Trashes")).unwrap();
    fs::set_permissions(volume.join(".Trashes"), fs::Permissions::from_mode(0o333)).unwrap();

    let result = copy_dmg_volume(&volume, &dest);
    fs::set_permissions(volume.join(".Trashes"), fs::Permissions::from_mode(0o755)).unwrap();
    result.unwrap();

    assert_eq!(
        fs::read_to_string(dest.join("Example.app/Contents/.Trashes/kept")).unwrap(),
        "kept"
    );
    assert!(!dest.join(".Trashes").exists());
    assert!(!dest.join(".DS_Store").exists());
    assert!(!dest.join(".fseventsd").exists());
}

#[test]
#[cfg(unix)]
fn test_make_symlink_creates_and_atomically_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let target_a = dir.path().join("a");
    let target_b = dir.path().join("b");
    fs::write(&target_a, "a").unwrap();
    fs::write(&target_b, "b").unwrap();
    let link = dir.path().join("link");

    // Creates a new symlink.
    make_symlink(&target_a, &link).unwrap();
    assert_eq!(fs::read_link(&link).unwrap(), target_a);

    // Atomically replaces an existing symlink (no EEXIST).
    make_symlink(&target_b, &link).unwrap();
    assert_eq!(fs::read_link(&link).unwrap(), target_b);

    // The temporary symlink is consumed by the rename — nothing left behind.
    let leftovers: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .map(|e| e.file_name())
        .collect();
    assert!(
        leftovers.is_empty(),
        "temp symlink left behind: {leftovers:?}"
    );
}

#[test]
#[cfg(windows)]
fn test_is_unc_path() {
    assert!(is_unc_path(Path::new(
        r"\\wsl.localhost\DistroName\github\verzly\mise-php"
    )));
    assert!(is_unc_path(Path::new(
        r"\\wsl$\DistroName\github\verzly\mise-php"
    )));
    assert!(is_unc_path(Path::new(
        r"\\?\UNC\wsl.localhost\DistroName\github\verzly\mise-php"
    )));

    assert!(!is_unc_path(Path::new(r"D:\github\verzly\mise-php")));
}
