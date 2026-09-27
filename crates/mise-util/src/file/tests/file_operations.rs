use pretty_assertions::assert_eq;

use super::*;

#[tokio::test]
async fn test_remove_file_async_if_exists_when_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    tokio::fs::write(&path, "content").await.unwrap();
    remove_file_async_if_exists(&path).await.unwrap();
    assert!(!path.exists());
}

#[tokio::test]
async fn test_remove_file_async_if_exists_when_file_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nonexistent");
    // Should not error when file does not exist.
    remove_file_async_if_exists(&path).await.unwrap();
}

#[cfg(all(unix, target_os = "linux"))]
#[test]
fn test_move_file_falls_back_to_copy_across_filesystems() {
    use std::{fs, os::unix::fs::MetadataExt};
    use tempfile::tempdir_in;

    let source_root = std::env::current_dir().unwrap();
    let source_dir = tempdir_in(&source_root).unwrap();
    let source_dev = source_dir.path().metadata().unwrap().dev();

    let target_dir = tempdir_in("/tmp").unwrap();
    if target_dir.path().metadata().unwrap().dev() == source_dev {
        // This host only has one filesystem for tempdirs, so skip if we can't reproduce EXDEV.
        return;
    }

    let src = source_dir.path().join("bun");
    let dst = target_dir.path().join("bun");
    fs::write(&src, b"hello").unwrap();

    move_file(&src, &dst).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
}

#[cfg(all(unix, target_os = "linux"))]
#[test]
fn test_move_dir_falls_back_to_copy_across_filesystems() {
    use std::{fs, os::unix::fs::MetadataExt};
    use tempfile::tempdir_in;

    let source_root = std::env::current_dir().unwrap();
    let source_dir = tempdir_in(&source_root).unwrap();
    let source_dev = source_dir.path().metadata().unwrap().dev();

    let target_dir = tempdir_in("/tmp").unwrap();
    if target_dir.path().metadata().unwrap().dev() == source_dev {
        // This host only has one filesystem for tempdirs, so skip if we can't reproduce EXDEV.
        return;
    }

    let src = source_dir.path().join("bun-tree");
    let dst = target_dir.path().join("bun-tree");
    fs::create_dir_all(src.join("nested")).unwrap();
    fs::write(src.join("nested/bun"), b"hello").unwrap();

    move_file(&src, &dst).unwrap();

    assert!(!src.exists());
    assert_eq!(fs::read(dst.join("nested/bun")).unwrap(), b"hello");
}

#[cfg(unix)]
#[test]
fn make_executable_hint_names_chmod_on_unix() {
    let hint = make_executable_hint(Path::new("/proj/mise-tasks/build"));
    assert!(hint.contains("chmod +x"), "{hint}");
    assert!(hint.contains("build"), "{hint}");
}

#[cfg(windows)]
#[test]
fn make_executable_hint_does_not_name_chmod_on_windows() {
    let hint = make_executable_hint(Path::new(r"C:\proj\mise-tasks\build"));
    // The point of the change: `chmod` is not just unavailable here, it is the wrong fix --
    // the Windows `is_executable` never looks at a permission bit.
    assert!(!hint.contains("chmod"), "{hint}");
    assert!(hint.contains("shebang"), "{hint}");
    // and it must offer what that branch actually accepts
    assert!(hint.contains("exe"), "{hint}");
    assert!(hint.contains("build"), "{hint}");
}

#[cfg(windows)]
#[test]
fn windows_io_hint_explains_a_path_at_the_limit() {
    use std::io::{Error, ErrorKind};

    let long = PathBuf::from(format!(r"C:\{}", "d".repeat(MAX_PATH)));
    let hint = windows_io_hint(&long, &Error::from_raw_os_error(3))
        .expect("a path past MAX_PATH failing with ERROR_PATH_NOT_FOUND should be explained");
    assert!(hint.contains(&MAX_PATH.to_string()), "{hint}");
    assert!(hint.contains(&(MAX_PATH + 3).to_string()), "{hint}");

    // The control, and the reason the length test is there at all: the same error on a path
    // nowhere near the limit is a genuinely missing directory, and answering it with advice
    // about path length would send the user after the wrong thing.
    assert_eq!(
        windows_io_hint(Path::new(r"C:\short"), &Error::from_raw_os_error(3)),
        None
    );

    // The other two codes Windows uses for the same cause.
    for code in [123, 206] {
        assert!(
            windows_io_hint(&long, &Error::from_raw_os_error(code)).is_some(),
            "os error {code}"
        );
    }

    // A different branch entirely, so a hint that only ever talked about length would fail.
    let in_use = windows_io_hint(Path::new(r"C:\short"), &Error::from_raw_os_error(32))
        .expect("a sharing violation should be explained");
    assert!(in_use.contains("in use"), "{in_use}");
    assert_eq!(
        windows_io_hint(Path::new(r"C:\short"), &Error::from(ErrorKind::NotFound)),
        None
    );
}

