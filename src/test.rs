use std::env::{join_paths, set_current_dir};
use std::fs;
use std::path::PathBuf;

use indoc::indoc;

use crate::{assert_cli, env};

#[ctor::ctor]
fn init() {
    if env::var("__RTX_DIFF").is_ok() {
        // TODO: fix this
        panic!("cannot run tests when rtx is activated");
    }
    env::set_var(
        "HOME",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"),
    );
    set_current_dir(env::HOME.join("cwd")).unwrap();
    env::set_var("NO_COLOR", "1");
    env::set_var("RTX_DATA_DIR", env::HOME.join("data"));
    env::set_var("RTX_CONFIG_DIR", env::HOME.join("config"));
    env::set_var("RTX_CACHE_DIR", env::HOME.join("data/cache"));
    env::set_var("RTX_DEFAULT_TOOL_VERSIONS_FILENAME", ".test-tool-versions");
    env::set_var("RTX_DEFAULT_CONFIG_FILENAME", ".test.rtx.toml");
    env::set_var("RTX_MISSING_RUNTIME_BEHAVIOR", "autoinstall");
    //env::set_var("TERM", "dumb");
    env_logger::init();
    reset_config();
    assert_cli!("install", "tiny@1", "tiny@2", "tiny@3", "tiny", "dummy");
}

pub fn reset_config() {
    fs::write(
        env::HOME.join(".test-tool-versions"),
        indoc! {r#"
            tiny  2
            dummy ref:master
            "#},
    )
    .unwrap();
    fs::write(
        env::PWD.join(".test-tool-versions"),
        indoc! {r#"
            tiny 3
            "#},
    )
    .unwrap();
    fs::write(
        env::HOME.join("config/config.toml"),
        indoc! {r#"
            [settings]
            experimental = true
            verbose = true
            missing_runtime_behavior= 'autoinstall'
            always_keep_download= true
            legacy_version_file= true
            plugin_autoupdate_last_check_duration = 20
            jobs = 2
            shims_dir = '~/data/shims'

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
        .replace(env::RTX_EXE.to_string_lossy().as_ref(), "rtx")
}
