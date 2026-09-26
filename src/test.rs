use std::env::join_paths;
use std::sync::{Mutex, MutexGuard};

use crate::config::{Settings, SettingsExt};
use crate::env;

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
    crate::testing::init();
}

pub(crate) use mise_util::testing::{EnvVarGuard, lock_ignoring_poison};

/// Held by every test that replaces the process-wide settings, and by every test that reads a
/// setting it needs to stay put. One lock for the whole crate: with a lock per module, a reset in
/// one module's tests silently undid another module's override mid-test. Take it before any
/// environment lock (such as `env_directive::file`'s `ENV_MUTEX`) when a test needs both.
pub(crate) static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Holds [`SETTINGS_LOCK`] and puts the settings back with `Settings::reset(None)` when dropped,
/// including when the test panics, so an override can never leak into a later test.
pub(crate) struct SettingsGuard {
    _lock: MutexGuard<'static, ()>,
}

impl SettingsGuard {
    pub(crate) fn lock() -> Self {
        Self {
            _lock: lock_ignoring_poison(&SETTINGS_LOCK),
        }
    }
}

impl Drop for SettingsGuard {
    fn drop(&mut self) {
        // Runs before `_lock` is released, so the next test starts from clean settings.
        Settings::reset(None);
    }
}

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

    #[test]
    fn a_child_test_process_leaves_the_parent_cwd_intact() {
        const CHILD_ENV: &str = "MISE_TEST_HARNESS_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            return;
        }

        let cwd = std::env::current_dir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "test::tests::a_child_test_process_leaves_the_parent_cwd_intact",
            ])
            .env(CHILD_ENV, "1")
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "child test failed");

        // Had the child reset the fixture tree, this process would be left in an
        // unlinked directory: getcwd fails and relative fixture paths vanish.
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        assert!(std::path::Path::new(".mise/tasks/filetask").is_file());
    }
}
