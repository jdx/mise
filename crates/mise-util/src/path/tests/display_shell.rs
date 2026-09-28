use super::*;

/// `canonicalize` hands back an extended-length path on Windows, and mise used to print it.
/// Only the drive form is simplified -- see the negative cases, which name shapes that do not
/// resolve without the prefix.
#[cfg(windows)]
#[test]
fn test_display_user_drops_the_extended_length_prefix() {
    assert_eq!(
        Path::new(r"\\?\C:\Users\me\proj").display_user(),
        r"C:\Users\me\proj"
    );
    // An ordinary path is untouched.
    assert_eq!(
        Path::new(r"C:\Users\me\proj").display_user(),
        r"C:\Users\me\proj"
    );
    // A real UNC path is not an extended-length one and must survive intact.
    assert_eq!(
        Path::new(r"\\server\share\proj").display_user(),
        r"\\server\share\proj"
    );
    // A verbatim UNC path does have a plain equivalent, and it is the only form mise accepts
    // back as input, so the prefix goes. `dunce` leaves this one alone: `is_safe_to_strip_unc`
    // takes `Prefix::VerbatimDisk` and nothing else.
    assert_eq!(
        Path::new(r"\\?\UNC\server\share").display_user(),
        r"\\server\share"
    );
    // The shape this was found on, from a project reached through WSL.
    assert_eq!(
        Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\me\proj").display_user(),
        r"\\wsl.localhost\Ubuntu\home\me\proj"
    );
    // A device path has no plain equivalent and keeps its prefix.
    assert_eq!(Path::new(r"\\.\COM1").display_user(), r"\\.\COM1");
    // A reserved name only resolves through the verbatim form.
    assert_eq!(
        Path::new(r"\\?\C:\proj\CON").display_user(),
        r"\\?\C:\proj\CON"
    );
}

/// The three shapes where `\\?\UNC\…` has to keep its prefix, because the plain form would not
/// name the same file. Each is a way the shorter answer would be wrong rather than merely ugly.
#[cfg(windows)]
#[test]
fn test_display_user_keeps_the_prefix_when_unc_needs_it() {
    // `/` is an ordinary character inside `\\?\`, so `a/b` is one component here and two in the
    // plain form. `display_path`'s tests pin the same trap for the disk prefix.
    assert_eq!(
        Path::new(r"\\?\UNC\server\share\a/b").display_user(),
        r"\\?\UNC\server\share\a/b"
    );
    // Past MAX_PATH the prefix is what makes the path work.
    let long = format!(r"\\?\UNC\server\share\{}", "d".repeat(260));
    assert_eq!(Path::new(&long).display_user(), long);
    // A trailing dot or space only survives the verbatim form.
    assert_eq!(
        Path::new(r"\\?\UNC\server\share\proj.").display_user(),
        r"\\?\UNC\server\share\proj."
    );
    assert_eq!(
        Path::new(r"\\?\UNC\server\share\proj ").display_user(),
        r"\\?\UNC\server\share\proj "
    );
}

/// The rewrite is display-only, so it lives beside `display_path` rather than in
/// `display_user` -- see the tests over there. This one pins the piece in isolation, and that
/// `display_user` itself leaves separators alone, since strings mise *matches* go through it.
#[test]
fn test_settle_display_separators() {
    #[cfg(windows)]
    {
        assert_eq!(
            settle_display_separators(r"C:/Users/me\proj\.git/hooks".to_string()),
            r"C:\Users\me\proj\.git\hooks"
        );
        assert_eq!(
            Path::new("C:/Users/me").display_user(),
            "C:/Users/me",
            "display_user must not settle separators"
        );
    }
    #[cfg(not(windows))]
    {
        // `\` is an ordinary filename character here, so nothing is rewritten.
        assert_eq!(settle_display_separators(r"/a/b\c".to_string()), r"/a/b\c");
    }
}

