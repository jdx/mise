use super::*;

#[test]
fn test_split_shell_command_bare_names() {
    assert_eq!(split_shell_command("bash -c").unwrap(), sv(&["bash", "-c"]));
    assert_eq!(split_shell_command("sh -c").unwrap(), sv(&["sh", "-c"]));
    assert_eq!(
        split_shell_command("sh -o errexit -c").unwrap(),
        sv(&["sh", "-o", "errexit", "-c"])
    );
}

#[test]
fn test_split_shell_command_empty() {
    assert_eq!(split_shell_command("").unwrap(), sv(&[]));
    assert_eq!(split_shell_command("   ").unwrap(), sv(&[]));
}

#[test]
fn test_split_shell_command_quoted_path_with_spaces() {
    // A double-quoted path containing spaces is one token on both platforms.
    assert_eq!(
        split_shell_command("\"C:/Program Files/Git/bin/bash.exe\" -c").unwrap(),
        sv(&["C:/Program Files/Git/bin/bash.exe", "-c"])
    );
}

#[cfg(windows)]
#[test]
fn test_split_shell_command_windows_backslash_is_literal() {
    // Backslash is a plain path char on Windows, not an escape.
    assert_eq!(
        split_shell_command(r"C:\msys64\usr\bin\bash.exe -c").unwrap(),
        sv(&[r"C:\msys64\usr\bin\bash.exe", "-c"])
    );
    assert_eq!(
        split_shell_command("\"C:\\Program Files\\Git\\bin\\bash.exe\" -c").unwrap(),
        sv(&[r"C:\Program Files\Git\bin\bash.exe", "-c"])
    );
}

#[cfg(windows)]
#[test]
fn test_split_shell_command_windows_unquoted_space_splits() {
    // Documented ambiguity: an unquoted space splits even inside a path.
    assert_eq!(
        split_shell_command(r"C:/Program Files/Git/bin/bash.exe -c").unwrap(),
        sv(&["C:/Program", "Files/Git/bin/bash.exe", "-c"])
    );
}

#[cfg(windows)]
#[test]
fn test_split_shell_command_windows_double_quote_is_literal() {
    // `""` inside a quoted span → a literal `"`.
    assert_eq!(
        split_shell_command("\"a\"\"b\" c").unwrap(),
        sv(&["a\"b", "c"])
    );
}

#[cfg(windows)]
#[test]
fn test_split_shell_command_windows_unbalanced_quote_errs() {
    assert!(split_shell_command("\"unterminated").is_err());
}

#[cfg(not(windows))]
#[test]
fn test_split_shell_command_unix_posix_semantics() {
    // Unix keeps shell_words (POSIX) behavior: backslash escapes, single quotes group.
    assert_eq!(
        split_shell_command(r"bash\ script -c").unwrap(),
        sv(&["bash script", "-c"])
    );
    assert_eq!(split_shell_command("'a b' c").unwrap(), sv(&["a b", "c"]));
}

#[test]
fn test_unix_path_to_windows_msys_drive_paths() {
    assert_eq!(unix_path_to_windows("/c/foo").as_deref(), Some(r"C:\foo"));
    assert_eq!(unix_path_to_windows("/C/foo").as_deref(), Some(r"C:\foo"));
    assert_eq!(
        unix_path_to_windows("/c/Program Files/Git").as_deref(),
        Some(r"C:\Program Files\Git")
    );
    assert_eq!(unix_path_to_windows("/c").as_deref(), Some(r"C:\"));
    assert_eq!(unix_path_to_windows("/c/").as_deref(), Some(r"C:\"));
}

#[test]
fn test_unix_path_to_windows_cygdrive_paths() {
    assert_eq!(
        unix_path_to_windows("/cygdrive/c/foo").as_deref(),
        Some(r"C:\foo")
    );
    assert_eq!(unix_path_to_windows("/cygdrive/c").as_deref(), Some(r"C:\"));
    assert_eq!(unix_path_to_windows("/cygdrive"), None);
    // Not the cygdrive prefix — just a dir that starts with the same letters.
    assert_eq!(unix_path_to_windows("/cygdrive2/c/x"), None);
}

#[test]
fn test_unix_path_to_windows_already_windows() {
    assert_eq!(
        unix_path_to_windows("C:/already").as_deref(),
        Some(r"C:\already")
    );
    assert_eq!(
        unix_path_to_windows(r"C:\already").as_deref(),
        Some(r"C:\already")
    );
}

#[test]
fn test_unix_path_to_windows_unc() {
    assert_eq!(
        unix_path_to_windows("//server/share/dir").as_deref(),
        Some(r"\\server\share\dir")
    );
    assert_eq!(unix_path_to_windows("//"), None);
}

#[test]
fn test_unix_path_to_windows_no_windows_equivalent() {
    assert_eq!(unix_path_to_windows("/usr/bin"), None);
    assert_eq!(unix_path_to_windows("/mingw64/bin"), None);
    assert_eq!(unix_path_to_windows("relative/x"), None);
    assert_eq!(unix_path_to_windows(""), None);
    // `/cc/foo` — two-letter first segment is not a drive.
    assert_eq!(unix_path_to_windows("/cc/foo"), None);
}
