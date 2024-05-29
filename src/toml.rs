#[macro_export]
macro_rules! parse_error {
    ($key:expr, $val:expr, $t:expr) => {{
        use eyre::bail;

        bail!(
            r#"expected value of {} to be a {}, got: {}"#,
            $crate::ui::style::eyellow($key),
            $crate::ui::style::ecyan($t),
            $crate::ui::style::eblue($val.to_string().trim()),
        )
    }};
}
