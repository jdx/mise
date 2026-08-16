use std::env::join_paths;
#[cfg(unix)]
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use indoc::indoc;

use crate::{env, file};

#[ctor::ctor(unsafe)]
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

/// Sets process environment variables for the duration of a test and restores
/// the previous state when dropped — including on an early panic, so a failing
/// assertion can never leak a variable into the rest of the test process.
///
/// Unit tests run single-threaded (`RUST_TEST_THREADS=1` in `.cargo/config.toml`
/// and in the `test:unit` task), so a guarded set/read/restore sequence is not
/// observed by other tests.
#[cfg(unix)]
pub struct EnvVarGuard {
    prev: Vec<(OsString, Option<OsString>)>,
}

#[cfg(unix)]
impl EnvVarGuard {
    pub fn new() -> Self {
        Self { prev: vec![] }
    }

    pub fn set<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, value: V) -> &mut Self {
        let key = key.as_ref().to_os_string();
        self.prev.push((key.clone(), env::var_os(&key)));
        env::set_var(&key, value);
        self
    }

    /// Removes an environment variable for the duration of the guard,
    /// restoring any previous value on drop. Useful for asserting default
    /// behavior even when the variable happens to be set in the caller's
    /// environment.
    pub fn remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Self {
        let key = key.as_ref().to_os_string();
        self.prev.push((key.clone(), env::var_os(&key)));
        env::remove_var(&key);
        self
    }
}

#[cfg(unix)]
impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // restore in reverse so repeated sets of the same key unwind correctly
        for (key, prev) in self.prev.drain(..).rev() {
            match prev {
                Some(value) => env::set_var(&key, value),
                None => env::remove_var(&key),
            }
        }
    }
}

/// Take a test-only global lock, ignoring poisoning.
///
/// These locks are `Mutex<()>`: they guard no data, only the order in which tests reach
/// process-wide state such as `Settings` or environment variables. Restoring that state is the
/// job of each guard's `Drop`, and `Drop` runs while unwinding, so by the time a panicking test
/// releases the lock the state is already back. The poison flag left behind therefore records
/// nothing about correctness — all it does is fail every later test that wanted the same lock.
///
/// Measured once: a single failed assertion in `http::tests` was reported as **29** failures,
/// 28 of them `PoisonError` from tests that had nothing to do with it. Triage cost more than the
/// bug did.
pub fn lock_ignoring_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
