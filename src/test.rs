use std::env::join_paths;
use std::path::PathBuf;

use indoc::indoc;

use crate::{env, file};

#[ctor::ctor]
fn init() {
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "debug")
    }
    console::set_colors_enabled(false);
    console::set_colors_enabled_stderr(false);
    env::set_var(
        "HOME",
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"),
    );
    env::remove_var("MISE_TRUSTED_CONFIG_PATHS");
    env::remove_var("MISE_DISABLE_TOOLS");
    env::set_var("NO_COLOR", "1");
    env::set_var("MISE_CACHE_PRUNE_AGE", "0");
    env::set_var("MISE_CACHE_DIR", env::HOME.join("data").join("cache"));
    env::set_var("MISE_CONFIG_DIR", env::HOME.join("config"));
    env::set_var("MISE_ENV", "");
    env::set_var("MISE_DATA_DIR", env::HOME.join("data"));
    env::set_var("MISE_GLOBAL_CONFIG_FILE", "~/config/config.toml");
    env::set_var("MISE_SYSTEM_CONFIG_FILE", "doesntexist");
    env::set_var(
        "MISE_OVERRIDE_CONFIG_FILENAMES",
        ".test.mise.toml:test.config.toml",
    );
    env::set_var(
        "MISE_OVERRIDE_TOOL_VERSIONS_FILENAMES",
        ".test-tool-versions",
    );
    env::set_var("MISE_STATE_DIR", env::HOME.join("state"));
    env::set_var("MISE_USE_TOML", "0");
    env::set_var("MISE_YES", "1");
    file::remove_all(&*env::HOME.join("cwd")).unwrap();
    file::create_dir_all(&*env::HOME.join("cwd").join(".mise").join("tasks")).unwrap();
    env::set_current_dir(env::HOME.join("cwd")).unwrap();
    file::write(
        env::HOME.join("config").join("config.toml"),
        indoc! {r#"
            [env]
            TEST_ENV_VAR = 'test-123'

            [alias.tiny.versions]
            "my/alias" = '3.0'

            [tasks.configtask]
            run = 'echo "configtask:"'
            [tasks.lint]
            run = 'echo "linting!"'
            [tasks.test]
            run = 'echo "testing!"'
            [settings]
            always_keep_download = true
            always_keep_install = true
            idiomatic_version_file = true
            plugin_autoupdate_last_check_duration = "20m"
            jobs = 2
            "#},
    )
    .unwrap();
    file::write(
        env::HOME.join(".test-tool-versions"),
        indoc! {r#"
            tiny  2
            dummy ref:master
            "#},
    )
    .unwrap();
    file::write(
        env::current_dir().unwrap().join(".test-tool-versions"),
        indoc! {r#"
            tiny 3
            "#},
    )
    .unwrap();
    file::write(
        ".mise/tasks/filetask",
        indoc! {r#"#!/usr/bin/env bash
        #MISE alias="ft"
        #MISE description="This is a test build script"
        #MISE depends=["lint", "test"]
        #MISE sources=[".test-tool-versions"]
        #MISE outputs=["$MISE_PROJECT_ROOT/test/test-build-output.txt"]
        #MISE env={TEST_BUILDSCRIPT_ENV_VAR = "VALID", BOOLEAN_VAR = true}

        #USAGE flag "--user <user>" help="The user to run as"

        set -exo pipefail
        cd "$MISE_PROJECT_ROOT" || exit 1
        echo "running test-build script"
        echo "TEST_BUILDSCRIPT_ENV_VAR: $TEST_BUILDSCRIPT_ENV_VAR" > test-build-output.txt
        echo "user=$usage_user"
        "#},
    )
    .unwrap();
    file::make_executable(".mise/tasks/filetask").unwrap();
}

pub fn replace_path(input: &str) -> String {
    let path = join_paths(&*env::PATH)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let home = env::HOME.to_string_lossy().to_string();
    input
        .replace(&path, "$PATH")
        .replace(&home, "~")
        .replace(&*env::MISE_BIN.to_string_lossy(), "mise")
}

#[macro_export]
macro_rules! with_settings {
    ($body:block) => {{
        let home = $crate::env::HOME.to_string_lossy().to_string();
        insta::with_settings!({sort_maps => true, filters => vec![
            (home.as_str(), "~"),
        ]}, {$body})
    }}
}