/// The prefix cannot occur on unix (`\` is an ordinary filename character there), so nothing
/// is stripped, no separator is rewritten, and the `~` substitution keeps working.
#[cfg(not(windows))]
#[test]
fn test_display_user_leaves_unix_paths_alone() {
    assert_eq!(Path::new("/usr/local/bin").display_user(), "/usr/local/bin");
    // The boundary for the separator rewrite: a file really can be named `weird\name` here,
    // so turning that into a separator would rename it in the message.
    assert_eq!(Path::new(r"weird\name").display_user(), r"weird\name");
    assert_eq!(Path::new(r"/a/b\c").display_user(), r"/a/b\c");
    // The substitution the refactor had to leave intact. `display_user` skips it when HOME is
    // `/`, so the assertion does too rather than depending on the runner's environment.
    if dirs::HOME.as_os_str() != "/" {
        assert_eq!(dirs::HOME.join("proj").display_user(), "~/proj");
    }
}

#[test]
fn test_is_posix_shell_program() {
    assert!(is_posix_shell_program(Path::new("bash")));
    assert!(is_posix_shell_program(Path::new("bash.exe")));
    assert!(is_posix_shell_program(Path::new("BASH.EXE")));
    assert!(is_posix_shell_program(Path::new(
        r"C:\Program Files\Git\bin\bash.exe"
    )));
    assert!(is_posix_shell_program(Path::new("/usr/bin/bash")));
    assert!(is_posix_shell_program(Path::new("sh")));
    assert!(is_posix_shell_program(Path::new("zsh")));
    assert!(is_posix_shell_program(Path::new("fish")));
    assert!(is_posix_shell_program(Path::new("ash")));

    assert!(!is_posix_shell_program(Path::new("cmd")));
    assert!(!is_posix_shell_program(Path::new("cmd.exe")));
    assert!(!is_posix_shell_program(Path::new("powershell")));
    assert!(!is_posix_shell_program(Path::new("pwsh.exe")));
    assert!(!is_posix_shell_program(Path::new("rustc")));
    assert!(!is_posix_shell_program(Path::new("")));
}

