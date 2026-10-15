//! The switch mise's unit-test harness flips so that code in this crate
//! behaves the way it did under `#[cfg(test)]` when it lived in mise.
//!
//! `cfg(test)` is only set for the crate being tested, so it cannot reach a
//! dependency. mise's test constructor calls [`enable`] instead, before
//! anything reads the environment statics.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::env;

static HOME: OnceLock<PathBuf> = OnceLock::new();

/// Run as mise's unit-test harness, with `home` as the home directory.
///
/// Must be called before anything reads [`crate::env::HOME`]. Only the first
/// call takes effect.
pub fn enable(home: PathBuf) {
    let _ = HOME.set(home);
}

/// Whether [`enable`] has been called.
pub fn active() -> bool {
    HOME.get().is_some()
}

/// Whether unit tests are running: this crate's own (`cfg(test)`) or mise's,
/// once its harness has called [`enable`]. Test-only overrides check this so
/// they work from either crate's tests.
pub fn in_tests() -> bool {
    cfg!(test) || active()
}

/// The home directory the test harness chose, if it is running.
pub fn home() -> Option<&'static Path> {
    HOME.get().map(PathBuf::as_path)
}

/// Sets process environment variables for the duration of a test and restores
/// the previous state when dropped — including on an early panic, so a failing
/// assertion can never leak a variable into the rest of the test process.
///
/// Unit tests run single-threaded (`RUST_TEST_THREADS=1` in `.cargo/config.toml`
/// and in the `test:unit` task), so a guarded set/read/restore sequence is not
/// observed by other tests.
pub struct EnvVarGuard {
    prev: Vec<(OsString, Option<OsString>)>,
}

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

impl Default for EnvVarGuard {
    fn default() -> Self {
        Self::new()
    }
}

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

/// A [`SingleReport`](crate::progress::SingleReport) that records every
/// message, for tests that assert what a reporter was told.
#[derive(Clone, Debug, Default)]
pub struct RecordingReport(pub std::sync::Arc<Mutex<Vec<String>>>);

impl crate::progress::SingleReport for RecordingReport {
    fn set_message(&self, message: String) {
        self.0.lock().unwrap().push(message);
    }
}

/// Held by every test that replaces the process-wide settings, and by every test that reads a
/// setting it needs to stay put. One lock for the whole crate: with a lock per module, a reset in
/// one module's tests silently undid another module's override mid-test. Take it before
/// `github::TEST_ENV_LOCK` when a test needs both.
#[cfg(test)]
pub(crate) static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

/// Replace the loaded settings with `overrides` layered over the `MISE_*`
/// environment and the `settings.toml` defaults, or with just those two when
/// `overrides` is `None`. The equivalent, for this crate's tests, of mise's
/// `Settings::reset`, minus config files.
#[cfg(test)]
pub(crate) fn reset_settings(overrides: Option<mise_settings::SettingsPartial>) {
    use confique::Config;
    let mut builder = mise_settings::Settings::builder().env();
    if let Some(overrides) = overrides {
        builder = builder.preloaded(overrides);
    }
    mise_settings::store(std::sync::Arc::new(builder.load().unwrap()));
}

/// Run `$f` (a `fn()`) before `main`, in a binary that links this crate.
///
/// Test harnesses use it to register what the code under test expects `main` to have set up.
/// It is the part of the `ctor` crate mise needs, minus the trampoline `ctor` puts in
/// `__TEXT,__text_startup` on Apple targets. That section is laid out after `__text`; once a
/// debug test binary's `__text` passes 128MB, the trampoline's direct branch into it is out of
/// arm64 range and ld cannot insert a branch island between sections, so the link fails with
/// "B/BL out of range". Here the constructor is an ordinary function in `__text`, and only a
/// pointer to it goes in the platform's initializer section.
#[macro_export]
macro_rules! run_before_main {
    ($f:ident) => {
        const _: () = {
            extern "C" fn run_before_main() {
                $f();
            }

            #[used]
            #[cfg_attr(
                target_vendor = "apple",
                unsafe(link_section = "__DATA,__mod_init_func,mod_init_funcs")
            )]
            #[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
            #[cfg_attr(
                not(any(target_vendor = "apple", windows)),
                unsafe(link_section = ".init_array")
            )]
            static RUN_BEFORE_MAIN: extern "C" fn() = run_before_main;
        };
    };
}

#[cfg(test)]
crate::run_before_main!(register_default_settings);

/// This crate's own unit tests have no mise config system to load settings
/// from, so they read the `settings.toml` defaults, and no mise build to key
/// caches on.
#[cfg(test)]
fn register_default_settings() {
    mise_settings::set_loader(mise_settings::load_defaults);
    crate::cache::set_base_cache_keys(vec!["mise-util-tests".to_string()]);
    crate::user_agent::set("mise-util-tests".to_string());
    crate::deprecation::set_version("0.0.0");
    crate::env::set_mise_env(|| &[]);
    crate::shells::set_implicit_inline_shell(|| true);
}
