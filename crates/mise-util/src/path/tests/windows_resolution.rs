use super::*;

#[test]
#[cfg(windows)]
fn test_is_bash_basename_accepts_bash_variants() {
    use std::ffi::OsStr;
    assert!(is_bash_basename(OsStr::new("bash")));
    assert!(is_bash_basename(OsStr::new("bash.exe")));
    assert!(is_bash_basename(OsStr::new("BASH.EXE")));
    assert!(is_bash_basename(OsStr::new(
        r"C:\Program Files\Git\bin\bash.exe"
    )));
    assert!(is_bash_basename(OsStr::new("/usr/bin/bash")));
}

#[test]
#[cfg(windows)]
fn test_is_bash_basename_rejects_other_shells() {
    use std::ffi::OsStr;
    assert!(!is_bash_basename(OsStr::new("sh")));
    assert!(!is_bash_basename(OsStr::new("zsh.exe")));
    assert!(!is_bash_basename(OsStr::new("fish")));
    assert!(!is_bash_basename(OsStr::new("dash")));
    assert!(!is_bash_basename(OsStr::new("cmd.exe")));
    assert!(!is_bash_basename(OsStr::new("bashfoo")));
}

#[test]
#[cfg(windows)]
fn test_is_wsl_launcher_bash_detects_system32() {
    assert!(is_wsl_launcher_bash(Path::new(
        r"C:\Windows\System32\bash.exe"
    )));
    assert!(is_wsl_launcher_bash(Path::new(
        r"C:\WINDOWS\system32\bash.exe"
    )));
    assert!(is_wsl_launcher_bash(Path::new(
        r"D:\Windows\System32\bash.exe"
    )));
}

#[test]
#[cfg(windows)]
fn test_is_wsl_launcher_bash_detects_windows_apps() {
    assert!(is_wsl_launcher_bash(Path::new(
        r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\bash.exe"
    )));
    // Forward slashes still match — `which::which_in` may produce them.
    assert!(is_wsl_launcher_bash(Path::new(
        "C:/Users/me/AppData/Local/Microsoft/WindowsApps/bash.exe"
    )));
}

#[test]
#[cfg(windows)]
fn test_is_wsl_launcher_bash_accepts_real_bash() {
    assert!(!is_wsl_launcher_bash(Path::new(
        r"C:\Program Files\Git\bin\bash.exe"
    )));
    assert!(!is_wsl_launcher_bash(Path::new(
        r"C:\Program Files\Git\usr\bin\bash.exe"
    )));
    assert!(!is_wsl_launcher_bash(Path::new(
        r"C:\msys64\usr\bin\bash.exe"
    )));
    assert!(!is_wsl_launcher_bash(Path::new(
        r"C:\Users\me\scoop\apps\git\current\bin\bash.exe"
    )));
}

