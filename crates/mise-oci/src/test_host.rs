//! The unit tests below mise have no config system, so they read the `settings.toml` defaults.

mise_util::run_before_main!(init);

fn init() {
    let home = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test");
    std::fs::create_dir_all(&home).unwrap();
    mise_util::env::set_var("HOME", &home);
    mise_util::testing::enable(home);
    mise_settings::set_loader(mise_settings::load_defaults);
    mise_util::cache::set_base_cache_keys(vec!["mise-oci-tests".to_string()]);
    mise_util::user_agent::set("mise-oci-tests".to_string());
    mise_util::deprecation::set_version(env!("CARGO_PKG_VERSION"));
    mise_util::env::set_mise_env(|| &[]);
    mise_util::shells::set_implicit_inline_shell(|| true);
}