#[cfg(windows)]
#[test]
fn windows_io_hint_measures_the_units_windows_counts() {
    use std::io::Error;

    // `OsStr::len()` is WTF-8 bytes on Windows and the limit counts UTF-16 code units, so a
    // path of non-ASCII characters measures ~3x too long by bytes. Each of these is one unit
    // and three bytes: by bytes this is well past the limit, by units it is nowhere near.
    let short_but_fat = PathBuf::from(format!(r"C:\{}", "あ".repeat(100)));
    assert!(short_but_fat.as_os_str().len() > MAX_PATH, "fixture");
    assert_eq!(
        windows_io_hint(&short_but_fat, &Error::from_raw_os_error(3)),
        None
    );
}

#[test]
fn utf16_bom_reports_only_the_marks_that_hide_a_shebang() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = tmp.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    };
    const SCRIPT: &[u8] = b"#!/usr/bin/env bash\necho hi\n";

    // What Windows PowerShell 5.1's `>` and `Out-File` write by default.
    let mut le = vec![0xff, 0xfe];
    le.extend(SCRIPT.iter().flat_map(|b| [*b, 0]));
    assert_eq!(utf16_bom(&write("le", &le)), Some("UTF-16LE"));
    let mut be = vec![0xfe, 0xff];
    be.extend(SCRIPT.iter().flat_map(|b| [0, *b]));
    assert_eq!(utf16_bom(&write("be", &be)), Some("UTF-16BE"));

    // A UTF-8 mark is not reported: `has_shebang` reads past it, so such a file is executable
    // and never reaches a caller that has to explain why it is not.
    let mut utf8_bom = vec![0xef, 0xbb, 0xbf];
    utf8_bom.extend_from_slice(SCRIPT);
    assert_eq!(utf16_bom(&write("utf8_bom", &utf8_bom)), None);
    assert_eq!(utf16_bom(&write("plain", SCRIPT)), None);

    // Shorter than a mark, and absent entirely: answers, not panics.
    assert_eq!(utf16_bom(&write("empty", b"")), None);
    assert_eq!(utf16_bom(&write("one", b"#")), None);
    assert_eq!(utf16_bom(&tmp.path().join("does-not-exist")), None);
}

#[cfg(windows)]
#[test]
fn make_executable_hint_names_the_encoding_when_a_shebang_is_hidden_by_one() {
    let tmp = tempfile::tempdir().unwrap();
    let script = b"#!/usr/bin/env bash\necho hi\n";

    let utf16 = tmp.path().join("build");
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(script.iter().flat_map(|b| [*b, 0]));
    fs::write(&utf16, &bytes).unwrap();
    let hint = make_executable_hint(&utf16);
    // The shebang is the first thing in the file. Telling the user to add one sends them
    // after something that is already there, so the encoding has to be what is named.
    assert!(hint.contains("UTF-16LE"), "{hint}");
    assert!(hint.contains("UTF-8"), "{hint}");
    assert!(!hint.contains("Add a shebang line"), "{hint}");

    // The control: without a mark the advice is unchanged. Without this, an implementation
    // that always blamed the encoding would pass too.
    let utf8 = tmp.path().join("plain");
    fs::write(&utf8, b"echo hi\n").unwrap();
    let hint = make_executable_hint(&utf8);
    assert!(hint.contains("Add a shebang line"), "{hint}");
    assert!(!hint.contains("UTF-16"), "{hint}");
}

#[test]
fn strip_utf8_bom_removes_only_a_leading_mark() {
    assert_eq!(
        strip_utf8_bom("\u{feff}#!/usr/bin/env bash"),
        "#!/usr/bin/env bash"
    );
    assert_eq!(strip_utf8_bom("#!/usr/bin/env bash"), "#!/usr/bin/env bash");
    assert_eq!(strip_utf8_bom(""), "");
    // Only the first one, and only at the front: a mark elsewhere is content.
    assert_eq!(strip_utf8_bom("\u{feff}\u{feff}x"), "\u{feff}x");
    assert_eq!(strip_utf8_bom("x\u{feff}"), "x\u{feff}");
}

