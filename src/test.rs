use std::fs;

use indoc::indoc;

use crate::{cmd, env};

#[ctor::ctor]
fn init() {
    env::set_var("NO_COLOR", "1");
    env_logger::init();
    let _ = fs::remove_dir_all("test/data/legacy_cache");
    if let Err(err) = cmd!(
        "git",
        "checkout",
        "test/.tool-versions",
        "test/cwd/.tool-versions",
        "test/config/config.toml"
    )
    .run()
    {
        warn!("failed to reset test files: {}", err);
    }

    reset_config();
}

pub fn reset_config() {
    fs::write(
        env::HOME.join("config/config.toml"),
        indoc! {r#"
                missing_runtime_behavior= 'autoinstall'
                always_keep_download= true
                legacy_version_file= true
                disable_plugin_short_name_repository= false
                plugin_repository_last_check_duration = 20
                plugin_autoupdate_last_check_duration = 20

                [alias.shfmt]
                "my/alias" = '3.0'
            "#},
    )
    .unwrap();
}
