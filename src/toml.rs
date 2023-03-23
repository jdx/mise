#[macro_export]
macro_rules! parse_error {
    ($key:expr, $val:expr, $t:expr) => {{
        Err(eyre!(
            r#"expected value of "{}" to be a {}, got: {}"#,
            $key,
            $t,
            $val
        ))
    }};
}
