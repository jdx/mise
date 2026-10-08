#[test]
fn test_cmd_body_args_cmd_verbatim() {
    // cmd /c <body>: the verbatim branch produces the cmd_verbatim_args output
    // (`/s /c "<body>"`) rather than the fall-through [/c, <body>] layout.
    let r = super::CmdLineRunner::new("cmd").cmd_body_args(&["/c".to_string()], r#"echo "a b""#);
    assert!(r.get_program().to_lowercase().contains("cmd"));
    assert_eq!(
        r.get_args(),
        vec![
            "/s".to_string(),
            "/c".to_string(),
            r#""echo "a b"""#.to_string()
        ]
    );
}

#[test]
fn test_cmd_body_args_non_cmd_fallthrough() {
    // A non-cmd Windows shell keeps the plain args(flags).arg(body) layout.
    let r =
        super::CmdLineRunner::new("pwsh").cmd_body_args(&["-Command".to_string()], r#"echo "a b""#);
    assert_eq!(
        r.get_args(),
        vec!["-Command".to_string(), r#"echo "a b""#.to_string()]
    );
}

/// A deny_env sandbox clears the inherited environment, and says so through `inherit_env`: a
/// command with a timeout is rebuilt under a Ctrl+C group leader, which copies the environment
/// only by that flag and would otherwise hand mise's whole environment on.
#[tokio::test]
async fn test_deny_env_sandbox_marks_env_cleared() {
    use std::ffi::OsStr;

    let mut runner = super::CmdLineRunner::new("cmd")
        .env("KEEP", "value")
        .with_sandbox(crate::sandbox::SandboxConfig {
            deny_env: true,
            ..Default::default()
        });
    assert!(runner.inherit_env);

    runner.apply_sandbox().await.unwrap();

    assert!(!runner.inherit_env);
    assert!(
        runner.cmd.as_std().get_envs().any(|(key, value)| {
            key == OsStr::new("KEEP") && value == Some(OsStr::new("value"))
        })
    );
}