#[test]
#[cfg(windows)]
fn test_bash_candidates_includes_program_files() {
    let env = std::collections::BTreeMap::new();
    let candidates = bash_candidates(&env);
    assert!(candidates.contains(&PathBuf::from(r"C:\Program Files\Git\bin\bash.exe")));
    assert!(candidates.contains(&PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe")));
}

#[test]
#[cfg(windows)]
fn test_bash_candidates_includes_msys2() {
    let env = std::collections::BTreeMap::new();
    let candidates = bash_candidates(&env);
    assert!(candidates.contains(&PathBuf::from(r"C:\msys64\usr\bin\bash.exe")));
    assert!(candidates.contains(&PathBuf::from(r"C:\msys32\usr\bin\bash.exe")));
}

#[test]
#[cfg(windows)]
fn test_bash_candidates_uses_localappdata_from_env() {
    let mut env = std::collections::BTreeMap::new();
    env.insert(
        "LOCALAPPDATA".to_string(),
        r"C:\Users\me\AppData\Local".to_string(),
    );
    let candidates = bash_candidates(&env);
    assert!(candidates.contains(&PathBuf::from(
        r"C:\Users\me\AppData\Local\Programs\Git\bin\bash.exe"
    )));
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_uses_mise_bash_path_override() {
    // SAFETY: tests in this module run sequentially within the cargo test runner;
    // env mutation is scoped via a guard.
    let tmp = tempfile::tempdir().expect("tempdir");
    let bash_path = tmp.path().join("custom-bash.exe");
    std::fs::write(&bash_path, b"").expect("write fake bash");

    let mut env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
    env.insert(
        "MISE_BASH_PATH".to_string(),
        bash_path.to_string_lossy().into_owned(),
    );

    let resolved = resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
        .expect("override should resolve");
    assert_eq!(PathBuf::from(&resolved), bash_path);
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_override_beats_unix_form_path_gate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let bash_path = tmp.path().join("custom-bash.exe");
    std::fs::write(&bash_path, b"").expect("write fake bash");

    // A Unix-form PATH normally ends resolution early, but the explicit
    // override still wins (e.g. `_.source` under mise running in Git Bash).
    let mut env = env_with_path("/c/foo:/d/bar");
    env.insert(
        "MISE_BASH_PATH".to_string(),
        bash_path.to_string_lossy().into_owned(),
    );

    let resolved = resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env)
        .expect("override should resolve");
    assert_eq!(PathBuf::from(&resolved), bash_path);
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_skips_when_not_posix_shell() {
    let env = env_with_path(r"C:\Windows\System32");
    assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("cmd.exe"), &env).is_none());
    assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("notepad.exe"), &env).is_none());
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_skips_when_path_already_unix() {
    let env = env_with_path("/c/foo:/d/bar");
    assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("bash"), &env).is_none());
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_honors_explicit_forward_slash_path() {
    // #9932: an explicit absolute bash path must be kept verbatim (None here,
    // so the caller keeps the original), NOT re-resolved to Git Bash via the
    // candidate list — even with a Windows-form PATH that would otherwise
    // trigger resolution.
    let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
    assert!(
        resolve_posix_shell_program_path(std::ffi::OsStr::new("C:/msys64/usr/bin/bash.exe"), &env)
            .is_none()
    );
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_honors_explicit_path_backslashes() {
    let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
    assert!(
        resolve_posix_shell_program_path(std::ffi::OsStr::new(r"C:\msys64\usr\bin\bash.exe"), &env)
            .is_none()
    );
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_honors_explicit_relative_path() {
    // A relative path with a separator is still an explicit choice, not a
    // bare name to look up on PATH.
    let env = env_with_path(r"C:\Windows\System32;C:\Program Files\Git\bin");
    assert!(resolve_posix_shell_program_path(std::ffi::OsStr::new("bin/bash"), &env).is_none());
}

#[test]
#[cfg(windows)]
fn test_resolve_posix_shell_program_path_honors_explicit_non_bash_shell_path() {
    // An explicit path to a non-bash POSIX shell is honored verbatim too.
    let env = env_with_path(r"C:\Windows\System32;C:\msys64\usr\bin");
    assert!(
        resolve_posix_shell_program_path(std::ffi::OsStr::new(r"C:\msys64\usr\bin\zsh.exe"), &env)
            .is_none()
    );
}

#[test]
#[cfg(windows)]
fn test_program_has_directory_component_detects_explicit_paths() {
    use std::ffi::OsStr;
    assert!(program_has_directory_component(OsStr::new(
        "C:/msys64/usr/bin/bash.exe"
    )));
    assert!(program_has_directory_component(OsStr::new(
        r"C:\msys64\usr\bin\bash.exe"
    )));
    assert!(program_has_directory_component(OsStr::new("./bash")));
    assert!(program_has_directory_component(OsStr::new("bin/bash")));
    assert!(program_has_directory_component(OsStr::new("/usr/bin/bash")));
}

#[test]
#[cfg(windows)]
fn test_program_has_directory_component_rejects_bare_names() {
    use std::ffi::OsStr;
    assert!(!program_has_directory_component(OsStr::new("bash")));
    assert!(!program_has_directory_component(OsStr::new("bash.exe")));
    assert!(!program_has_directory_component(OsStr::new("BASH.EXE")));
}
