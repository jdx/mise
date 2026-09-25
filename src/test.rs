use std::env::join_paths;
use std::path::PathBuf;
use std::sync::Mutex;

use indoc::indoc;

use crate::{env, file};

// ctor puts the constructor body in `__TEXT,__text_startup` on Apple targets, a
// section laid out after `__text`. The debug test binary's `__text` is past the
// 128MB reach of an arm64 direct branch, and ld only inserts branch islands
// inside `__text`, so a call from that section into the front of `__text` fails
// to link with "B/BL out of range". Keeping the body in `__text` lets the
// linker island the call like any other.
#[cfg_attr(
    target_vendor = "apple",
    ctor::ctor(unsafe, body(link_section = "__TEXT,__text,regular,pure_instructions"))
)]
#[cfg_attr(not(target_vendor = "apple"), ctor::ctor(unsafe))]
fn init() {
    // Invocations that only list tests (`--list`, which nextest runs twice per
    // binary, concurrently) must not reset the shared fixture tree: one
    // process's remove_all() unlinks the directory another just chdir'd into,
    // and that process then aborts on its next current_dir() call.
    if std::env::args_os().any(|a| a == "--list") {
        return;
    }
    mise_util::testing::enable(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test"));
    crate::config::settings::register_loader();
    crate::cache::register_base_cache_keys();
    // Tests must start from the environment nextest gives their process, not
    // from an activation diff inherited from the process that launched it.
    // This has to happen before the first access to env::HOME initializes
    // PRISTINE_ENV.
    env::remove_var("__MISE_DIFF");
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
    env::set_var("MISE_SYSTEM_CONFIG_FILE", "nonexistent");
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

pub(crate) use mise_util::testing::{EnvVarGuard, lock_ignoring_poison};

pub(crate) fn replace_path(input: &str) -> String {
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

// Last in the file: `clippy::items_after_test_module` rejects anything declared after it.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_lock_is_still_taken() {
        static LOCK: Mutex<()> = Mutex::new(());

        // Poison it for real first. Without this the call below would only show that an
        // unpoisoned mutex can be locked, which is true of `.lock().unwrap()` as well and so
        // proves nothing. The panic message says it is deliberate because it reaches the log.
        let poisoner = std::thread::spawn(|| {
            let _guard = LOCK.lock().unwrap();
            panic!("deliberate panic: poisoning the lock for a_poisoned_lock_is_still_taken");
        });
        assert!(
            poisoner.join().is_err(),
            "the thread had to panic for the lock to be poisoned"
        );
        assert!(LOCK.lock().is_err(), "the lock should now be poisoned");

        // The property: a later test still gets the lock rather than inheriting the failure.
        let _guard = lock_ignoring_poison(&LOCK);
    }
}