#[test]
fn test_command_mode_script_payload() {
    let payload = |p: &str| command_mode_script_payload(Path::new(p));

    for posix in [
        "bash",
        "sh",
        "zsh",
        "ksh",
        "dash",
        // Alpine's `/bin/sh`. Measured in an alpine container: `ash -c <path> ARG1` drops
        // the argument exactly as the others do, and the payload restores it.
        "ash",
        "/usr/bin/bash",
        "BASH.EXE",
    ] {
        assert_eq!(payload(posix), Some(r#""$0" "$@""#), "{posix}");
    }

    // The whole reason this is not just `is_posix_shell_program`: fish answers true there
    // and cannot use `$@`.
    for fish in ["fish", "fish.exe", "/usr/bin/fish"] {
        assert_eq!(payload(fish), Some("$argv"), "{fish}");
    }

    // cmd forwards arguments after `/c` already; PowerShell has no `$0`/`$@`.
    for other in ["cmd", "cmd.exe", "pwsh", "powershell.exe", "rustc", ""] {
        assert_eq!(payload(other), None, "{other}");
    }
}

#[test]
fn test_is_cmd_shell_program() {
    assert!(is_cmd_shell_program(Path::new("cmd")));
    assert!(is_cmd_shell_program(Path::new("cmd.exe")));
    assert!(is_cmd_shell_program(Path::new("CMD.EXE")));
    assert!(is_cmd_shell_program(Path::new(
        r"C:\Windows\System32\cmd.exe"
    )));

    assert!(!is_cmd_shell_program(Path::new("bash")));
    assert!(!is_cmd_shell_program(Path::new("bash.exe")));
    assert!(!is_cmd_shell_program(Path::new("powershell")));
    assert!(!is_cmd_shell_program(Path::new("pwsh.exe")));
    // `cmd.com` is not the modern interpreter we target.
    assert!(!is_cmd_shell_program(Path::new("cmd.com")));
    assert!(!is_cmd_shell_program(Path::new("")));
}

#[test]
fn test_is_powershell_program() {
    assert!(is_powershell_program(Path::new("pwsh")));
    assert!(is_powershell_program(Path::new("pwsh.exe")));
    assert!(is_powershell_program(Path::new("PWSH.EXE")));
    assert!(is_powershell_program(Path::new("powershell")));
    assert!(is_powershell_program(Path::new("powershell.exe")));
    assert!(is_powershell_program(Path::new(
        r"C:\Program Files\PowerShell\7\pwsh.exe"
    )));

    assert!(!is_powershell_program(Path::new("cmd")));
    assert!(!is_powershell_program(Path::new("bash")));
    assert!(!is_powershell_program(Path::new("")));
}

#[test]
fn test_inject_powershell_no_profile() {
    let inject = |args: &[&str]| {
        let mut v = sv(args);
        inject_powershell_no_profile(&mut v);
        v
    };

    // Injected right after the program, before -Command.
    assert_eq!(
        inject(&["pwsh", "-Command"]),
        sv(&["pwsh", "-NoProfile", "-Command"])
    );
    assert_eq!(
        inject(&["powershell", "-c"]),
        sv(&["powershell", "-NoProfile", "-c"])
    );
    assert_eq!(
        inject(&["pwsh.exe", "-NoLogo", "-Command"]),
        sv(&["pwsh.exe", "-NoProfile", "-NoLogo", "-Command"])
    );

    // Non-PowerShell shells are untouched.
    assert_eq!(inject(&["cmd", "/c"]), sv(&["cmd", "/c"]));
    assert_eq!(inject(&["bash", "-c"]), sv(&["bash", "-c"]));
    assert_eq!(inject(&[]), sv(&[]));

    // Idempotent — already present in any accepted spelling/abbreviation.
    assert_eq!(
        inject(&["pwsh", "-NoProfile", "-Command"]),
        sv(&["pwsh", "-NoProfile", "-Command"])
    );
    assert_eq!(
        inject(&["pwsh", "-noprofile", "-c"]),
        sv(&["pwsh", "-noprofile", "-c"])
    );
    assert_eq!(inject(&["pwsh", "-nop", "-c"]), sv(&["pwsh", "-nop", "-c"]));
    assert_eq!(
        inject(&["pwsh", "/NoProfile", "-c"]),
        sv(&["pwsh", "/NoProfile", "-c"])
    );

    // NoProfile-like payload arguments after -Command/-File are not shell
    // options and must not prevent injection.
    assert_eq!(
        inject(&["pwsh", "-Command", "-nop"]),
        sv(&["pwsh", "-NoProfile", "-Command", "-nop"])
    );
    assert_eq!(
        inject(&["pwsh", "-File", "script.ps1", "-nop"]),
        sv(&["pwsh", "-NoProfile", "-File", "script.ps1", "-nop"])
    );

    // -NoProfileLoadTime does not suppress the profile, so still injected.
    assert_eq!(
        inject(&["pwsh", "-NoProfileLoadTime", "-c"]),
        sv(&["pwsh", "-NoProfile", "-NoProfileLoadTime", "-c"])
    );

    // -NoLogo starts with "-no" but is not a NoProfile abbreviation.
    assert_eq!(
        inject(&["pwsh", "-NoLogo", "-c"]),
        sv(&["pwsh", "-NoProfile", "-NoLogo", "-c"])
    );
}

#[test]
fn test_cmd_verbatim_args() {
    let c = || "/c".to_string();

    // The reported case: inner double quotes must survive untouched inside
    // the single outer quote pair, with `/s` ensured. The caller passes
    // each element as a raw arg, so the resulting command line is
    // `cmd /s /c "uv run python -c "import x""` — cmd strips only the outer
    // pair. See discussion #9355.
    assert_eq!(
        cmd_verbatim_args(&[c()], r#"uv run python -c "import x""#, &[]),
        sv(&["/s", "/c", r#""uv run python -c "import x"""#])
    );

    // No inner quotes — still wrapped, still gets `/s`.
    assert_eq!(
        cmd_verbatim_args(&[c()], "echo hi", &[]),
        sv(&["/s", "/c", r#""echo hi""#])
    );

    // Forwarded args go inside the same outer quote pair, each MSVCRT-quoted
    // when it contains spaces so the program still sees them as one argument
    // (preserving the #6744 spaces-in-args fix). `c` stays bare.
    assert_eq!(
        cmd_verbatim_args(&[c()], "proxy", &["a b".to_string(), "c".to_string()]),
        sv(&["/s", "/c", r#""proxy "a b" c""#])
    );

    // A forwarded path with a space (mirrors e2e-win/task_args.Tests.ps1):
    // `type ".\test dir\file.txt"` must reach `type` as a single argument.
    assert_eq!(
        cmd_verbatim_args(&[c()], "type", &[r".\test dir\file.txt".to_string()]),
        sv(&["/s", "/c", r#""type ".\test dir\file.txt"""#])
    );

    // An explicit `/s` in the shell flags is not duplicated.
    assert_eq!(
        cmd_verbatim_args(&["/s".to_string(), c()], "echo hi", &[]),
        sv(&["/s", "/c", r#""echo hi""#])
    );
}

#[test]
fn test_positional_cmd_command() {
    // How pitchfork runs a `mise = true` daemon: `mise x -- cmd /C <run>`.
    let args = sv(&["/C", r#"echo init && "C:\a b\mise.exe" run "dev""#]);
    assert_eq!(
        positional_cmd_command(&args),
        Some((&args[..1], args[1].as_str()))
    );
    // Other switches may come first, and `/k` runs a command too.
    let args = sv(&["/d", "/s", "/k", r#"echo "a b""#]);
    assert_eq!(
        positional_cmd_command(&args),
        Some((&args[..3], args[3].as_str()))
    );
    // Without a `"`, std's quoting already reaches cmd intact: a path with a
    // space works because cmd keeps the only two quotes std adds.
    assert_eq!(
        positional_cmd_command(&sv(&["/c", r"C:\Program Files\app.exe"])),
        None
    );
    // Several arguments after `/c` stay arguments, not one command.
    assert_eq!(
        positional_cmd_command(&sv(&["/c", r#"echo "one""#, "two"])),
        None
    );
    assert_eq!(positional_cmd_command(&sv(&["x", "/c", r#""a""#])), None);
    assert_eq!(positional_cmd_command(&sv(&[r#""a""#])), None);
    assert_eq!(positional_cmd_command(&[]), None);
}

#[test]
fn test_quote_arg_for_cmd_body() {
    // No special chars -> returned as-is (no quotes added).
    assert_eq!(quote_arg_for_cmd_body("plain"), "plain");
    // Space/tab -> wrapped.
    assert_eq!(quote_arg_for_cmd_body("a b"), r#""a b""#);
    // Empty -> quoted empty string.
    assert_eq!(quote_arg_for_cmd_body(""), r#""""#);
    // Inner quote -> escaped as \".
    assert_eq!(quote_arg_for_cmd_body(r#"a"b"#), r#""a\"b""#);
    // Trailing backslashes are doubled before the closing quote (only when
    // the arg is quoted because it also contains a space).
    assert_eq!(quote_arg_for_cmd_body(r"a b\"), r#""a b\\""#);
    // A backslash not adjacent to a quote is left alone.
    assert_eq!(quote_arg_for_cmd_body(r"a\b c"), r#""a\b c""#);
    // cmd metacharacters (no whitespace) are still quoted so cmd does not
    // interpret them as shell syntax after stripping the outer quote pair.
    assert_eq!(quote_arg_for_cmd_body("a&b"), r#""a&b""#);
    assert_eq!(quote_arg_for_cmd_body("foo|bar"), r#""foo|bar""#);
    assert_eq!(quote_arg_for_cmd_body("a>b"), r#""a>b""#);
}

#[test]
#[cfg(windows)]
fn test_cmd_verbatim_command() {
    // cmd + /c -> Some; the body is wrapped in one outer quote pair with `/s`
    // ensured, passed via raw_arg (which appears verbatim in get_args()).
    let c = cmd_verbatim_command("cmd", &["/c".to_string()], r#"echo "a b""#).unwrap();
    assert_eq!(c.get_program().to_str(), Some("cmd"));
    let args: Vec<String> = c
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        vec![
            "/s".to_string(),
            "/c".to_string(),
            r#""echo "a b"""#.to_string()
        ]
    );
    // /k also runs a command string.
    assert!(cmd_verbatim_command("cmd", &["/k".to_string()], "echo hi").is_some());
    // Non-cmd shell, or cmd without /c|/k -> None (caller falls through).
    assert!(cmd_verbatim_command("bash", &["-c".to_string()], "echo hi").is_none());
    assert!(cmd_verbatim_command("cmd", &[], "echo hi").is_none());
}
