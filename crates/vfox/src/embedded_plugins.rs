// This module provides access to embedded vfox plugin Lua code.
// The actual code is generated at build time by build.rs

include!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));

#[cfg(test)]
mod tests {
    // Build script output is cached and reused by jobs that check the repo out elsewhere, so the
    // generated code must resolve plugin files at compile time instead of baking in a checkout path.
    #[test]
    fn generated_code_has_no_checkout_path() {
        let generated = include_str!(concat!(env!("OUT_DIR"), "/embedded_plugins.rs"));
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        assert!(
            !generated.contains(manifest_dir)
                && !generated.contains(&manifest_dir.replace('\\', "/")),
            "embedded_plugins.rs must not contain {manifest_dir}"
        );
        // Without an `embedded-plugins` directory build.rs generates an empty implementation.
        if !super::list_embedded_plugins().is_empty() {
            assert!(generated.contains("env!(\"CARGO_MANIFEST_DIR\")"));
        }
    }
}
