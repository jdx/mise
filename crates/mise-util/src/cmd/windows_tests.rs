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