#[cfg(windows)]
#[test]
fn has_shebang_looks_past_a_utf8_bom() {
    let tmp = tempfile::tempdir().unwrap();
    let write = |name: &str, bytes: &[u8]| {
        let path = tmp.path().join(name);
        fs::write(&path, bytes).unwrap();
        path
    };
    const SCRIPT: &[u8] = b"#!/usr/bin/env bash\necho hi\n";
    let mut marked = UTF8_BOM_BYTES.to_vec();
    marked.extend_from_slice(SCRIPT);

    // The regression: `Out-File -Encoding utf8` writes this, and reading two bytes saw `EF BB`.
    let bom = write("bom", &marked);
    assert!(has_shebang(&bom));
    // On Windows a shebang is the whole of `is_executable` for a file with no known extension,
    // so this is what decided whether the task existed at all.
    assert!(is_executable(&bom));

    // Controls: the unmarked twin, and files that genuinely have no shebang.
    assert!(has_shebang(&write("plain", SCRIPT)));
    assert!(!has_shebang(&write("no_shebang", b"echo hi\n")));
    assert!(!is_executable(&write("no_shebang2", b"echo hi\n")));
    // Shorter than the buffer: a `read_exact` would error here rather than answer.
    assert!(!has_shebang(&write("bom_only", &UTF8_BOM_BYTES)));
    assert!(!has_shebang(&write("empty", b"")));
    assert!(!has_shebang(&write("one_byte", b"#")));
    // A UTF-16 mark is deliberately not accepted.
    assert!(!has_shebang(&write("utf16", b"\xff\xfe#\0!\0")));
}

/// `Path::join` appends a multi-segment literal verbatim, and some roots arrive
/// `/`-separated, so one printed path could switch form more than once.
#[cfg(windows)]
#[test]
fn display_path_settles_on_one_separator() {
    assert_eq!(
        display_path(r"C:/Users/me\proj\.git/hooks\pre-commit"),
        r"C:\Users\me\proj\.git\hooks\pre-commit"
    );
    assert_eq!(display_path("C:/Users/me"), r"C:\Users\me");
    // Already uniform, and a relative path: both unchanged.
    assert_eq!(display_path(r"C:\Users\me"), r"C:\Users\me");
    assert_eq!(display_path(r"a\b"), r"a\b");
    assert_eq!(display_path("a/b"), r"a\b");
    // Composes with the extended-length prefix being dropped.
    assert_eq!(display_path(r"\\?\C:\a\b"), r"C:\a\b");
    // Measured, not assumed: `/` is not a separator inside `\\?\`, so `a/b` reads as a single
    // component there and `dunce` declines to simplify a name Windows could not hold. The
    // separators still settle; the prefix stays. `canonicalize` never emits this shape.
    assert_eq!(display_path(r"\\?\C:\a/b"), r"\\?\C:\a\b");
    // A UNC path keeps its leading pair while its interior settles.
    assert_eq!(display_path(r"\\server\share/dir"), r"\\server\share\dir");
}

/// Both halves of the string take the host's separator. Hardcoding `./` and printing the
/// remainder raw gave `./mise-tasks\build` on Windows.
#[test]
fn display_rel_path_uses_one_separator() {
    let cwd = dirs::CWD.as_ref().expect("a cwd").clone();
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        display_rel_path(cwd.join("mise-tasks").join("build")),
        format!(".{sep}mise-tasks{sep}build")
    );
    // A path outside the cwd falls through to `display_path`, which settles separators too.
    let outside = display_rel_path(Path::new("relative-to-nothing"));
    assert!(!outside.starts_with('.'), "{outside}");
}

/// The list answers only "can the OS start this", so it is fixed and does not consult
/// settings. Case-insensitive, since Windows extensions are.
#[cfg(windows)]
#[test]
fn os_can_launch_extension_names_only_what_the_os_starts() {
    for ext in ["exe", "com", "bat", "cmd", "EXE", "Cmd"] {
        assert!(os_can_launch_extension(ext), "{ext}");
    }
    // ps1 and vbs need an interpreter; the rest are extensions a user might add to
    // `windows_executable_extensions`, which does not make CreateProcess able to start them.
    for ext in ["ps1", "PS1", "vbs", "sh", "py", "js", ""] {
        assert!(!os_can_launch_extension(ext), "{ext}");
    }
}

