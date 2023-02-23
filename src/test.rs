use std::fs;

use indoc::indoc;

use crate::{assert_cli, cmd, env};

#[ctor::ctor]
fn init() {
    env::set_var("NO_COLOR", "1");
    env_logger::init();
    let _ = fs::remove_dir_all("test/cache");
    let _ = fs::remove_dir_all("test/data");
    let _ = fs::remove_dir_all("plugins");
    if let Err(err) = cmd!(
        "git",
        "checkout",
        "plugins",
        "test/.test-tool-versions",
        "test/cwd/.test-tool-versions",
        "test/config/config.toml",
        "test/data"
    )
    .run()
    {
        warn!("failed to reset test files: {}", err);
    }
    reset_config();
    assert_cli!("install", "tiny", "dummy");
}

pub fn reset_config() {
    fs::write(
        env::HOME.join("config/config.toml"),
        indoc! {r#"
                verbose = true
                missing_runtime_behavior= 'autoinstall'
                always_keep_download= true
                legacy_version_file= true
                plugin_autoupdate_last_check_duration = 20
                jobs = 2

                [alias.tiny]
                "my/alias" = '3.0'
            "#},
    )
    .unwrap();
}
