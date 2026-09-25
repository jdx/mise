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