/// Under the shipped default `windows_executable_extensions` the answers are exactly what
/// they were before the whitelist replaced the interpreter-only blacklist -- the two agree on
/// that list, which is why the old shape looked correct.
#[cfg(windows)]
#[test]
fn can_execute_directly_is_unchanged_for_the_default_extensions() {
    for name in [r"C:\x\tool.exe", r"C:\x\tool.com", "tool.bat", "tool.CMD"] {
        assert!(can_execute_directly(Path::new(name)), "{name}");
    }
    for name in ["tool.ps1", "tool.PS1", "tool.vbs", "tool", r"C:\x\tool"] {
        assert!(!can_execute_directly(Path::new(name)), "{name}");
    }
}

/// `which` joins the bare name, so on Windows it never finds `ssh.exe`; host tools that
/// mise spawns by resolved path must go through `which_spawnable`.
#[cfg(windows)]
#[test]
fn which_spawnable_finds_the_exe_that_which_misses() {
    let dir = tempfile::tempdir().unwrap();
    let paths = [dir.path().to_path_buf()];
    fs::write(dir.path().join("ssh.exe"), "").unwrap();

    assert_eq!(_which("ssh", &paths), None);
    assert_eq!(
        _which_spawnable("ssh", &paths),
        Some(dir.path().join("ssh.exe"))
    );
}

fn io(raw: i32) -> std::io::Error {
    std::io::Error::from_raw_os_error(raw)
}

/// `mise uninstall` of a tool whose binary is still running reported only "Access is denied",
/// which on Windows is nearly always that and nothing else.
#[cfg(windows)]
#[test]
fn an_in_use_file_says_so() {
    for raw in [5, 32] {
        let hint = windows_io_hint(Path::new(r"C:\x\installs\jq\1.8.2"), &io(raw))
            .unwrap_or_else(|| panic!("raw {raw} should be explained"));
        assert!(hint.contains("in use"), "raw {raw}: {hint}");
    }
}

/// Measured: the atomic write breaks at a 253-character target while `fs::rename` on the same
/// tree succeeds at 415, so "path not found" here is the length rather than a missing parent.
#[cfg(windows)]
#[test]
fn a_path_near_the_limit_says_so() {
    let long = PathBuf::from(format!(r"C:\{}", "d".repeat(300)));
    let hint = windows_io_hint(&long, &io(3)).expect("a long path should be explained");
    assert!(hint.contains("260"), "{hint}");
}

/// The controls, and the point of the whole thing: do not attach an explanation to an error
/// that already means what it says.
#[cfg(windows)]
#[test]
fn an_ordinary_error_is_left_alone() {
    // A short path that is genuinely absent is not the `MAX_PATH` case.
    assert!(windows_io_hint(Path::new(r"C:\x\gone"), &io(2)).is_none());
    assert!(windows_io_hint(Path::new(r"C:\x\gone"), &io(3)).is_none());

    // And neither is an unrelated failure that happens to occur deep in a tree. Length alone
    // used to be enough to trigger the advice, which answered "disk full" with "shorten it".
    let long = PathBuf::from(format!(r"C:\{}", "d".repeat(300)));
    for raw in [2, 39, 112] {
        assert!(windows_io_hint(&long, &io(raw)).is_none(), "raw {raw}");
    }
}

/// The limit counts UTF-16 code units; `OsStr::len()` is WTF-8 bytes on Windows. A path of
/// non-ASCII characters is under the limit while measuring well over it in bytes.
#[cfg(windows)]
#[test]
fn the_limit_is_measured_the_way_windows_measures_it() {
    use std::os::windows::ffi::OsStrExt;

    // 200 three-byte characters: 600 bytes, 200 UTF-16 units.
    let path = PathBuf::from(format!(r"C:\{}", "あ".repeat(200)));
    assert!(path.as_os_str().len() > MAX_PATH, "premise");
    assert!(
        path.as_os_str().encode_wide().count() < MAX_PATH - 16,
        "premise"
    );
    assert!(windows_io_hint(&path, &io(3)).is_none());
}

/// unix has neither failure mode: a running binary can be unlinked, and there is no `MAX_PATH`.
#[cfg(not(windows))]
#[test]
fn unix_is_never_annotated() {
    let long = PathBuf::from(format!("/{}", "d".repeat(300)));
    for (path, raw) in [(Path::new("/x/installs/jq/1.8.2"), 13), (long.as_path(), 2)] {
        assert!(windows_io_hint(path, &io(raw)).is_none(), "{path:?}");
    }
}
