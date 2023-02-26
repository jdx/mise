use std::env::join_paths;
use std::fs;

use indoc::indoc;

use crate::{assert_cli, env};

#[ctor::ctor]
fn init() {
    env::set_var("NO_COLOR", "1");
    env_logger::init();
    reset_config();
    assert_cli!("install", "tiny@1", "tiny@2", "tiny@3", "tiny", "dummy");
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

pub fn replace_path(input: &str) -> String {
    let path = join_paths(&*env::PATH)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let home = env::HOME.to_string_lossy().to_string();
    input
        .replace(path.as_str(), "$PATH")
        .replace(home.as_str(), "~")
}
