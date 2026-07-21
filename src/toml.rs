use std::collections::HashSet;

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

pub fn dedup_toml_array(array: &toml_edit::Array) -> toml_edit::Array {
    let mut seen = HashSet::new();
    let mut deduped = toml_edit::Array::new();
    for item in array.iter() {
        if seen.insert(item.as_str()) {
            deduped.push(item.clone());
        }
    }
    deduped
}
